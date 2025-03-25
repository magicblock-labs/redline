use std::fmt;
use std::fmt::Formatter;
use std::{path::PathBuf, time::Duration};

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub modes: Vec<BenchMode>,
    pub concurrency: Vec<usize>,
    pub chain: String,
    pub ephem: String,
    pub duration: BenchDuration,
    pub ws: String,
    pub keypairs: Vec<PathBuf>,
}

pub struct ConfigPermutator {
    config: Config,
    mode: usize,
    concurrency: usize,
    preflight_check: usize,
    inter_txn_lag: usize,
}

pub struct ConfigPermuation {
    pub chain: String,
    pub ephem: String,
    pub duration: BenchDuration,
    pub ws: String,
    pub keypairs: Vec<PathBuf>,

    pub mode: BenchMode,
    pub concurrency: usize,
    pub preflight_check: bool,
    pub inter_txn_lag: bool,
}

impl ConfigPermutator {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            mode: 0,
            concurrency: 0,
            preflight_check: 0,
            inter_txn_lag: 0,
        }
    }

    pub fn permutate(&mut self) -> Option<ConfigPermuation> {
        (self.concurrency < self.config.concurrency.len()).then_some(())?;
        let permutation = ConfigPermuation {
            chain: self.config.chain.clone(),
            ephem: self.config.ephem.clone(),
            duration: self.config.duration.clone(),
            ws: self.config.ws.clone(),
            keypairs: self.config.keypairs.clone(),

            mode: self.config.modes[self.mode].clone(),
            concurrency: self.config.concurrency[self.concurrency],
            preflight_check: (self.preflight_check == 1),
            inter_txn_lag: (self.inter_txn_lag == 1),
        };
        self.inter_txn_lag += 1;
        if self.inter_txn_lag == 2 {
            self.inter_txn_lag = 0;
            self.preflight_check += 1;
        }
        if self.preflight_check == 2 {
            self.preflight_check = 0;
            self.mode += 1;
        }
        if self.mode == self.config.modes.len() {
            self.mode = 0;
            self.concurrency += 1;
        }
        Some(permutation)
    }
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum BenchDuration {
    Time(#[serde(deserialize_with = "duration::deserialize_duration")] Duration),
    Iters(u64),
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum BenchMode {
    RawSpeed { space: u32 },
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
            BenchMode::RawSpeed { space } => {
                write!(f, "raw speed - space: {}", space)
            }
            BenchMode::CloneSpeed { noise } => write!(f, "with cloning - noise factor: {}", noise),
        }
    }
}

impl fmt::Display for ConfigPermuation {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "MODE: {}", self.mode)?;
        writeln!(f, "DURATION: {}", self.duration)?;
        writeln!(f, "CONCURRENCY: {}", self.concurrency)?;
        write!(
            f,
            "PREFLIGHT CHECK: {}, INTER TXN LAG: {}",
            self.preflight_check, self.inter_txn_lag
        )
    }
}

impl ConfigPermuation {
    pub fn as_abr_str(&self) -> String {
        let mode = match self.mode {
            BenchMode::RawSpeed { space } => format!("RS-{space}"),
            BenchMode::CloneSpeed { noise } => format!("CS-{noise}"),
        };
        mode + &format!("/CC-{}", self.concurrency)
    }
}
