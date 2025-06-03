use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::types::{
    AccountEncoding, AccountSize, BenchResult, ConnectionType, RpsBenchMode, TpsBenchMode, Url,
};

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub connection: ConnectionSettings,
    pub tps_benchmark: TransactionBenchmarkSettings,
    pub rps_benchmark: RoRequestBenchmarkSettings,
    pub confirmations: ConfirmationSettings,
    pub data: DataSettings,
    pub parallelism: u8,
}

impl Config {
    pub fn from_path(path: PathBuf) -> BenchResult<Self> {
        let config = std::fs::read_to_string(path)?;
        toml::from_str(&config).map_err(Into::into)
    }
    pub fn from_args() -> BenchResult<Self> {
        let path = std::env::args()
            .nth(1)
            .ok_or("usage: redline config.toml")?
            .into();
        Self::from_path(path)
    }
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ConnectionSettings {
    #[serde(skip_serializing)]
    pub chain_url: Url,
    #[serde(skip_serializing)]
    pub ephem_url: Url,
    pub http_connection_type: ConnectionType,
    pub http_connections_count: usize,
    pub ws_connections_count: usize,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct TransactionBenchmarkSettings {
    pub enabled: bool,
    pub iterations: u64,
    pub tps: u32,
    pub concurrency: usize,
    pub preflight_check: bool,
    pub mode: TpsBenchMode,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct RoRequestBenchmarkSettings {
    pub enabled: bool,
    pub iterations: u64,
    pub rps: u32,
    pub accounts_count: u8,
    pub concurrency: usize,
    pub mode: RpsBenchMode,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ConfirmationSettings {
    pub subscribe_to_accounts: bool,
    pub subscribe_to_signatures: bool,
    pub get_signature_status: bool,
    pub enforce_total_sync: bool,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct DataSettings {
    pub account_encoding: AccountEncoding,
    pub account_size: AccountSize,
}
