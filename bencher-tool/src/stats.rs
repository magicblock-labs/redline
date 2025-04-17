use std::fmt;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub struct LatencyTracker {
    latency: Latency,
    pending: HashMap<u64, Instant>,
}

impl LatencyTracker {
    pub fn new(capacity: usize) -> Self {
        Self {
            latency: Latency::new(capacity),
            pending: Default::default(),
        }
    }

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
}

pub struct Latency {
    pub observations: Vec<f64>,
}

impl Latency {
    pub fn new(capacity: usize) -> Self {
        Self {
            observations: Vec::with_capacity(capacity),
        }
    }

    pub fn observe(&mut self, duration: Duration) {
        let micros = duration.as_secs_f64();
        self.observations.push(micros);
    }

    pub fn compute(&self) -> LatencyStats {
        let avg = self.observations.iter().sum::<f64>() / self.observations.len() as f64;
        let variance = self
            .observations
            .iter()
            .map(|value| {
                let diff = value - avg;
                diff * diff
            })
            .sum::<f64>()
            / self.observations.len() as f64;

        let sd = variance.sqrt();
        let outliers = self
            .observations
            .iter()
            .filter(|o| (*o - avg).abs() > 2.0 * sd)
            .count() as u32;
        LatencyStats { outliers, avg, sd }
    }
}

pub struct LatencyStats {
    avg: f64,
    sd: f64,
    outliers: u32,
}

pub struct LatencyCollection {
    pub delivery: LatencyTracker,
    pub update: LatencyTracker,
    pub confirmation: LatencyTracker,
    pub failures: Latency,
    pub error_count: usize,
}

impl LatencyCollection {
    pub fn new(capacity: usize) -> Self {
        Self {
            delivery: LatencyTracker::new(capacity),
            update: LatencyTracker::new(capacity),
            confirmation: LatencyTracker::new(capacity),
            failures: Latency::new(16),
            error_count: 0,
        }
    }

    pub fn record_error(&mut self, id: &u64) {
        let Some(timer) = self.delivery.pending.remove(id) else {
            return;
        };
        self.update.pending.remove(id);
        self.failures.observe(timer.elapsed());
        self.error_count += 1;
    }
}

impl fmt::Display for LatencyStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "average: {:.4}, standard deviation: {:.4}, outliers: {}",
            self.avg, self.sd, self.outliers
        )
    }
}

impl fmt::Display for LatencyCollection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Delivery: {}", self.delivery.latency.compute())?;
        if !self.update.latency.observations.is_empty() {
            writeln!(f, "Update: {}", self.update.latency.compute())?;
        }
        if self.error_count > 0 {
            write!(
                f,
                "Failures: {}, Error Count: {}",
                self.failures.compute(),
                self.error_count
            )
        } else {
            Ok(())
        }
    }
}

impl LatencyCollection {
    pub fn as_abr_summary(&self) -> String {
        format!(
            "{:.1}/{}/{}",
            (self.delivery.latency.compute().avg * 1000.0) as u64,
            if !self.update.latency.observations.is_empty() {
                format!("{:.1}", (self.update.latency.compute().avg * 1000.0) as u64)
            } else {
                "NA".to_string()
            },
            (self.confirmation.latency.compute().avg * 1000.0) as u64,
        )
    }
}
