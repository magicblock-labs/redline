use json::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BenchStatistics {
    configuration: json::Value,
    http_requests_latency: ObservationsStats,
    account_update_latency: ObservationsStats,
    signature_confirmation_latency: ObservationsStats,
    transactions_per_second: ObservationsStats,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ObservationsStats {
    pub count: usize,
    pub median: u32,
    pub min: u32,
    pub max: u32,
    pub avg: u32,
    pub quantile95: u32,
    pub stddev: u32,
}

impl ObservationsStats {
    pub fn new(mut observations: Vec<u32>) -> Self {
        observations.sort();
        let count = observations.len();
        let sum: u64 = observations.iter().map(|&x| x as u64).sum();
        let avg = (sum / count as u64) as u32;
        let median = observations[count / 2];
        let min = *observations.first().unwrap();
        let max = *observations.last().unwrap();
        let quantile95 = observations[(count as f64 * 0.95).ceil() as usize - 1];

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
