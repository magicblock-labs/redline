use std::{env, fs::read_to_string};

use config::Config;
use runner::BenchRunner;
use tokio::{runtime::Builder, task::LocalSet};

fn main() {
    let rt = Builder::new_current_thread()
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let path = env::args().nth(1).expect("usage: bencher config.toml");
    let config = read_to_string(path).expect("config path doesn't exist");
    let config: Config = toml::from_str(&config).expect("invalid config file");
    let local = LocalSet::new();
    let bencher = rt.block_on(local.run_until(BenchRunner::new(config)));
    let f = local.run_until(bencher.run());
    rt.block_on(f);
}

mod accounts_reader;
mod config;
mod http;
mod pda;
mod runner;
mod stats;
mod system;
mod ws;
