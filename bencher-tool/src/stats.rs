use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Debug, Default)]
pub struct TxnRequestStats {
    pub id: u64,
    pub success: bool,
}

impl TxnRequestStats {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }
}

#[derive(Default)]
pub struct LatencyTracker {
    latency: Latency,
    pending: HashMap<u64, Instant>,
}

impl LatencyTracker {
    pub fn track(&mut self, key: u64) {
        let now = Instant::now();
        self.pending.insert(key, now);
    }

    pub fn confirm(&mut self, key: &u64) {
        let Some(timer) = self.pending.remove(key) else {
            return;
        };
        let duration = timer.elapsed();
        self.latency.observe(duration);
    }

    pub fn replace_id(&mut self, old: u64, new: u64) {
        let Some(timer) = self.pending.remove(&old) else {
            return;
        };
        self.pending.insert(new, timer);
    }
}

#[derive(Default)]
pub struct Latency {
    pub min: u64,
    pub max: u64,
    pub ma: f64,  // moving average
    pub sd: f64,  // standard deviation
    pub var: f64, // variance
    pub outliers: usize,
}

impl Latency {
    pub fn observe(&mut self, duration: Duration) {
        let micros = duration.as_micros() as u64;
        self.min = self.min.min(micros);
        self.max = self.max.max(micros);
        let micros = micros as f64;

        if self.ma == 0.0 {
            self.ma = micros;
            return;
        }
        // values which are two standard deviations away
        // from the mean are statistical outliers
        if self.sd != 0.0 && (micros - self.ma).abs() > 2.0 * self.sd {
            self.outliers += 1;
        }

        let ma = (self.ma + micros) / 2.0;
        self.var += (micros - ma) * (micros - self.ma);
        self.ma = ma;
        self.sd = self.var.sqrt();
    }
}

#[derive(Default)]
pub struct LatencyCollection {
    pub delivery: LatencyTracker,
    pub confirmation: LatencyTracker,
    pub update: LatencyTracker,
    pub failures: Latency,
    pub error_count: usize,
}

impl LatencyCollection {
    pub fn record_error(&mut self, id: &u64) {
        let Some(timer) = self.delivery.pending.remove(id) else {
            return;
        };
        self.failures.observe(timer.elapsed());
        self.error_count += 1;
    }
}
