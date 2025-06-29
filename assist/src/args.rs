use std::path::PathBuf;

use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "redline-assist", rename_all = "kebab-case")]
pub enum AssistCommand {
    /// Prepare accounts for given benchmark configuration, make sure that PDAs exist and delegated, and payers are funded
    Prepare {
        /// Path containing the configuration for benchmark
        #[structopt(parse(from_os_str))]
        config: PathBuf,
    },
    /// Generate comprehensive report for benchmark results
    Report {
        /// Benchmark results, JSON file
        #[structopt(parse(from_os_str))]
        results: Option<PathBuf>,
    },
    /// Compare results of two different benchmark runs, inputs are JSON file results
    Compare {
        /// Benchmark results, JSON file #1
        #[structopt(parse(from_os_str))]
        this: Option<PathBuf>,
        /// Benchmark results, JSON file #2
        #[structopt(parse(from_os_str))]
        that: Option<PathBuf>,
        #[structopt(long)]
        silent: bool,
        #[structopt(long)]
        sensitivity: u8,
    },
    /// Cleanup benchmark runs results (JSON files)
    Cleanup {
        #[structopt(long, short)]
        all: bool,
    },
}
