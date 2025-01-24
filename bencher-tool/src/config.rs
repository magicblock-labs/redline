use std::fmt;
use std::fmt::Formatter;
use std::{path::PathBuf, time::Duration};

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub mode: BenchMode,
    pub concurrency: Option<usize>,
    pub latency: u64,
    pub duration: BenchDuration,
    pub chain: String,
    pub ephem: String,
    pub ws: String,
    pub keypairs: Vec<PathBuf>,
    pub subscriptions: bool,
    pub confirmations: bool,
    pub sigverify: bool,
    pub validator_mode: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BenchDuration {
    Time(#[serde(deserialize_with = "duration::deserialize_duration")] Duration),
    Iters(u64),
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BenchMode {
    RawSpeed { space: u32, local: bool },
    CloneSpeed { noise: u8 },
}

impl BenchMode {
    pub fn space(&self) -> u32 {
        match self {
            Self::RawSpeed { space, .. } => *space,
            Self::CloneSpeed { .. } => size_of::<u64>() as u32,
        }
    }
}

impl BenchDuration {
    pub fn iters(&self) -> usize {
        match self {
            Self::Time(_) => 65536,
            Self::Iters(i) => *i as usize,
        }
    }
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "mode: {}\nconcurrency: {}, inter transaction lag (ms): {}\nduration: {}\nsubscription to updates: {}, signature confirmations: {}\nsigverify: {}, validator mode: {}",
            self.mode,
            self.concurrency.unwrap_or(usize::MAX),
            self.latency,
            self.duration,
            self.subscriptions,
            self.confirmations,
            self.sigverify,
            self.validator_mode
        )
    }
}

impl fmt::Display for BenchDuration {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BenchDuration::Time(duration) => {
                write!(f, "bench duration: {:.1}s", duration.as_secs_f64())
            }
            BenchDuration::Iters(iters) => write!(f, "tx count: {}", iters),
        }
    }
}

impl fmt::Display for BenchMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BenchMode::RawSpeed { space, local } => {
                write!(f, "raw speed - space: {}, local: {}", space, local)
            }
            BenchMode::CloneSpeed { noise } => write!(f, "with cloning - noise factor: {}", noise),
        }
    }
}
