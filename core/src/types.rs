use std::error::Error;
use std::fmt;

use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize,
};

pub type DynError = Box<dyn Error + 'static>;
pub type BenchResult<T> = Result<T, DynError>;

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum BenchMode {
    SimpleByteSet,
    #[serde(rename_all = "kebab-case")]
    TriggerClones {
        clone_frequency_secs: u64,
        accounts_count: u8,
    },
    HighCuCost {
        iters: u32,
    },
    #[serde(rename_all = "kebab-case")]
    ReadWrite {
        accounts_count: u8,
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
