use std::{env, fs::read_to_string};

use config::Config;
use runner::BenchRunner;
use tokio::{runtime::Builder, task::LocalSet};

fn main() {
    let rt = Builder::new_current_thread().enable_all().build().unwrap();
    let f = async move {
        let path = env::args().nth(1).expect("usage: bencher config.toml");
        let config = read_to_string(path).expect("config path doesn't exist");
        let config: Config = toml::from_str(&config).expect("invalid config file");
        println!("=================================================");
        println!("{config}");
        let bencher = BenchRunner::new(config).await;
        bencher.run().await
    };
    let local = LocalSet::new();
    let f = local.spawn_local(f);
    rt.block_on(local);
    let (latency, tps) = rt.block_on(f).unwrap();
    println!("{}\nTPS:{tps}", latency.borrow(),);
    println!("=================================================\n\n");
}

mod client;
mod config;
mod pda;
mod runner;
mod stats;
