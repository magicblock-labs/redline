use core::{consts::RUNS_OUTPUT_PATH, types::BenchResult};
use std::{fs, path::PathBuf};

use args::AssistCommand;
use structopt::StructOpt;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> BenchResult<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let cmd = AssistCommand::from_args();
    match cmd {
        AssistCommand::Prepare { config } => prepare::prepare(config).await?,
        AssistCommand::Report { results } => report::report(results)?,
        AssistCommand::Cleanup { all } => cleanup::cleanup(all),
        AssistCommand::Compare {
            sensitivity,
            silent,
            this,
            that,
        } => compare::compare(this, that, sensitivity, silent)?,
    }
    Ok(())
}

fn latest_run_output_path(mut count: usize) -> PathBuf {
    let dir = fs::read_dir(RUNS_OUTPUT_PATH)
        .inspect_err(
            |error| tracing::error!(%error, "failed to read output directory for benchmark runs"),
        )
        .unwrap();
    let mut outputs: Vec<_> = dir
        .filter_map(|e| e.map(|e| e.path()).ok().filter(|p| p.is_file()))
        .collect();
    outputs.sort();
    loop {
        let path = outputs
            .pop()
            .expect("benchmark runs output directory didn't have enough entries");
        count -= 1;
        if count > 0 {
            continue;
        }
        break path;
    }
}

mod args;
mod cleanup;
mod compare;
mod prepare;
mod report;
