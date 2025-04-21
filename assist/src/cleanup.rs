use core::consts::RUNS_OUTPUT_PATH;
use std::fs;

use crate::latest_run_output_path;

pub fn cleanup(all: bool) {
    println!("cleanup is invoked");
    if all {
        let _ = fs::remove_dir_all(RUNS_OUTPUT_PATH);
        return;
    }
    if !fs::exists(RUNS_OUTPUT_PATH).unwrap_or_default() {
        return;
    }
    let last = latest_run_output_path(1);
    let _ = fs::remove_file(last);
}
