use json::{Deserialize, Serialize};
use std::collections::HashMap;

/// # Benchmark Statistics
///
/// A unified structure for storing all benchmark statistics, with a clear distinction
/// between transaction and RPC request metrics.
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct BenchStatistics {
    /// The configuration used for the benchmark.
    pub configuration: json::Value,
    /// A map of statistics for each transaction-based benchmark mode.
    pub transaction_stats: HashMap<String, ObservationsStats>,
    /// A map of statistics for each RPC-based benchmark mode.
    pub rpc_request_stats: HashMap<String, ObservationsStats>,
    /// Latency for receiving signature confirmations.
    pub signature_confirmation_latency: ObservationsStats,
    /// Latency for receiving account updates.
    pub account_update_latency: ObservationsStats,
    /// Throughput statistics for the entire benchmark run.
    pub rps: ObservationsStats,
}

/// # Observation Statistics
///
/// A detailed breakdown of a set of observations, including count, median, min, max, average, 95th percentile, and standard deviation.
#[derive(Deserialize, Serialize, Clone, Copy, Default)]
#[serde(rename_all = "kebab-case")]
pub struct ObservationsStats {
    pub count: usize,
    pub median: i32,
    pub min: u32,
    pub max: u32,
    pub avg: i32,
    pub quantile95: i32,
    pub stddev: u32,
}

impl BenchStatistics {
    /// # Merge Statistics
    ///
    /// Merges a vector of `BenchStatistics` into a single, consolidated report.
    pub fn merge(mut stats: Vec<Self>) -> Self {
        if stats.is_empty() {
            return Self::default();
        }
        let configuration = std::mem::take(&mut stats.first_mut().unwrap().configuration);
        let mut transaction_stats = HashMap::new();
        let mut rpc_request_stats = HashMap::new();
        let mut rps = Vec::new();
        let mut account_update_stats = Vec::new();
        let mut signature_confirmation_stats = Vec::new();

        for s in stats {
            for (key, value) in s.transaction_stats {
                transaction_stats
                    .entry(key)
                    .or_insert_with(Vec::new)
                    .push(value);
            }
            for (key, value) in s.rpc_request_stats {
                rpc_request_stats
                    .entry(key)
                    .or_insert_with(Vec::new)
                    .push(value);
            }
            account_update_stats.push(s.account_update_latency);
            signature_confirmation_stats.push(s.signature_confirmation_latency);
            rps.push(s.rps);
        }

        let transaction_stats = transaction_stats
            .into_iter()
            .map(|(key, value)| (key, ObservationsStats::merge(value)))
            .collect();
        let rpc_request_stats = rpc_request_stats
            .into_iter()
            .map(|(key, value)| (key, ObservationsStats::merge(value)))
            .collect();

        Self {
            configuration,
            transaction_stats,
            account_update_latency: ObservationsStats::merge(account_update_stats),
            signature_confirmation_latency: ObservationsStats::merge(signature_confirmation_stats),
            rpc_request_stats,
            rps: ObservationsStats::merge(rps),
        }
    }
}

impl ObservationsStats {
    /// # Merge Observation Statistics
    ///
    /// Merges a vector of `ObservationsStats` into a single, consolidated report.
    pub fn merge(stats: Vec<ObservationsStats>) -> Self {
        let total_count = stats.len();
        if total_count == 0 {
            return Self::default();
        }
        let sum = stats.iter().fold(
            (0usize, 0i32, u32::MAX, 0u32, 0i32, 0i32, 0u32),
            |acc, stat| {
                (
                    acc.0 + stat.count,
                    acc.1 + stat.median,
                    acc.2.min(stat.min),
                    acc.3.max(stat.max),
                    acc.4 + stat.avg,
                    acc.5 + stat.quantile95,
                    acc.6 + stat.stddev,
                )
            },
        );

        ObservationsStats {
            count: sum.0,
            median: sum.1 / total_count as i32,
            min: sum.2,
            max: sum.3,
            avg: sum.4 / total_count as i32,
            quantile95: sum.5 / total_count as i32,
            stddev: sum.6 / total_count as u32,
        }
    }

    /// # New Observation Statistics
    ///
    /// Creates a new `ObservationsStats` instance from a vector of observations.
    pub fn new(mut observations: Vec<u32>, invertedq: bool) -> Self {
        if observations.is_empty() {
            return Self::default();
        }
        observations.sort();
        let count = observations.len();
        let sum: u64 = observations.iter().map(|&x| x as u64).sum();
        let avg = (sum / count as u64) as i32;
        let median = observations[count / 2] as i32;
        let min = *observations.first().unwrap();
        let max = *observations.last().unwrap();
        let q95 = (count as f64 * 0.95).ceil() as usize;
        let qindex = if invertedq {
            count.saturating_sub(q95 + 1)
        } else {
            q95.saturating_sub(1)
        };
        let quantile95 = observations[qindex] as i32;

        let variance = observations
            .iter()
            .map(|&x| ((x as i64 - avg as i64).pow(2)) as u64)
            .sum::<u64>()
            / count as u64;
        let stddev = (variance as f64).sqrt() as u32;

        ObservationsStats {
            count,
            median,
            min,
            max,
            avg,
            quantile95,
            stddev,
        }
    }
}
