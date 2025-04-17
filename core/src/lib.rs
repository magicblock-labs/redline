use std::fmt;
use std::{error::Error, path::PathBuf};

use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize,
};

pub type DynError = Box<dyn Error + 'static>;
pub type BenchResult<T> = Result<T, DynError>;

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[serde(skip_serializing)]
    pub connection: ConnectionSettings,
    pub benchmark: BenchmarkSettings,
    pub subscription: SubscriptionSettings,
    pub data: DataSettings,
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
pub struct BenchmarkSettings {
    pub iterations: u64,
    pub tps: u32,
    pub concurrency: usize,
    pub preflight_check: bool,
    pub parallelism: u8,
    pub mode: BenchMode,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct SubscriptionSettings {
    pub subscribe_to_accounts: bool,
    pub subscribe_to_signatures: bool,
    pub enforce_total_sync: bool,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct DataSettings {
    pub account_encoding: AccountEncoding,
    pub account_size: AccountSize,
}
#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum BenchMode {
    SimpleByteSet,
    TriggerClones {
        clone_frequency_secs: u64,
        accounts_count: u8,
    },
    HighCuCost {
        iters: u32,
    },
    ReadWrite {
        accounts_pool_size: u8,
    },
    Mixed(Vec<Self>),
}

#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionType {
    Http1,
    Http2,
}

#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
#[repr(u32)]
pub enum AccountSize {
    BYTES128 = 128,
    BYTES512 = 512,
    BYTES2048 = 2048,
    BYTES8192 = 8192,
}

#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum AccountEncoding {
    Base58,
    Base64,
    #[serde(rename = "base64+zstd")]
    Base64Zstd,
}

impl AccountEncoding {
    pub fn as_str(&self) -> &str {
        match self {
            AccountEncoding::Base58 => "base58",
            AccountEncoding::Base64 => "base64",
            AccountEncoding::Base64Zstd => "base64+zstd",
        }
    }
}

#[derive(Clone)]
pub struct Url(pub hyper::Uri);

impl Url {
    pub fn address(&self, ws: bool) -> String {
        let host = self.host();
        let port = self.0.port_u16().map(|p| p + ws as u16).unwrap_or(80);

        format!("{}:{}", host, port)
    }

    pub fn host(&self) -> &str {
        self.0.host().expect("uri has no host")
    }
}

impl<'de> Deserialize<'de> for Url {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UrlVisitor;

        impl Visitor<'_> for UrlVisitor {
            type Value = Url;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid URI string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Url, E>
            where
                E: de::Error,
            {
                value
                    .parse::<hyper::Uri>()
                    .map(Url)
                    .map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_str(UrlVisitor)
    }
}
