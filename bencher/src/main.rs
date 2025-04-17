use std::{ops::Deref, rc::Rc};

use core::{BenchResult, Config};
use keypair::Keypair;
use runner::BenchRunner;
use signer::EncodableKey;
use tokio::{runtime, sync::Notify, task::LocalSet};

fn main() -> BenchResult<()> {
    let config = Config::from_args()?;
    let keypairs: Vec<_> = (1..=config.benchmark.parallelism)
        .map(|n| Keypair::read_from_file(format!("keypairs/{n:>03}.json")))
        .collect::<BenchResult<_>>()?;
    let mut handles = Vec::new();
    for kp in keypairs {
        let config = config.clone();
        let handle = std::thread::spawn(move || {
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
        handles.push(handle);
    }
    for h in handles {
        h.join().expect("failed to join on tokio runtime thread")
    }
    Ok(())
}

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
mod confirmation;
mod extractor;
mod http;
mod payload;
mod runner;
mod stats;
mod tps;
mod transaction;
mod websocket;
