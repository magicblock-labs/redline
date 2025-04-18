use core::{stats::BenchStatistics, types::BenchResult};
use std::{fs, path::PathBuf};

use prettytable::{
    color::{Color, GREEN, RED},
    Attr,
};

use crate::latest_run_output_path;

pub fn compare(
    this: Option<PathBuf>,
    that: Option<PathBuf>,
    sensitivity: u8,
    _silent: bool,
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

    fn compare_and_print(this: &BenchStatistics, that: &BenchStatistics) {
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

        for (name, this_stats, that_stats) in metrics {
            let comparisons = vec![
                (
                    "Median",
                    this_stats.median,
                    that_stats.median,
                    this_stats.median < that_stats.median,
                ),
                (
                    "Q95",
                    this_stats.quantile95,
                    that_stats.quantile95,
                    this_stats.quantile95 < that_stats.quantile95,
                ),
                (
                    "Average",
                    this_stats.avg,
                    that_stats.avg,
                    this_stats.avg < that_stats.avg,
                ),
            ];

            for (stat_name, this_value, that_value, better) in comparisons {
                let diff = ((this_value - that_value) as f64 / this_value as f64) * 100.0;
                let mut cell = Cell::new(&format!("{diff:>+03.1}%"));
                if better {
                    cell.style(Attr::ForegroundColor(GREEN));
                    cell.style(Attr::Blink);
                } else {
                    cell.style(Attr::ForegroundColor(RED));
                };

                table.add_row(Row::new(vec![
                    Cell::new(&format!("{} {}", name, stat_name)),
                    Cell::new(&this_value.to_string()),
                    Cell::new(&that_value.to_string()),
                    cell,
                ]));
            }
        }

        table.printstd();
    }

    // Use the compare_and_print function after constructing `this` and `that`
    compare_and_print(&this, &that);
    Ok(())
}
