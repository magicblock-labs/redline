use std::{
    io::{stdout, Write},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

/// # Progress Bar
///
/// A simple, animated progress bar for tracking the progress of the benchmark.
/// It is designed to run in a separate thread to avoid blocking the main benchmark thread.
pub struct ProgressBar {
    /// The total number of iterations in the benchmark.
    total: u64,
    /// An atomic counter for tracking the number of completed requests.
    progress: Arc<AtomicU64>,
}

impl ProgressBar {
    /// # New Progress Bar
    ///
    /// Creates a new `ProgressBar` instance.
    pub fn new(total: u64, progress: Arc<AtomicU64>) -> Self {
        Self { total, progress }
    }

    /// # Start Progress Bar
    ///
    /// Starts the progress bar in a separate thread.
    pub fn start(self) {
        // Unicode spinner characters
        let spinner = ["|", "/", "-", "\\"];
        let mut spin_idx = 0;
        println!();
        // Hide the cursor for a cleaner look
        print!("\x1B[?25l");

        loop {
            let current = self.progress.load(Ordering::Relaxed);
            let shutdown = crate::SHUTDOWN.load(Ordering::Relaxed);

            // Once done, clear the line, print a final message, and show the cursor
            if current >= self.total || shutdown {
                print!("\r\x1B[K"); // Clear the current line
                if shutdown {
                    tracing::info!(
                        "⚠️  Benchmark Interrupted: {}/{} requests sent.",
                        current,
                        self.total
                    );
                } else {
                    tracing::info!(
                        "✅ Benchmark Complete: {}/{} requests sent.",
                        current,
                        self.total
                    );
                }
                print!("\x1B[?25h"); // Show the cursor again
                stdout().flush().unwrap();
                break;
            }

            let percent = (current as f64 / self.total as f64) * 100.0;
            let bar_len = 80;
            let filled_len = (percent / 100.0 * bar_len as f64) as usize;

            // Create a bar with Unicode block characters
            let bar = "█".repeat(filled_len);
            let empty = "-".repeat(bar_len - filled_len);
            let spinner_char = spinner[spin_idx % spinner.len()];

            // Use carriage return `\r` to overwrite the line on each update
            print!(
                "\r {} Running benchmark [{}{}] {:.2}% ({}/{})",
                spinner_char, bar, empty, percent, current, self.total
            );

            // We need to flush stdout to ensure the progress bar updates immediately
            stdout().flush().unwrap();
            spin_idx += 1;

            // Refresh the bar every 200ms for a smooth animation
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
