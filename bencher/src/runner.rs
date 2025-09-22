use crate::{
    blockhash::BlockHashProvider,
    confirmation::{Confirmations, ConfirmationsDB, EventConfirmer},
    extractor::{account_update_extractor, signature_status_extractor_ws},
    http::{Connection, ConnectionPool},
    payload,
    rate::RateManager,
    requests::{make_builder, RequestBuilder},
    transfer::TransferManager,
    websocket::{Subscription, WebsocketPool},
    BenchResult, ShutDown, ShutDownSender,
};
use core::{
    config::Config,
    stats::{BenchStatistics, ObservationsStats},
};
use keypair::Keypair;
use signer::EncodableKey;
use std::{collections::HashMap, rc::Rc, time::Duration};
use tokio::sync::oneshot;

/// # Bench Runner
///
/// The unified benchmark runner, capable of handling both TPS and RPS benchmarks.
pub struct BenchRunner {
    request_builder: Box<dyn RequestBuilder>,
    ephem: ConnectionPool,
    signatures_websocket: WebsocketPool<bool>,
    account_confirmations: ConfirmationsDB<u64>,
    signature_confirmations: ConfirmationsDB<bool>,
    delivery_confirmations: HashMap<&'static str, ConfirmationsDB<()>>,
    transfer_manager: TransferManager,
    rate_manager: RateManager,
    config: Config,
    shutdown: ShutDown,
}

type AccountConfirmationReceiver = Option<oneshot::Receiver<u64>>;
type SignatureConfirmationReceiver = Option<oneshot::Receiver<bool>>;

impl BenchRunner {
    /// # New Bench Runner
    ///
    /// Creates a new `BenchRunner` instance, initializing all the necessary components.
    pub async fn new(signer: Keypair, config: Config) -> BenchResult<Self> {
        let ephem_conn = Connection::new(
            &config.connection.ephem_url,
            config.connection.http_connection_type,
        )
        .await?;
        let shutdown = ShutDownSender::init();

        let blockhash_provider = BlockHashProvider::new(ephem_conn, shutdown.listener()).await?;
        let ephem = ConnectionPool::new(&config.connection).await?;
        let rate_manager = RateManager::new(config.benchmark.concurrency, config.benchmark.rate);

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

        let request_builder = make_builder(
            &config.benchmark.mode,
            &config,
            signer,
            blockhash_provider.clone(),
        );

        let accounts = request_builder.accounts();
        if config.confirmations.subscribe_to_accounts {
            let mut accounts_websocket = WebsocketPool::new(
                &config.connection,
                account_update_extractor,
                shutdown.clone(),
            )
            .await?;
            let encoding = config.data.account_encoding;
            for (id, pk) in accounts.iter().enumerate() {
                let id = id as u64;
                let tx = account_confirmations.borrow().tx.clone();
                let con = accounts_websocket.connection();
                let sub = Subscription {
                    tx,
                    payload: payload::account_subscription(*pk, encoding, id),
                    oneshot: false,
                    id,
                };
                let _ = con.send(sub).await;
            }
            tokio::time::sleep(Duration::from_secs(1)).await
        }
        let vault =
            Keypair::read_from_file("keypairs/vault.json").expect("failed to read vault keypair");
        let transfer_manager =
            TransferManager::new(&config, vault, &accounts, blockhash_provider).await;

        Ok(Self {
            request_builder,
            ephem,
            signatures_websocket,
            account_confirmations,
            signature_confirmations,
            delivery_confirmations: HashMap::new(),
            rate_manager,
            transfer_manager,
            config,
            shutdown,
        })
    }

    /// # Run Benchmark
    ///
    /// Starts the benchmark, sending requests at the configured rate.
    pub async fn run(mut self) -> BenchResults {
        for i in 0..self.config.benchmark.iterations {
            // this will trigger an account update on chain and subsequent clone on ER
            self.transfer_manager.transfer();

            self.step(i).await;
        }
        tracing::info!(
            iterations = self.config.benchmark.iterations,
            "The benchmark run is complete",
        );

        BenchResults {
            config: self.config,
            delivery_confirmations: self.delivery_confirmations,
            account_confirmations: self.account_confirmations,
            signature_confirmations: self.signature_confirmations,
            rate: self.rate_manager.stats(),
        }
    }

    #[inline(always)]
    async fn step(&mut self, id: u64) {
        let mut con = self.ephem.connection().await.expect("connection closed");
        let permit = self.rate_manager.tick().await;

        let request = self.request_builder.build(id);
        let request_name = self.request_builder.name();
        let extractor = self.request_builder.extractor();

        let delivery = self
            .delivery_confirmations
            .entry(request_name)
            .or_insert_with(|| Confirmations::new().0)
            .clone();

        let is_transaction = self.request_builder.signature().is_some();
        let response = con.send(request, extractor);
        drop(con);
        let (account_rx, signature_rx) = self.subscribe_if_needed(id, is_transaction).await;

        delivery.borrow_mut().track(id, None);

        let shutdown = self.shutdown.clone();
        let total_sync = self.config.confirmations.enforce_total_sync;
        tokio::task::spawn_local(async move {
            match response.resolve().await {
                Ok(Some(false)) => tracing::warn!("request failed to be executed"),
                Err(err) => tracing::error!("request failed to be delivered: {err}"),
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
            drop(shutdown);
        });
    }

    async fn subscribe_if_needed(
        &mut self,
        id: u64,
        is_transaction: bool,
    ) -> (AccountConfirmationReceiver, SignatureConfirmationReceiver) {
        let total_sync = self.config.confirmations.enforce_total_sync;
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

        if is_transaction {
            if self.config.confirmations.subscribe_to_signatures {
                if let Some(signature) = self.request_builder.signature() {
                    let con = self.signatures_websocket.connection();
                    let tx = self.signature_confirmations.borrow().tx.clone();
                    let sub = Subscription {
                        tx,
                        payload: payload::signature_subscription(signature, id),
                        oneshot: true,
                        id,
                    };
                    let _ = con.send(sub).await;
                }
            }

            let account_rx = maybe_subscribe!(
                self.config.confirmations.subscribe_to_accounts,
                self.account_confirmations
            );
            let signature_rx = maybe_subscribe!(
                self.config.confirmations.subscribe_to_signatures,
                self.signature_confirmations
            );
            (account_rx, signature_rx)
        } else {
            (None, None)
        }
    }
}

/// # Benchmark Results
///
/// Holds the results of the benchmark run, including all collected statistics.
pub struct BenchResults {
    config: Config,
    account_confirmations: ConfirmationsDB<u64>,
    signature_confirmations: ConfirmationsDB<bool>,
    delivery_confirmations: HashMap<&'static str, ConfirmationsDB<()>>,
    rate: ObservationsStats,
}

impl BenchResults {
    /// # Calculate Statistics
    ///
    /// Finalizes the benchmark results and calculates the statistics.
    pub fn stats(self) -> BenchStatistics {
        macro_rules! finalize {
            ($confirmation: expr) => {
                Rc::try_unwrap($confirmation)
                    .unwrap()
                    .into_inner()
                    .finalize()
            };
        }

        let mut request_stats = HashMap::new();

        for (mode_name, confirmations) in self.delivery_confirmations {
            request_stats.insert(mode_name.to_string(), finalize!(confirmations));
        }

        BenchStatistics {
            configuration: json::to_value(&self.config).unwrap(),
            request_stats,
            signature_confirmation_latency: finalize!(self.signature_confirmations),
            account_update_latency: finalize!(self.account_confirmations),
            rps: self.rate,
        }
    }
}
