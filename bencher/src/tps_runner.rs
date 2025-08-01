use core::{
    config::Config,
    stats::{ObservationsStats, TpsBenchStatistics},
};
use std::{rc::Rc, time::Duration};

use hyper::Request;
use keypair::Keypair;
use signer::Signer;
use tokio::sync::oneshot;

use crate::{
    blockhash::BlockHashProvider,
    confirmation::{Confirmations, ConfirmationsDB, EventConfirmer},
    extractor::{
        account_update_extractor, signature_response_extractor, signature_status_extractor_ws,
    },
    http::{Connection, ConnectionPool},
    payload,
    rps::RpsManager,
    transaction::TransactionProvider,
    websocket::{Subscription, WebsocketPool},
    BenchResult, ShutDown, ShutDownSender,
};

pub struct TpsBenchRunner {
    transaction_provider: Box<dyn TransactionProvider>,
    blockhash: BlockHashProvider,
    signer: Keypair,

    chain: Connection,
    ephem: ConnectionPool,

    signatures_websocket: WebsocketPool<bool>,

    account_confirmations: ConfirmationsDB<u64>,
    signature_confirmations: ConfirmationsDB<bool>,
    delivery_confirmations: ConfirmationsDB<()>,

    tps_manager: RpsManager,

    iterations: u64,

    preflight_check: bool,
    subscribe_to_accounts: bool,
    subscribe_to_signatures: bool,
    enforce_total_sync: bool,

    shutdown: ShutDown,
    config: json::Value,
}

impl TpsBenchRunner {
    pub async fn new(signer: Keypair, config: Config) -> BenchResult<Self> {
        let chain = Connection::new(
            &config.connection.chain_url,
            config.connection.http_connection_type,
        )
        .await?;
        let ephem = Connection::new(
            &config.connection.ephem_url,
            config.connection.http_connection_type,
        )
        .await?;
        let shutdown = ShutDownSender::init();

        let blockhash = BlockHashProvider::new(ephem, shutdown.listener())
            .await
            .inspect_err(|error| tracing::error!(error, "failed to create blockhash provider"))?;
        let ephem = ConnectionPool::new(&config.connection).await?;

        let tps_manager =
            RpsManager::new(config.tps_benchmark.concurrency, config.tps_benchmark.tps);

        let signatures_websocket = WebsocketPool::new(
            &config.connection,
            signature_status_extractor_ws,
            shutdown.clone(),
        )
        .await?;

        let account_updates_confirmer = EventConfirmer::new(shutdown.listener());
        let account_confirmations = account_updates_confirmer.db.clone();
        tokio::task::spawn_local(account_updates_confirmer.confirm_by_value());

        let signatures_confirmer = EventConfirmer::new(shutdown.listener());
        let signature_confirmations = signatures_confirmer.db.clone();
        tokio::task::spawn_local(signatures_confirmer.confirm_by_id());

        let (delivery_confirmations, _) = Confirmations::new();

        let transaction_provider = crate::transaction::make_provider(
            &config.tps_benchmark.mode,
            signer.pubkey(),
            config.data.account_size as u32,
        );

        let subscribe_to_accounts = config.confirmations.subscribe_to_accounts;

        if subscribe_to_accounts {
            let mut accounts_websocket = WebsocketPool::new(
                &config.connection,
                account_update_extractor,
                shutdown.clone(),
            )
            .await?;
            let encoding = config.data.account_encoding;
            for (id, pk) in transaction_provider.accounts().into_iter().enumerate() {
                let id = id as u64;
                let tx = account_confirmations.borrow().tx.clone();
                let con = accounts_websocket.connection();
                let sub = Subscription {
                    tx,
                    payload: payload::account_subscription(pk, encoding, id),
                    oneshot: false,
                    id,
                };
                let _ = con.send(sub).await;
            }
            tokio::time::sleep(Duration::from_secs(1)).await
        }

        Ok(Self {
            transaction_provider,
            blockhash,
            signer,
            chain,
            ephem,
            tps_manager,
            signatures_websocket,
            iterations: config.tps_benchmark.iterations,
            account_confirmations,
            signature_confirmations,
            delivery_confirmations,
            subscribe_to_accounts,
            subscribe_to_signatures: config.confirmations.subscribe_to_signatures,
            enforce_total_sync: config.confirmations.enforce_total_sync,
            preflight_check: config.tps_benchmark.preflight_check,
            shutdown,
            config: json::to_value(&config).unwrap(),
        })
    }

