use std::{path::PathBuf, time::Duration};

use reqwest::Url;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub mode: BenchMode,
    pub concurrency: Option<usize>,
    pub duration: BenchDuration,
    pub chain: Url,
    pub ephem: Url,
    pub ws: String,
    pub keypairs: Vec<PathBuf>,
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
    RawSpeed { space: u32 },
    CloneSpeed { accounts: u8, pubkeys: PathBuf },
}
