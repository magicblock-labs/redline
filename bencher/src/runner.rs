use hyper::Request;
use keypair::Keypair;
use signer::Signer;
use tokio::sync::oneshot;

use crate::{
    blockhash::BlockHashProvider,
    config::Config,
    confirmation::{Confirmations, ConfirmationsBundle, ConfirmationsDB, EventConfirmer},
    extractor::{
        account_update_extractor, signature_response_extractor, signature_status_extractor,
    },
    http::{Connection, ConnectionPool},
    payload,
    tps::TpsManager,
    transaction::TransactionProvider,
    websocket::{Subscription, WebsocketPool},
    BenchResult, ShutDown,
};

pub struct BenchRunner {
    transaction_provider: Box<dyn TransactionProvider>,
    blockhash: BlockHashProvider,
    signer: Keypair,

    chain: Connection,
    ephem: ConnectionPool,

    signatures_websocket: WebsocketPool<bool>,

    account_confirmations: ConfirmationsDB<u64>,
    signature_confirmations: ConfirmationsDB<bool>,
    delivery_confirmations: ConfirmationsDB<()>,

    tps_manager: TpsManager,

    iterations: u64,

    preflight_check: bool,
    subscribe_to_accounts: bool,
    subscribe_to_signatures: bool,
    enforce_total_sync: bool,

    _shutdown: ShutDown,
}

impl BenchRunner {
    pub async fn new(signer: Keypair, config: &Config) -> BenchResult<Self> {
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
        let blockhash = BlockHashProvider::new(ephem)
            .await
            .inspect_err(|err| eprintln!("failed to create blockhash provider: {err}"))?;
        let ephem = ConnectionPool::new(&config.connection).await?;

        let tps_manager = TpsManager::new(config.benchmark.concurrency, config.benchmark.tps);
        let shutdown = ShutDown::default();

        let signatures_websocket = WebsocketPool::new(
            &config.connection,
            signature_status_extractor,
            shutdown.clone(),
        )
        .await?;

        let account_updates_confirmer = EventConfirmer::new();
        let account_confirmations = account_updates_confirmer.db.clone();
        tokio::task::spawn_local(account_updates_confirmer.confirm_by_value());

        let signatures_confirmer = EventConfirmer::new();
        let signature_confirmations = signatures_confirmer.db.clone();
        tokio::task::spawn_local(signatures_confirmer.confirm_by_id());

        let (delivery_confirmations, _) = Confirmations::new();

        let transaction_provider = crate::transaction::make_provider(
            &config.benchmark.mode,
            signer.pubkey(),
            config.data.account_size as u32,
        );

        let subscribe_to_accounts = config.subscription.subscribe_to_accounts;

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
                    payload: payload::accountsub(pk, encoding, id),
                    oneshot: false,
                    id,
                };
                let _ = con.send(sub).await;
            }
        }

        Ok(Self {
            transaction_provider,
            blockhash,
            signer,
            chain,
            ephem,
            tps_manager,
            signatures_websocket,
            iterations: config.benchmark.iterations,
            account_confirmations,
            signature_confirmations,
            delivery_confirmations,
            subscribe_to_accounts,
            subscribe_to_signatures: config.subscription.subscribe_to_signatures,
            enforce_total_sync: config.subscription.enforce_total_sync,
            preflight_check: config.benchmark.preflight_check,
            _shutdown: shutdown,
        })
    }

    pub async fn run(mut self) -> ConfirmationsBundle {
        for i in 0..self.iterations {
            self.transaction_provider.bookkeep(&mut self.chain, i);
            self.step(i).await;
        }
        ConfirmationsBundle {
            accounts_updates: self.account_confirmations,
            signature_confirmations: self.signature_confirmations,
            delivery_confirmations: self.delivery_confirmations,
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
        let request = Request::new(payload::transaction(&txn, self.preflight_check));
        let response = con.send(request, signature_response_extractor);
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
                payload: payload::signaturesub(&txn, id),
                oneshot: false,
                id,
            };
            let _ = con.send(sub).await;
        }
        let (account_rx, signature_rx) = (
            maybe_subscribe!(self.subscribe_to_accounts, self.account_confirmations),
            maybe_subscribe!(self.subscribe_to_signatures, self.signature_confirmations),
        );
        let delivery = self.delivery_confirmations.clone();
        delivery.borrow_mut().track(id, None);

        let task = async move {
            match response.resolve().await {
                Ok(Some(true)) => {
                    eprintln!("transaction failed to executed");
                }
                Err(err) => {
                    eprintln!("transaction failed to be delivered: {err}");
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
        };
        tokio::task::spawn_local(task);
    }
}
