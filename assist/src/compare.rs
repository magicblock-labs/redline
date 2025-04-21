use core::{stats::BenchStatistics, types::BenchResult};
use std::{fs, path::PathBuf};

use prettytable::{
    color::{GREEN, RED, YELLOW},
    format::Alignment,
    Attr,
};

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
    use prettytable::{Cell, Row, Table};

    let mut table = Table::new();

    let metrics = vec![
        (
            "Request Latency",
            &this.http_requests_latency,
            &that.http_requests_latency,
        ),
        (
            "Account Update",
            &this.account_update_latency,
            &that.account_update_latency,
        ),
        (
            "Signature Confirmation",
            &this.signature_confirmation_latency,
            &that.signature_confirmation_latency,
        ),
        (
            "TPS",
            &this.transactions_per_second,
            &that.transactions_per_second,
        ),
    ];

    let mut regression_detected = false;
    for (name, this_stats, that_stats) in metrics {
        let comparisons = vec![
            ("Median", this_stats.median, that_stats.median, 1.0),
            ("Q95", this_stats.quantile95, that_stats.quantile95, 1.0),
            ("Average", this_stats.avg, that_stats.avg, -1.0),
        ];

        for (stat_name, this_value, that_value, modifier) in comparisons {
            let diff = (this_value - that_value) as f64 / this_value as f64 * 100.0 * modifier;
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
    }

    if !silent || regression_detected {
        table.printstd();
    }
    (!regression_detected)
        .then_some(())
        .ok_or("Performance regression has been detected".into())
}
