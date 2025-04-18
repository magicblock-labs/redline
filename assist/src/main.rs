use core::types::BenchResult;

use args::AssistCommand;
use structopt::StructOpt;

#[tokio::main(flavor = "current_thread")]
async fn main() -> BenchResult<()> {
    let cmd = AssistCommand::from_args();
    match cmd {
        AssistCommand::Prepare { config } => prepare::prepare(config).await?,
        AssistCommand::Report { results } => report::report(results)?,
        AssistCommand::Compare { this, that } => compare::compare(this, that),
    }
    Ok(())
}

mod args;
mod compare;
mod prepare;
mod report;
