use core::{
    config::Config,
    stats::{BenchStatistics, RpsBenchStatistics, TpsBenchStatistics},
    types::BenchResult,
};
use std::{fs::File, path::PathBuf, rc::Rc, thread::JoinHandle, time::SystemTime};

use get_requests::RpsBenchRunner;
use json::writer::BufferedWriter;
use keypair::Keypair;
use send_txn::TpsBenchRunner;
use signer::{EncodableKey, Signer};
use tokio::{runtime, sync::broadcast, task::LocalSet};

fn main() -> BenchResult<()> {
    let config = Config::from_args()?;
    let keypairs: Vec<_> = (1..=config.parallelism)
        .map(|n| Keypair::read_from_file(format!("keypairs/{n:>03}.json")))
        .collect::<BenchResult<_>>()?;
    let mut tps_handles = Vec::new();
    let mut rps_handles = Vec::new();
    for kp in keypairs {
        let bench_rps = config.rps_benchmark.enabled;
        let bench_tps = config.tps_benchmark.enabled;

        let base = kp.pubkey();

        if bench_rps {
            let cfg = config.clone();
            let handle = std::thread::spawn(move || {
                let rt = runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let local = LocalSet::new();
                let bencher = local
                    .block_on(&rt, RpsBenchRunner::new(base, &cfg))
                    .expect("failed to create bencher");
                let task = local.run_until(bencher.run());
                let results = rt.block_on(task);
                rt.block_on(local);
                results.stats()
            });
            rps_handles.push(handle);
        }
        if bench_tps {
            let cfg = config.clone();
            let handle = std::thread::spawn(move || {
                let rt = runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let local = LocalSet::new();
                let bencher = local
                    .block_on(&rt, TpsBenchRunner::new(kp, cfg))
                    .expect("failed to create bencher");
                let task = local.run_until(bencher.run());
                let results = rt.block_on(task);
                rt.block_on(local);
                results.stats()
            });
            tps_handles.push(handle);
        }
    }

    let tps_stats = tps_handles
        .into_iter()
        .map(JoinHandle::join)
        .collect::<std::thread::Result<Vec<TpsBenchStatistics>>>()
        .expect("failed to join tokio runtime for tps bencher");
    let rps_stats = rps_handles
        .into_iter()
        .map(JoinHandle::join)
        .collect::<std::thread::Result<Vec<RpsBenchStatistics>>>()
        .expect("failed to join tokio runtime for rps bencher");

    let stats = (!tps_stats.is_empty()).then(|| TpsBenchStatistics::merge(tps_stats));
    let rps_stats = (!rps_stats.is_empty()).then(|| RpsBenchStatistics::merge(rps_stats));
    let stats = if let Some(s) = stats {
        s.merge_rps_to_tps(rps_stats)
    } else if let Some(s) = rps_stats {
        BenchStatistics::Rps(s)
    } else {
        panic!("both bench methods cannot be disabled!");
    };

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
mod get_requests;
mod http;
mod payload;
mod rps;
mod send_txn;
mod transaction;
mod websocket;
