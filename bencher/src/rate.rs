use core::stats::ObservationsStats;
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const ONESEC: Duration = Duration::from_secs(1);
const ONEMS: Duration = Duration::from_millis(1);

/// # Rate Manager
///
/// Manages the rate of requests per second (RPS) or transactions per second (TPS)
/// for the benchmark, ensuring a steady and controlled load on the target validator.
pub struct RateManager {
    /// The number of requests sent in the current epoch.
    count: u32,
    /// The target rate of requests or transactions per second.
    rate: u32,
    /// The start time of the current epoch.
    epoch: Instant,
    /// A semaphore to control concurrency and prevent overwhelming the validator.
    permits: Arc<Semaphore>,
    /// A collection of observed rates per second.
    observations: VecDeque<u32>,
}

impl RateManager {
    /// # New Rate Manager
    ///
    /// Creates a new `RateManager` with the specified concurrency and rate.
    ///
    /// ### Arguments
    ///
    /// * `concurrency` - The maximum number of concurrent requests.
    /// * `rate` - The target rate of requests or transactions per second.
    pub fn new(concurrency: usize, rate: u32) -> Self {
        let permits = Arc::new(Semaphore::new(concurrency));
        Self {
            rate,
            permits,
            count: 0,
            epoch: Instant::now(),
            observations: VecDeque::new(),
        }
    }

    /// # Tick
    ///
    /// Processes a single request tick, managing the rate and concurrency.
    /// This method will block if the target rate is exceeded, ensuring a steady load.
    pub async fn tick(&mut self) -> OwnedSemaphorePermit {
        let elapsed = self.epoch.elapsed();
        self.count += 1;
        if elapsed >= ONESEC {
            self.observations.push_back(self.count);
            self.reset();
        }
        let remaining = (self.rate - self.count).max(1) as u64;
        let mut lag =
            Duration::from_millis(1000u64.saturating_sub(elapsed.as_millis() as u64) / remaining);
        if lag >= ONEMS {
            tokio::time::sleep(lag).await;
        } else if self.count >= self.rate {
            lag = Duration::from_millis(1000u64.saturating_sub(elapsed.as_millis() as u64));
            tokio::time::sleep(lag).await;
        }
        self.permits.clone().acquire_owned().await.unwrap()
    }

    /// # Get Statistics
    ///
    /// Returns the final statistics for the observed rates.
    pub fn stats(self) -> ObservationsStats {
        ObservationsStats::new(self.observations, true)
    }

    #[inline]
    pub fn reset(&mut self) {
        self.epoch = Instant::now();
        self.count = 0;
    }
}
