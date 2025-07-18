use core::{stats::BenchStatistics, types::BenchResult};
use std::{fs, path::PathBuf};

use prettytable::{
    color::{GREEN, RED, YELLOW},
    format::Alignment,
    Attr,
};
use prettytable::{Cell, Row, Table};

use crate::latest_run_output_path;

pub fn compare(
    this: Option<PathBuf>,
    that: Option<PathBuf>,
    sensitivity: u8,
    silent: bool,
) -> BenchResult<()> {
    let sensitivity = sensitivity as f64;
    let mut index = 1;
    let this = this.unwrap_or_else(|| {
        latest_run_output_path({
            let i = index;
            index += 1;
            i
        })
    });
    let that = that.unwrap_or_else(|| latest_run_output_path(index));

    let this: BenchStatistics = json::from_str(&fs::read_to_string(this)?)?;
    let that: BenchStatistics = json::from_str(&fs::read_to_string(that)?)?;

    let mut table = Table::new();

    let metrics = vec![
        (
            "sendTransaction Response Latency",
            this.send_txn_requests_latency(),
            that.send_txn_requests_latency(),
            1.0f64,
        ),
        (
            "Account Update",
            this.account_update_latency(),
            that.account_update_latency(),
            1.0,
        ),
        (
            "Signature Confirmation",
            this.signature_confirmation_latency(),
            that.signature_confirmation_latency(),
            1.0,
        ),
        (
            "TPS",
            this.transactions_per_second(),
            that.transactions_per_second(),
            -1.0,
        ),
        (
            "getX Request Response Latency",
            this.get_request_latency(),
            that.get_request_latency(),
            1.0,
        ),
        (
            "RPS",
            this.requests_per_second(),
            that.requests_per_second(),
            -1.0,
        ),
    ];

    let mut regression_detected = false;
    for (name, this_stats, that_stats, modifier) in metrics {
        let comparisons = vec![
            (
                "Median",
                this_stats.map(|s| s.median),
                that_stats.map(|s| s.median),
            ),
            (
                "Q95",
                this_stats.map(|s| s.quantile95),
                that_stats.map(|s| s.quantile95),
            ),
            (
                "Average",
                this_stats.map(|s| s.avg),
                that_stats.map(|s| s.avg),
            ),
        ];

        for (stat_name, this_value, that_value) in comparisons {
            let Some(this_value) = this_value else {
                continue;
            };
            let Some(that_value) = that_value else {
                continue;
            };

            let diff = ((this_value as f64 / that_value as f64) * 100.0 - 100.0) * modifier;
            let mut cell = Cell::new_align(&format!("{diff:>+03.1}%",), Alignment::RIGHT);
            if diff.abs() > sensitivity && diff.is_sign_positive() {
                cell.style(Attr::ForegroundColor(RED));
                regression_detected = true;
            } else if diff.abs() > sensitivity && diff.is_sign_negative() {
                cell.style(Attr::ForegroundColor(GREEN));
            } else {
                cell.style(Attr::ForegroundColor(YELLOW));
            };

            table.add_row(Row::new(vec![
                Cell::new(&format!("{} {}", name, stat_name)),
                Cell::new(&this_value.to_string()),
                Cell::new(&that_value.to_string()),
                cell,
            ]));
        }
        if name != "TPS" {
            table.add_empty_row();
        }
    }

    if !silent || regression_detected {
        table.printstd();
    }
    (!regression_detected)
        .then_some(())
        .ok_or("Performance regression has been detected".into())
}
