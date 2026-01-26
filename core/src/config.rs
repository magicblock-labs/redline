use crate::types::{AccountEncoding, AccountSize, BenchMode, BenchResult, ConnectionType, Url};
use pubkey::Pubkey;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use std::path::PathBuf;

/// # Redline Configuration
///
/// This structure holds all the configuration parameters for the Redline benchmark tool.
/// It is typically loaded from a TOML file.
#[serde_as]
#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    /// Indicates whether ER is running in gasless mode
    pub gasless: bool,
    /// ## Connection Settings
    ///
    /// Defines the network parameters for connecting to the Solana cluster.
    #[serde(skip_serializing)]
    pub connection: ConnectionSettings,
    /// ## Benchmark Settings
    ///
    /// Configures the benchmark execution, including the mode, duration, and concurrency.
    pub benchmark: BenchmarkSettings,
    /// ## Confirmation Settings
    ///
    /// Specifies how to handle transaction and event confirmations during the benchmark.
    pub confirmations: ConfirmationSettings,
    /// ## Data Settings
    ///
    /// Configures the parameters for account data, such as encoding and size.
    pub data: DataSettings,
    /// ## Parallelism
    ///
    /// Determines how many concurrent benchmarks to run, each on its own thread.
    pub parallelism: u8,
    /// ## Payers/Signers
    ///
    /// Indicates the number of different payers/signers to use when sending transactions.
    pub payers: u8,
    /// ## Keypairs path
    ///
    /// Path to keypairs directory where vault and signer keypairs are stored
    pub keypairs: PathBuf,
    /// ## ER Authority/Identity
    ///
    /// Authority/Identity of the validator, used to delegate the accounts
    #[serde_as(as = "DisplayFromStr")]
    pub authority: Pubkey,
}

impl Config {
    /// # Load from Path
    ///
    /// Loads the configuration from a specified file path.
    ///
    /// ### Arguments
    ///
    /// * `path` - A `PathBuf` to the TOML configuration file.
    pub fn from_path(path: PathBuf) -> BenchResult<Self> {
        let config = std::fs::read_to_string(path)?;
        toml::from_str(&config).map_err(Into::into)
    }

    /// # Load from Arguments
    ///
    /// Loads the configuration from a command-line argument.
    /// Expects the first argument to be the path to the configuration file.
    pub fn from_args() -> BenchResult<Self> {
        let path = std::env::args()
            .nth(1)
            .ok_or("usage: redline config.toml")?
            .into();
        tracing::info!("using config file at {path:?} to run the benchmark");
        Self::from_path(path)
    }
}

/// # Connection Settings
///
/// Holds the network configuration for connecting to the Solana cluster.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ConnectionSettings {
    /// The URL of the main chain node.
    pub chain_url: Url,
    /// The URL of the ephemeral node.
    pub ephem_url: Url,
    /// The type of HTTP connection to use (`http1` or `http2`).
    pub http_connection_type: ConnectionType,
    /// The maximum number of HTTP connections to establish.
    pub http_connections_count: usize,
    /// The maximum number of WebSocket connections to establish.
    pub ws_connections_count: usize,
}

/// # Benchmark Settings
///
/// Configures the execution of the benchmark, including the mode, load, and duration.
#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct BenchmarkSettings {
    /// ## Iterations
    ///
    /// The total number of requests or transactions to send.
    pub iterations: u64,
    /// ## Rate
    ///
    /// The desired rate of requests or transactions per second (RPS/TPS).
    pub rate: u32,
    /// ## Concurrency
    ///
    /// The number of concurrent tasks to use for sending requests.
    pub concurrency: usize,
    /// ## Preflight Check
    ///
    /// A flag to enable or disable the preflight check for transactions.
    pub preflight_check: bool,
    /// ## Clone frequency
    ///
    /// The frequency in milliseconds, at which the account cloning should be triggered.
    pub clone_frequency_ms: u64,
    /// ## Accounts Count
    ///
    /// The number of accounts to use for RPC-based benchmarks.
    pub accounts_count: u8,
    /// ## Mode
    ///
    /// The benchmark mode to execute, which can be a single mode or a mix of modes.
    pub mode: BenchMode,
}

/// # Confirmation Settings
///
/// Specifies how to handle transaction and event confirmations.
#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ConfirmationSettings {
    /// Subscribe to account notifications for transaction confirmations.
    pub subscribe_to_accounts: bool,
    /// Subscribe to signature notifications for transaction confirmations.
    pub subscribe_to_signatures: bool,
    /// Use `getSignatureStatuses` for transaction confirmations.
    pub get_signature_status: bool,
    /// Enforce total synchronization, ensuring all confirmations are received before completing a transaction.
    pub enforce_total_sync: bool,
}

/// # Data Settings
///
/// Configures the parameters for account data used in the benchmark.
#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct DataSettings {
    /// The encoding for account data.
    pub account_encoding: AccountEncoding,
    /// The size of the accounts to be created.
    pub account_size: AccountSize,
}
