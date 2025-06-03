use std::{fs, path::PathBuf};

use core::{stats::BenchStatistics, types::BenchResult};

use json::JsonContainerTrait;
use prettytable::{Cell, Row, Table};

use crate::latest_run_output_path;

pub fn report(path: Option<PathBuf>) -> BenchResult<()> {
    let path = path.unwrap_or_else(|| latest_run_output_path(1));
    let output = fs::read_to_string(path)?;
    let stats: BenchStatistics = json::from_str(&output)?;

    print_stats_pretty(&stats);
    Ok(())
}

macro_rules! add_stats_row {
    ($table:expr, $label:expr, $stats:expr) => {
        let row = if let Some(stats) = $stats {
            vec![
                Cell::new($label),
                Cell::new(&stats.count.to_string()),
                Cell::new(&stats.median.to_string()),
                Cell::new(&stats.min.to_string()),
                Cell::new(&stats.max.to_string()),
                Cell::new(&stats.avg.to_string()),
                Cell::new(&stats.quantile95.to_string()),
                Cell::new(&stats.stddev.to_string()),
            ]
        } else {
            vec![
                Cell::new($label),
                Cell::new("---"),
                Cell::new("---"),
                Cell::new("---"),
                Cell::new("---"),
                Cell::new("---"),
                Cell::new("---"),
                Cell::new("---"),
            ]
        };
        $table.add_row(Row::new(row));
    };
}

fn print_stats_pretty(stats: &BenchStatistics) {
    let mut table = Table::new();

    table.add_row(Row::new(vec![
        Cell::new("Configuration"),
        Cell::new("Values"),
    ]));

    if let Some(obj) = stats.configuration().as_object() {
        for (key, value) in obj {
            let mut inner_content = String::new();
            if let Some(inner_obj) = value.as_object() {
                for (inner_key, inner_value) in inner_obj {
                    inner_content.push_str(&format!("{}: {}\n", inner_key, inner_value));
                }
            } else {
                inner_content.push_str(&format!("{}: {}\n", key, value));
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
        Cell::new("95th Perc"),
        Cell::new("Stddev"),
    ]));

    add_stats_row!(
        table,
        "sendTransaction Response (μs)",
        stats.send_txn_requests_latency()
    );
    add_stats_row!(table, "Account Update (μs)", stats.account_update_latency());
    add_stats_row!(
        table,
        "Signature Confirmation (μs)",
        stats.signature_confirmation_latency()
    );
    add_stats_row!(
        table,
        "Transactions Per Second (TPS)",
        stats.transactions_per_second()
    );
    add_stats_row!(
        table,
        "Requests Per Second (TPS)",
        stats.requests_per_second()
    );
    add_stats_row!(
        table,
        "getX Request Response (TPS)",
        stats.get_request_latency()
    );

    table.printstd();
}
