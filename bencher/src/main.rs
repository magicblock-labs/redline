use core::{config::Config, stats::BenchStatistics, types::BenchResult};
use std::{
    fs::{self, File},
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::SystemTime,
};

use json::writer::BufferedWriter;
use keypair::Keypair;
use runner::BenchRunner;
use signal_hook::{consts::*, low_level};
use signer::EncodableKey;
use tokio::{runtime, sync::broadcast, task::LocalSet};
use tracing_subscriber::EnvFilter;

use crate::progress::ProgressBar;

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// # Main Entry Point
///
/// The main entry point for the Redline bencher, responsible for initializing the configuration,
/// creating and managing parallel benchmark runners, and aggregating the results.
fn main() -> BenchResult<()> {
    // Initialize the logger
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // Load the configuration from command-line arguments
    let config = Config::from_args()?;
    let keypairs: Vec<_> = (1..=config.payers * config.parallelism)
        .map(|n| Keypair::read_from_file(config.keypairs.join(format!("{n}.json"))))
        .collect::<BenchResult<_>>()?;

    // Set up signal handlers for graceful shutdown
    setup_signal_handlers()?;

    let mut handles = Vec::new();
    // Create a shared atomic counter for tracking progress.
    let progress = Arc::new(AtomicU64::new(0));

    // Create and start the progress bar.
    let progress_bar = ProgressBar::new(
        config.benchmark.iterations * config.parallelism as u64,
        progress.clone(),
    );
    let bar = thread::spawn(move || progress_bar.start());

    // Spawn a new thread for each keypair, up to the specified parallelism
    for kp in keypairs.chunks(config.payers as usize) {
        let signers = kp.iter().map(|k| k.insecure_clone()).collect();
        let cfg = config.clone();
        let progress = progress.clone();
        let handle = thread::spawn(move || {
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let local = LocalSet::new();
            let bencher = local
                .block_on(&rt, BenchRunner::new(signers, cfg, progress))
                .expect("failed to create bencher");
            let task = local.run_until(bencher.run());
            let results = rt.block_on(task);
            rt.block_on(local);
            results.stats()
        });
        handles.push(handle);
    }
    let _ = bar.join();

    // Collect and merge the statistics from all threads
    let stats: Vec<BenchStatistics> = handles
        .into_iter()
        .map(JoinHandle::join)
        .collect::<thread::Result<Vec<BenchStatistics>>>()
        .expect("failed to join benchmark thread");

    let stats = BenchStatistics::merge(stats);

    // Write the aggregated results to a JSON file
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();
    let outdir = PathBuf::from("runs");
    let _ = fs::create_dir(&outdir);
    let output = outdir.join(format!("redline-{ts:0>12}.json"));
    let writer = File::create(&output).map(BufferedWriter::new)?;
    json::to_writer(writer, &stats)?;

    if SHUTDOWN.load(Ordering::Relaxed) {
        tracing::warn!(
            "Interrupted - partial results saved to {}",
            output.display()
        );
    } else {
        tracing::info!("Results written to {}", output.display());
    }

    Ok(())
}

/// Sets up signal handlers for graceful shutdown on SIGTERM/SIGINT
fn setup_signal_handlers() -> BenchResult<()> {
    unsafe {
        low_level::register(SIGTERM, || SHUTDOWN.store(true, Ordering::Relaxed))?;
        low_level::register(SIGINT, || SHUTDOWN.store(true, Ordering::Relaxed))?;
    }
    Ok(())
}

/// A sender for the shutdown signal.
struct ShutDownSender(broadcast::Sender<()>);
/// A listener for the shutdown signal.
type ShutDownListener = broadcast::Receiver<()>;
/// A reference-counted `ShutDownSender`.
type ShutDown = Rc<ShutDownSender>;

impl Drop for ShutDownSender {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

impl ShutDownSender {
    /// Initializes a new `ShutDown` instance.
    fn init() -> ShutDown {
        let (tx, _) = broadcast::channel(1);
        Rc::new(Self(tx))
    }

    /// Creates a new `ShutDownListener`.
    fn listener(&self) -> ShutDownListener {
        self.0.subscribe()
    }
}

mod blockhash;
mod confirmation;
mod extractor;
mod http;
mod payload;
mod progress;
mod rate;
mod requests;
mod runner;
mod transaction;
mod transfer;
mod websocket;
