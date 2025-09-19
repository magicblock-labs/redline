use std::path::PathBuf;
use structopt::StructOpt;

/// # Redline Assist Command-Line Interface
///
/// Defines the command-line arguments for the `redline-assist` utility, which provides
/// helper functions for preparing, reporting on, and cleaning up benchmark runs.
#[derive(StructOpt, Debug)]
#[structopt(name = "redline-assist", rename_all = "kebab-case")]
pub enum AssistCommand {
    /// ## Prepare
    ///
    /// Prepares the environment for a benchmark run by creating and funding the necessary
    /// accounts, and ensuring that all PDAs are properly delegated.
    Prepare {
        /// The path to the benchmark configuration file.
        #[structopt(parse(from_os_str))]
        config: PathBuf,
    },
    /// ## Report
    ///
    /// Generates a comprehensive report from a benchmark results file.
    Report {
        /// The path to the JSON file containing the benchmark results.
        #[structopt(parse(from_os_str))]
        results: Option<PathBuf>,
    },
    /// ## Compare
    ///
    /// Compares the results of two different benchmark runs, highlighting any significant
    /// performance regressions or improvements.
    Compare {
        /// The path to the first benchmark results file.
        #[structopt(parse(from_os_str))]
        this: Option<PathBuf>,
        /// The path to the second benchmark results file.
        #[structopt(parse(from_os_str))]
        that: Option<PathBuf>,
        /// A flag to suppress the output if no regression is detected.
        #[structopt(long)]
        silent: bool,
        /// The sensitivity threshold for detecting performance regressions (0-100).
        #[structopt(long)]
        sensitivity: u8,
    },
    /// ## Cleanup
    ///
    /// Cleans up the benchmark runs directory, removing all results files.
    Cleanup {
        /// A flag to remove all benchmark results, not just the latest one.
        #[structopt(long, short)]
        all: bool,
    },
}
