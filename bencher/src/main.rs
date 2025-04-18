use core::{config::Config, stats::BenchStatistics, types::BenchResult};
use std::{fs::File, path::PathBuf, rc::Rc, thread::JoinHandle, time::SystemTime};

use json::writer::BufferedWriter;
use keypair::Keypair;
use runner::BenchRunner;
use signer::EncodableKey;
use tokio::{runtime, sync::broadcast, task::LocalSet};

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
                .block_on(&rt, BenchRunner::new(kp, config))
                .expect("failed to create bencher");
            let task = local.run_until(bencher.run());
            let results = rt.block_on(task);
            rt.block_on(local);
            results.stats()
        });
        handles.push(handle);
    }

    let stats = handles
        .into_iter()
        .map(JoinHandle::join)
        .collect::<std::thread::Result<_>>()
        .expect("failed to join tokio runtime for bencher");
    let stats = BenchStatistics::merge(stats);
    let output = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    let outdir = PathBuf::from("runs");
    let _ = std::fs::create_dir(&outdir);
    let output = outdir.join(format!("redline-{:0>12}.json", output.as_secs()));
    let writer = File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&output)
        .map(BufferedWriter::new)?;
    json::to_writer(writer, &stats)?;
    println!(
        "The results of the benchmark are written to {}",
        output.display()
    );
    Ok(())
}

struct ShutDownSender(broadcast::Sender<()>);
type ShutDownListener = broadcast::Receiver<()>;
type ShutDown = Rc<ShutDownSender>;

impl Drop for ShutDownSender {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

impl ShutDownSender {
    fn init() -> ShutDown {
        let (tx, _) = broadcast::channel(1);
        Rc::new(Self(tx))
    }

    fn listener(&self) -> ShutDownListener {
        self.0.subscribe()
    }
}

mod blockhash;
mod confirmation;
mod extractor;
mod http;
mod payload;
mod runner;
mod tps;
mod transaction;
mod websocket;
