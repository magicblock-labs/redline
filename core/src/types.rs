use std::error::Error;
use std::fmt;

use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize,
};

/// A type alias for a dynamic error, commonly used for benchmark results.
pub type DynError = Box<dyn Error + 'static>;
/// A type alias for a result that can return a dynamic error, used for benchmark outcomes.
pub type BenchResult<T> = Result<T, DynError>;

/// Defines the modes for benchmarking, covering both TPS and RPS scenarios.
#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum BenchMode {
    /// **(TPS)** Writes a small set of bytes to a specified number of accounts.
    SimpleByteSet,
    /// **(TPS)** Executes transactions with high computational cost to stress the validator's processing capacity.
    HighCuCost { iters: u32 },
    /// **(TPS)** Performs read and write operations across a set of accounts to test for lock contention.
    ReadWrite,
    /// **(TPS)** Executes read-only transactions to measure parallel processing performance.
    #[serde(rename_all = "kebab-case")]
    ReadOnly { accounts_per_transaction: u8 },
    /// **(TPS)** Sends commit transactions to the Ephemeral Rollup (ER) to test state-committing performance.
    #[serde(rename_all = "kebab-case")]
    Commit { accounts_per_transaction: u8 },

    /// **(RPS)** Fetches account information for a single account.
    GetAccountInfo,
    /// **(RPS)** Fetches account information for multiple accounts in a single request.
    GetMultipleAccounts,
    /// **(RPS)** Fetches the balance of a single account.
    GetBalance,
    /// **(RPS)** Fetches the token balance of a single token account.
    GetTokenAccountBalance,

    /// A mixed mode that combines multiple benchmark modes with specified weights.
    Mixed(Vec<WeightedBenchMode>),
}

/// Represents a benchmark mode with an assigned weight for mixed-mode benchmarks.
#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct WeightedBenchMode {
    /// The benchmark mode to be executed.
    pub mode: BenchMode,
    /// The weight assigned to this mode, determining its frequency in the benchmark.
    pub weight: u16,
}

/// Defines the type of HTTP connection to use for the benchmark.
#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionType {
    /// Use HTTP/1.1 for all connections.
    Http1,
    /// Use HTTP/2 with a specified number of streams.
    Http2,
}

/// Defines the size of accounts to be used in the benchmark.
#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
#[repr(u32)]
pub enum AccountSize {
    BYTES128 = 128,
    BYTES512 = 512,
    BYTES2048 = 2048,
    BYTES8192 = 8192,
}

/// Defines the encoding for account data in RPC requests.
#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum AccountEncoding {
    Base58,
    Base64,
    #[serde(rename = "base64+zstd")]
    Base64Zstd,
}

impl AccountEncoding {
    /// Returns the string representation of the account encoding.
    pub fn as_str(&self) -> &str {
        match self {
            AccountEncoding::Base58 => "base58",
            AccountEncoding::Base64 => "base64",
            AccountEncoding::Base64Zstd => "base64+zstd",
        }
    }
}

/// A wrapper around `hyper::Uri` to provide custom methods for URL manipulation.
#[derive(Clone)]
pub struct Url(pub hyper::Uri);

impl Url {
    /// Returns the full address string, including the host and port.
    ///
    /// # Arguments
    ///
    /// * `ws` - A boolean indicating whether to use the WebSocket port (port + 1).
    pub fn address(&self, ws: bool) -> String {
        let host = self.host();
        let port = self.0.port_u16().map(|p| p + ws as u16).unwrap_or(80);

        format!("{}:{}", host, port)
    }

    /// Returns the host part of the URL.
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
