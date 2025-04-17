use std::{error::Error, ops::Deref, rc::Rc};

use config::Config;
use keypair::Keypair;
use runner::BenchRunner;
use signer::EncodableKey;
use tokio::{runtime, sync::Notify, task::LocalSet};

fn main() -> BenchResult<()> {
    let config = std::env::args()
        .nth(1)
        .ok_or("usage: redline config.toml")?;
    let config = std::fs::read_to_string(config)?;
    let mut config: Config = toml::from_str(&config)?;
    let keypairs: Vec<_> = std::mem::take(&mut config.benchmark.keypairs)
        .into_iter()
        .map(Keypair::read_from_file)
        .collect::<BenchResult<_>>()?;
    let mut handles = Vec::new();
    for kp in keypairs {
        let config = config.clone();
        let h = std::thread::spawn(move || {
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let local = LocalSet::new();
            let bencher = local
                .block_on(&rt, BenchRunner::new(kp, &config))
                .expect("failed to create bencher");
            let task = local.run_until(bencher.run());
            let results = rt.block_on(task);
            rt.block_on(local);
            results.stats();
        });
        handles.push(h);
    }
    for h in handles {
        h.join().expect("failed to join on tokio runtime thread")
    }
    Ok(())
}

type DynError = Box<dyn Error + 'static>;
type BenchResult<T> = Result<T, DynError>;
#[derive(Default)]
struct ShutDownInner(Notify);
type ShutDown = Rc<ShutDownInner>;

impl Drop for ShutDownInner {
    fn drop(&mut self) {
        self.0.notify_waiters();
    }
}

impl Deref for ShutDownInner {
    type Target = Notify;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

mod blockhash;
mod config;
mod confirmation;
mod extractor;
mod http;
mod payload;
mod runner;
mod stats;
mod tps;
mod transaction;
mod websocket;
