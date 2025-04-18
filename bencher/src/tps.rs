use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::stats::ObservationsStats;

const ONESEC: Duration = Duration::from_secs(1);
const ONEMS: Duration = Duration::from_millis(1);

pub struct TpsManager {
    count: u32,
    tps: u32,
    epoch: Instant,
    permits: Arc<Semaphore>,
    observations: Vec<u32>,
}

impl TpsManager {
    pub fn new(concurrency: usize, tps: u32) -> Self {
        let permits = Arc::new(Semaphore::new(concurrency));
        Self {
            tps,
            permits,
            count: 0,
            epoch: Instant::now(),
            observations: Vec::new(),
        }
    }
    pub async fn tick(&mut self) -> OwnedSemaphorePermit {
        let elapsed = self.epoch.elapsed();
        if elapsed >= ONESEC {
            self.epoch = Instant::now();
            if self.count > 0 {
                self.observations.push(self.count);
            }
            self.count = 0;
        }
        self.count += 1;
        let remaining = (self.tps - self.count).max(1) as u64;
        let lag =
            Duration::from_millis(1000u64.saturating_sub(elapsed.as_millis() as u64) / remaining);
        if lag >= ONEMS {
            tokio::time::sleep(lag).await;
        }
        self.permits.clone().acquire_owned().await.unwrap()
    }

    pub fn stats(self) -> ObservationsStats {
        ObservationsStats::new(self.observations, true)
    }
}
