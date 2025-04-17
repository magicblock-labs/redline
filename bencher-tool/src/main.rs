use std::{env, fs::read_to_string};

use config::{Config, ConfigPermutator};
use prettytable::{Cell, Table};
use runner::BenchRunner;
use tokio::{runtime::Builder, task::LocalSet};

fn main() {
    let rt = Builder::new_current_thread().enable_all().build().unwrap();
    let path = env::args().nth(1).expect("usage: bencher config.toml");
    let config = read_to_string(path).expect("config path doesn't exist");
    let config: Config = toml::from_str(&config).expect("invalid config file");
    let mut permutator = ConfigPermutator::new(config);
    let mut local = LocalSet::new();
    let mut row = prettytable::row!["mode/concurrency", "ff", "ft", "tf", "tt",];
    let mut table = Table::new();
    let mut last_mode = String::new();
    if let Some(config) = permutator.permutate() {
        let mode = config.as_abr_str();
        if last_mode != mode {
            table.add_row(std::mem::take(&mut row));
            row.add_cell(Cell::new(&mode));
            last_mode = mode;
        }
        let f = async move {
            let bencher = BenchRunner::new(config).await;
            bencher.run().await
        };
        let f = local.spawn_local(f);
        rt.block_on(&mut local);
        let (latency, tps) = rt.block_on(f).unwrap();
        row.add_cell(Cell::new(&format!(
            "{}/{}",
            latency.borrow().as_abr_summary(),
            tps as u64
        )));
    }
    table.add_row(row);
    table.printstd();
}

mod client;
mod config;
mod pda;
mod runner;
mod stats;
