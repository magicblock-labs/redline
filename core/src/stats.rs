use json::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BenchStatistics {
    pub configuration: json::Value,
    pub http_requests_latency: ObservationsStats,
    pub account_update_latency: ObservationsStats,
    pub signature_confirmation_latency: ObservationsStats,

    pub transactions_per_second: ObservationsStats,
}

impl BenchStatistics {
    pub fn merge(mut stats: Vec<Self>) -> Self {
        let configuration = std::mem::take(&mut stats.first_mut().unwrap().configuration);

        let http_requests_latency =
            ObservationsStats::merge(stats.iter().map(|s| s.http_requests_latency).collect());
        let account_update_latency =
            ObservationsStats::merge(stats.iter().map(|s| s.account_update_latency).collect());
        let signature_confirmation_latency = ObservationsStats::merge(
            stats
                .iter()
                .map(|s| s.signature_confirmation_latency)
                .collect(),
        );
        let transactions_per_second =
            ObservationsStats::merge(stats.iter().map(|s| s.transactions_per_second).collect());

        BenchStatistics {
            configuration,
            http_requests_latency,
            account_update_latency,
            signature_confirmation_latency,
            transactions_per_second,
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Copy)]
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

impl ObservationsStats {
    pub fn merge(stats: Vec<ObservationsStats>) -> Self {
        let total_count = stats.len();
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
}

impl ObservationsStats {
    pub fn new(mut observations: Vec<u32>, invertedq: bool) -> Self {
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
            q95 - 1
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
