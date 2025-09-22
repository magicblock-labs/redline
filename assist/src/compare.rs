use core::{
    stats::{BenchStatistics, ObservationsStats},
    types::BenchResult,
};
use std::{fs, path::PathBuf};

use prettytable::{
    color::{GREEN, RED, YELLOW},
    format::Alignment,
    Attr, Cell, Row, Table,
};

use crate::latest_run_output_path;

/// # Compare Command
///
/// The main entry point for the `compare` command, responsible for orchestrating the
/// entire comparison process.
pub fn compare(
    this: Option<PathBuf>,
    that: Option<PathBuf>,
    sensitivity: u8,
    silent: bool,
) -> BenchResult<()> {
    let sensitivity = sensitivity as f64;
    let this_path = this.unwrap_or_else(|| latest_run_output_path(1));
    let that_path = that.unwrap_or_else(|| latest_run_output_path(2));

    let this: BenchStatistics = json::from_str(&fs::read_to_string(this_path)?)?;
    let mut that: BenchStatistics = json::from_str(&fs::read_to_string(that_path)?)?;

    let mut table = Table::new();
    let mut regression_detected = false;

    for (mode, this_stats) in this.request_stats {
        if let Some(that_stats) = that.request_stats.remove(&mode) {
            let metrics = vec![("Request Latency (μs)", this_stats, that_stats, 1.0)];
            add_metrics_to_table(
                &mut table,
                &mode,
                metrics,
                sensitivity,
                &mut regression_detected,
            );
        }
    }
    let metrics = vec![
        (
            "Sig. Confirm Latency (μs)",
            this.signature_confirmation_latency,
            that.signature_confirmation_latency,
            1.0,
        ),
        (
            "Acct. Update Latency (μs)",
            this.account_update_latency,
            that.account_update_latency,
            1.0,
        ),
        ("TPS", this.rps, that.rps, -1.0),
    ];
    add_metrics_to_table(
        &mut table,
        "",
        metrics,
        sensitivity,
        &mut regression_detected,
    );

    if !silent || regression_detected {
        table.printstd();
    }

    if regression_detected {
        Err("Performance regression has been detected".into())
    } else {
        Ok(())
    }
}

/// # Add Metrics to Table
///
/// A helper function to add a set of metrics to the comparison table.
fn add_metrics_to_table(
    table: &mut Table,
    mode: &str,
    metrics: Vec<(&str, ObservationsStats, ObservationsStats, f64)>,
    sensitivity: f64,
    regression_detected: &mut bool,
) {
    table.add_row(Row::new(vec![Cell::new(&format!("[{}]", mode))
        .with_style(Attr::Bold)
        .with_hspan(4)]));

    for (name, this_stats, that_stats, modifier) in metrics {
        let comparisons = vec![
            ("Median", this_stats.median, that_stats.median),
            ("Q95", this_stats.quantile95, that_stats.quantile95),
            ("Average", this_stats.avg, that_stats.avg),
        ];

        for (stat_name, this_value, that_value) in comparisons {
            let diff = ((this_value as f64 / that_value as f64) * 100.0 - 100.0) * modifier;
            let mut cell = Cell::new_align(&format!("{diff:>+03.1}%"), Alignment::RIGHT);
            if diff.abs() > sensitivity && diff.is_sign_positive() {
                cell.style(Attr::ForegroundColor(RED));
                *regression_detected = true;
            } else if diff.abs() > sensitivity && diff.is_sign_negative() {
                cell.style(Attr::ForegroundColor(GREEN));
            } else {
                cell.style(Attr::ForegroundColor(YELLOW));
            };

            table.add_row(Row::new(vec![
                Cell::new(&format!("  {} {}", name, stat_name)),
                Cell::new(&this_value.to_string()),
                Cell::new(&that_value.to_string()),
                cell,
            ]));
        }
        table.add_empty_row();
    }
}
