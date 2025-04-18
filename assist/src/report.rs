use std::{fs, path::PathBuf};

use core::{consts::RUNS_OUTPUT_PATH, stats::BenchStatistics, types::BenchResult};

use json::JsonContainerTrait;
use prettytable::{Cell, Row, Table};

pub fn report(path: Option<PathBuf>) -> BenchResult<()> {
    let path = path.unwrap_or_else(latest_output_path);
    let output = fs::read_to_string(path)?;
    let stats: BenchStatistics = json::from_str(&output)?;

    print_stats_pretty(&stats);
    Ok(())
}

fn latest_output_path() -> PathBuf {
    let dir = fs::read_dir(RUNS_OUTPUT_PATH)
        .inspect_err(|err| eprintln!("failed to read output directory for benchmark runs: {err}"))
        .unwrap();
    let mut outputs: Vec<_> = dir.filter_map(|e| e.map(|e| e.path()).ok()).collect();
    outputs.sort();
    outputs
        .pop()
        .expect("benchmark runs output directory should have at least one entry")
}

macro_rules! add_stats_row {
    ($table:expr, $label:expr, $stats:expr) => {
        $table.add_row(Row::new(vec![
            Cell::new($label),
            Cell::new(&$stats.count.to_string()),
            Cell::new(&$stats.median.to_string()),
            Cell::new(&$stats.min.to_string()),
            Cell::new(&$stats.max.to_string()),
            Cell::new(&$stats.avg.to_string()),
            Cell::new(&$stats.quantile95.to_string()),
            Cell::new(&$stats.stddev.to_string()),
        ]));
    };
}

fn print_stats_pretty(stats: &BenchStatistics) {
    let mut table = Table::new();

    table.add_row(Row::new(vec![
        Cell::new("Configuration Key"),
        Cell::new("Value"),
    ]));

    if let Some(obj) = stats.configuration.as_object() {
        for (key, value) in obj {
            let mut inner_content = String::new();
            if let Some(inner_obj) = value.as_object() {
                for (inner_key, inner_value) in inner_obj {
                    inner_content.push_str(&format!("{}: {}\n", inner_key, inner_value));
                }
            }
            table.add_row(Row::new(vec![
                Cell::new(key),
                Cell::new(inner_content.trim_end()),
            ]));
        }
    }

    table.printstd();

    let mut table = Table::new();

    table.add_row(Row::new(vec![
        Cell::new("Metric"),
        Cell::new("Observations"),
        Cell::new("Median"),
        Cell::new("Min"),
        Cell::new("Max"),
        Cell::new("Avg"),
        Cell::new("95th Percentile"),
        Cell::new("Stddev"),
    ]));

    add_stats_row!(
        table,
        "HTTP Requests Latency (μs)",
        stats.http_requests_latency
    );
    add_stats_row!(
        table,
        "Account Update Latency (μs)",
        stats.account_update_latency
    );
    add_stats_row!(
        table,
        "Signature Confirmation Latency (μs)",
        stats.signature_confirmation_latency
    );
    add_stats_row!(
        table,
        "Transactions Per Second (TPS)",
        stats.transactions_per_second
    );

    table.printstd();
}
