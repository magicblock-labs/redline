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
use std::{
    collections::HashMap,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::oneshot;

/// # Bench Runner
///
/// The unified benchmark runner, capable of handling both TPS and RPS benchmarks. It
/// is the main orchestrator of the benchmarking process, responsible for creating and
/// managing all the necessary components, sending requests, and collecting statistics.
pub struct BenchRunner {
    /// The request builder, which creates the transactions or RPC requests to be sent.
    request_builder: Box<dyn RequestBuilder>,
    /// A pool of HTTP connections to the ephemeral node.
    ephem: ConnectionPool,
    /// A pool of WebSocket connections for signature subscriptions.
    signatures_websocket: WebsocketPool<bool>,
    /// A database for tracking account update confirmations.
    account_confirmations: ConfirmationsDB<u64>,
    /// A database for tracking signature confirmations.
    signature_confirmations: ConfirmationsDB<bool>,
    /// A map of databases for tracking the delivery of each type of request.
    delivery_confirmations: HashMap<&'static str, ConfirmationsDB<()>>,
    /// A manager for periodically transferring lamports to trigger account cloning.
    transfer_manager: TransferManager,
    /// A manager for controlling the rate of requests per second
    rate_manager: RateManager,
    /// The benchmark configuration.
    config: Config,
    /// A mechanism for gracefully shutting down the benchmark.
    shutdown: ShutDown,
    /// Shared benchmark progress indicator value
    progress: Arc<AtomicU64>,
}

type AccountConfirmationReceiver = Option<oneshot::Receiver<u64>>;
type SignatureConfirmationReceiver = Option<oneshot::Receiver<bool>>;

impl BenchRunner {
    /// # New Bench Runner
    ///
    /// Creates a new `BenchRunner` instance, initializing all the necessary components.
    pub async fn new(
        signer: Keypair,
        config: Config,
        progress: Arc<AtomicU64>,
    ) -> BenchResult<Self> {
        // Create a new HTTP connection to the ephemeral node. This is used for fetching the blockhash.
        let ephem_conn = Connection::new(
            &config.connection.ephem_url,
            config.connection.http_connection_type,
        )
        .await?;
        // Initialize the shutdown signal handler.
        let shutdown = ShutDownSender::init();

        // Create a new blockhash provider to keep the blockhash updated.
        let blockhash_provider = BlockHashProvider::new(ephem_conn, shutdown.listener()).await?;
        // Create a new pool of HTTP connections to the ephemeral node.
        let ephem = ConnectionPool::new(&config.connection).await?;
        // Create a new rate manager to control the request rate.
        let rate_manager = RateManager::new(config.benchmark.concurrency, config.benchmark.rate);

        // Create a new pool of WebSocket connections for signature subscriptions.
        let signatures_websocket = WebsocketPool::new(
            &config.connection,
            signature_status_extractor_ws,
            shutdown.clone(),
        )
        .await?;

        // This confirmer will track account updates via WebSocket subscriptions.
        let account_updates_confirmer = EventConfirmer::new(shutdown.listener());
        let account_confirmations = account_updates_confirmer.db.clone();
        tokio::task::spawn_local(account_updates_confirmer.confirm_by_value());

        // This confirmer will track signature confirmations via WebSocket subscriptions.
        let signatures_confirmer = EventConfirmer::new(shutdown.listener());
        let signature_confirmations = signatures_confirmer.db.clone();
        tokio::task::spawn_local(signatures_confirmer.confirm_by_id());

        // The request builder creates the transactions or RPC requests to be sent.
        let request_builder = make_builder(
            &config.benchmark.mode,
            &config,
            signer,
            blockhash_provider.clone(),
        );

        let accounts = request_builder.accounts();
        if config.confirmations.subscribe_to_accounts {
            // Create a new pool of WebSocket connections for account update subscriptions.
            let mut accounts_websocket = WebsocketPool::new(
                &config.connection,
                account_update_extractor,
                shutdown.clone(),
            )
            .await?;
            let encoding = config.data.account_encoding;
            // Subscribe to account updates for all the accounts used in the benchmark.
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
        // The vault is a pre-funded account that is used to trigger account cloning.
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
            progress,
        })
    }

    /// # Run Benchmark
    ///
    /// Starts the benchmark, sending requests at the configured rate.
    pub async fn run(mut self) -> BenchResults {
        for i in 0..self.config.benchmark.iterations {
            // This will trigger an account update on the main chain, which in turn
            // will trigger an account clone on the Ephemeral Rollup.
            self.transfer_manager.transfer();

            self.step(i).await;
            // report progress
            self.progress.fetch_add(1, Ordering::Relaxed);
        }

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
        // Get a connection from the pool.
        let mut con = self.ephem.connection().await.expect("connection closed");
        // Get a permit from the rate manager to send a request.
        let permit = self.rate_manager.tick().await;

        // Build the request.
        let request = self.request_builder.build(id);
        let request_name = self.request_builder.name();
        let extractor = self.request_builder.extractor();

        // Get the confirmation database for this request type.
        let delivery = self
            .delivery_confirmations
            .entry(request_name)
            .or_insert_with(|| Confirmations::new().0)
            .clone();

        let is_transaction = self.request_builder.signature().is_some();
        let response = con.send(request, extractor);
        drop(con);
        // Subscribe to confirmations if needed.
        let (account_rx, signature_rx) = self.subscribe_if_needed(id, is_transaction).await;

        // Track the delivery of the request.
        delivery.borrow_mut().track(id, None);

        // Spawn a new task to handle the response and confirmations.
        let shutdown = self.shutdown.clone();
        let total_sync = self.config.confirmations.enforce_total_sync;
        tokio::task::spawn_local(async move {
            match response.resolve().await {
                Ok(Some(false)) => tracing::warn!("request failed to be executed"),
                Err(err) => tracing::error!("request failed to be delivered: {err}"),
                _ => (),
            }
            // Observe the delivery of the request.
            delivery.borrow_mut().observe(id, ());
            // If total sync is not enforced, drop the permit to allow other requests to be sent.
            if !total_sync {
                drop(permit);
            }
            // Wait for the account update confirmation, if subscribed.
            if let Some(rx) = account_rx {
                let _ = rx.await;
            }
            // Wait for the signature confirmation, if subscribed.
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
        // A macro to conditionally subscribe to a confirmation feed. If total_sync is enabled,
        // it creates a new oneshot channel and passes the sender to the confirmation database.
        // Otherwise, it just tracks the confirmation without creating a channel.
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
            // If signature subscriptions are enabled, subscribe to the signature of the transaction.
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
        // A macro to finalize the statistics of a confirmation database. It unwraps
        // the Rc and RefCell to get the inner Confirmations struct and then calls
        // the finalize method to calculate the statistics.
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
