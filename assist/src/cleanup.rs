use core::{consts::RUNS_OUTPUT_PATH, types::BenchResult};
use std::fs;

use crate::latest_run_output_path;

pub fn cleanup(all: bool) -> BenchResult<()> {
    if all {
        fs::remove_dir_all(RUNS_OUTPUT_PATH)?;
    }
    let last = latest_run_output_path(1);
    fs::remove_file(last).map_err(Into::into)
}
