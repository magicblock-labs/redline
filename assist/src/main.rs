use core::{consts::RUNS_OUTPUT_PATH, types::BenchResult};
use std::{fs, path::PathBuf};

use args::AssistCommand;
use structopt::StructOpt;

#[tokio::main(flavor = "current_thread")]
async fn main() -> BenchResult<()> {
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
        .inspect_err(|err| eprintln!("failed to read output directory for benchmark runs: {err}"))
        .unwrap();
    let mut outputs: Vec<_> = dir
        .filter_map(|e| e.map(|e| e.path()).ok().filter(|p| p.is_file()))
        .collect();
    outputs.sort();
    println!("outputs: {outputs:?}");
    loop {
        let path = outputs
            .pop()
            .expect("benchmark runs output directory didn't have enough entries");
        count -= 1;
        if count > 0 {
            continue;
        }
        println!("removed: {:?}", path);
        break path;
    }
}

mod args;
mod cleanup;
mod compare;
mod prepare;
mod report;
