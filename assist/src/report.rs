use std::{fs, path::PathBuf};

use core::{
    stats::{BenchStatistics, ObservationsStats},
    types::BenchResult,
};
use json::JsonContainerTrait;
use prettytable::{Attr, Cell, Row, Table};

use crate::latest_run_output_path;

/// # Report Command
///
/// The main entry point for the `report` command, responsible for orchestrating the
/// entire report generation process.
pub fn report(path: Option<PathBuf>) -> BenchResult<()> {
    let path = path.unwrap_or_else(|| latest_run_output_path(1));
    let output = fs::read_to_string(path)?;
    let stats: BenchStatistics = json::from_str(&output)?;

    print_stats_pretty(&stats);
    Ok(())
}

/// # Print Statistics Pretty
///
/// A helper function to print the benchmark statistics in a human-readable format.
fn print_stats_pretty(stats: &BenchStatistics) {
    print_configuration_table(stats);
    print_results_table(stats);
}

/// # Print Configuration Table
///
/// A helper function to print the benchmark configuration in a pretty table.
fn print_configuration_table(stats: &BenchStatistics) {
    let mut table = Table::new();
    table.add_row(Row::new(vec![
        Cell::new("Configuration").with_style(Attr::Bold),
        Cell::new("Values").with_style(Attr::Bold),
    ]));

    if let Some(obj) = stats.configuration.as_object() {
        for (key, value) in obj {
            let mut inner_content = String::new();
            if let Some(inner_obj) = value.as_object() {
                for (inner_key, inner_value) in inner_obj {
                    if let Some(value) = inner_value.as_object() {
                        inner_content.push_str(&format!(
                            "{}: {}\n",
                            inner_key,
                            json::to_string_pretty(value).unwrap()
                        ));
                    } else {
                        inner_content.push_str(&format!("{}: {}\n", inner_key, inner_value));
                    }
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
}

/// # Print Results Table
///
/// A helper function to print the benchmark results in a pretty table.
fn print_results_table(stats: &BenchStatistics) {
    let mut table = Table::new();
    table.add_row(Row::new(vec![
        Cell::new("Metric").with_style(Attr::Bold),
        Cell::new("Observations").with_style(Attr::Bold),
        Cell::new("Median").with_style(Attr::Bold),
        Cell::new("Min").with_style(Attr::Bold),
        Cell::new("Max").with_style(Attr::Bold),
        Cell::new("Avg").with_style(Attr::Bold),
        Cell::new("95th Perc").with_style(Attr::Bold),
        Cell::new("Stddev").with_style(Attr::Bold),
    ]));

    for (mode, stats) in &stats.request_stats {
        add_request_stats_to_table(&mut table, mode, *stats);
    }

    add_stats_row!(
        &mut table,
        "Sig. Confirmation",
        Some(stats.signature_confirmation_latency)
    );
    add_stats_row!(
        &mut table,
        "Acc. Update",
        Some(stats.account_update_latency)
    );
    add_stats_row!(&mut table, "Total RPS", Some(stats.rps));

    table.printstd();
}

/// # Add RPC Request Stats to Table
///
/// A helper function to add RPC request statistics to the results table.
fn add_request_stats_to_table(table: &mut Table, mode: &str, stats: ObservationsStats) {
    table.add_row(Row::new(vec![Cell::new(&format!("[{}]", mode))
        .with_style(Attr::Bold)
        .with_hspan(8)]));
    add_stats_row!(table, "  Request Latency (μs)", Some(stats));
}

/// # Add Stats Row
///
/// A helper macro to add a row of statistics to the results table.
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

use add_stats_row;
