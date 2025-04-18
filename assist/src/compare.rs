use core::{stats::BenchStatistics, types::BenchResult};
use std::{fs, path::PathBuf};

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
    let diff = this
        .http_requests_latency
        .median
        .abs_diff(that.http_requests_latency.median);
    Ok(())
}
