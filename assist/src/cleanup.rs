use core::consts::RUNS_OUTPUT_PATH;
use std::fs;

use crate::latest_run_output_path;

/// # Cleanup Benchmark Results
///
/// Deletes benchmark result files from the output directory. This function can either
/// remove all results or just the most recent one.
pub fn cleanup(all: bool) {
    // If the `all` flag is set, remove the entire `runs` directory and its contents.
    if all {
        if let Err(e) = fs::remove_dir_all(RUNS_OUTPUT_PATH) {
            tracing::error!("Failed to remove all runs: {}", e);
        }
        return;
    }

    // If the `runs` directory does not exist, there is nothing to clean up.
    if !fs::exists(RUNS_OUTPUT_PATH).unwrap_or_default() {
        return;
    }

    // Get the path to the latest benchmark result file.
    let last = latest_run_output_path(1);

    // Remove the latest result file.
    if let Err(e) = fs::remove_file(&last) {
        tracing::error!("Failed to remove latest run ({}): {}", last.display(), e);
    }
}