    pub async fn run(mut self) -> TpsBenchResults {
        for i in 0..self.iterations {
            self.transaction_provider.bookkeep(&mut self.chain, i);
            self.step(i).await;
        }
        tracing::info!(
            iterations = self.iterations,
            "The TPS Benchmark run is complete",
        );

        TpsBenchResults {
            configuration: self.config,
            delivery_confirmations: self.delivery_confirmations,
            account_confirmations: self.account_confirmations,
            signature_confirmations: self.signature_confirmations,
            tps: self.tps_manager.stats(),
        }
    }

    #[inline(always)]
    async fn step(&mut self, id: u64) {
        let mut con = self.ephem.connection().await.expect("connection closed");
        let permit = self.tps_manager.tick().await;

        let blockhash = self.blockhash.hash();
        let txn = self
            .transaction_provider
            .generate(id, blockhash, &self.signer);
        let total_sync = self.enforce_total_sync;
        macro_rules! maybe_subscribe {
            ($subscribe:expr, $confirmations:expr) => {
                if $subscribe && total_sync {
                    let (tx, rx) = oneshot::channel();
                    $confirmations.borrow_mut().track(id, Some(tx));
                    Some(rx)
                } else {
                    if $subscribe {
                        $confirmations.borrow_mut().track(id, None);
                    }
                    None
                }
            };
        }

        if self.subscribe_to_signatures {
            let con = self.signatures_websocket.connection();
            let tx = self.signature_confirmations.borrow().tx.clone();
            let sub = Subscription {
                tx,
                payload: payload::signature_subscription(&txn, id),
                oneshot: true,
                id,
            };
            let _ = con.send(sub).await;
        }
        let (account_rx, signature_rx) = (
            maybe_subscribe!(self.subscribe_to_accounts, self.account_confirmations),
            maybe_subscribe!(self.subscribe_to_signatures, self.signature_confirmations),
        );
        let request = Request::new(payload::transaction(&txn, self.preflight_check));
        let response = con.send(request, signature_response_extractor);
        let delivery = self.delivery_confirmations.clone();
        delivery.borrow_mut().track(id, None);

        let shutdown = self.shutdown.clone();
        let task = async move {
            match response.resolve().await {
                Ok(Some(false)) => {
                    tracing::warn!("transaction failed to be executed");
                }
                Err(err) => {
                    tracing::error!("transaction failed to be delivered: {err}");
                }
                _ => (),
            }
            delivery.borrow_mut().observe(id, ());
            if !total_sync {
                drop(permit);
            }
            if let Some(rx) = account_rx {
                let _ = rx.await;
            }
            if let Some(rx) = signature_rx {
                let _ = rx.await;
            }
            drop(shutdown)
        };
        tokio::task::spawn_local(task);
    }
}

pub struct TpsBenchResults {
    configuration: json::Value,
    account_confirmations: ConfirmationsDB<u64>,
    signature_confirmations: ConfirmationsDB<bool>,
    delivery_confirmations: ConfirmationsDB<()>,
    tps: ObservationsStats,
}

impl TpsBenchResults {
    pub fn stats(self) -> TpsBenchStatistics {
        macro_rules! finalize {
            ($confirmation: expr) => {
                Rc::try_unwrap($confirmation)
                    .unwrap()
                    .into_inner()
                    .finalize()
            };
        }
        TpsBenchStatistics {
            configuration: self.configuration,
            account_update_latency: finalize!(self.account_confirmations),
            signature_confirmation_latency: finalize!(self.signature_confirmations),
            send_txn_requests_latency: finalize!(self.delivery_confirmations),
            transactions_per_second: self.tps,
        }
    }
}
