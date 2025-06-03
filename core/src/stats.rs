use json::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TpsBenchStatistics {
    pub configuration: json::Value,
    pub send_txn_requests_latency: ObservationsStats,
    pub account_update_latency: ObservationsStats,
    pub signature_confirmation_latency: ObservationsStats,

    pub transactions_per_second: ObservationsStats,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RpsBenchStatistics {
    pub configuration: json::Value,
    pub latency: ObservationsStats,

    pub requests_per_second: ObservationsStats,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CombinedBenchStatistics {
    pub configuration: json::Value,
    pub get_request_latency: ObservationsStats,
    pub requests_per_second: ObservationsStats,

    pub send_txn_requests_latency: ObservationsStats,
    pub account_update_latency: ObservationsStats,
    pub signature_confirmation_latency: ObservationsStats,

    pub transactions_per_second: ObservationsStats,
}

#[derive(Serialize, Deserialize)]
pub enum BenchStatistics {
    Tps(TpsBenchStatistics),
    Rps(RpsBenchStatistics),
    Combined(CombinedBenchStatistics),
}

impl BenchStatistics {
    pub fn configuration(&self) -> &json::Value {
        match self {
            Self::Tps(s) => &s.configuration,
            Self::Rps(s) => &s.configuration,
            Self::Combined(s) => &s.configuration,
        }
    }

    pub fn merge_rps_to_tps(self, stats: Option<RpsBenchStatistics>) -> Self {
        let Some(stats) = stats else {
            return self;
        };
        let Self::Tps(s) = self else {
            return self;
        };
        let combined = CombinedBenchStatistics {
            configuration: stats.configuration,
            account_update_latency: s.account_update_latency,
            transactions_per_second: s.transactions_per_second,
            signature_confirmation_latency: s.signature_confirmation_latency,
            requests_per_second: stats.requests_per_second,
            send_txn_requests_latency: s.send_txn_requests_latency,
            get_request_latency: stats.latency,
        };
        Self::Combined(combined)
    }

    pub fn account_update_latency(&self) -> Option<ObservationsStats> {
        match self {
            Self::Tps(s) => Some(s.account_update_latency),
            Self::Combined(s) => Some(s.account_update_latency),
            Self::Rps(_) => None,
        }
    }

    pub fn send_txn_requests_latency(&self) -> Option<ObservationsStats> {
        match self {
            Self::Tps(s) => Some(s.send_txn_requests_latency),
            Self::Combined(s) => Some(s.send_txn_requests_latency),
            Self::Rps(_) => None,
        }
    }

    pub fn signature_confirmation_latency(&self) -> Option<ObservationsStats> {
        match self {
            Self::Tps(s) => Some(s.signature_confirmation_latency),
            Self::Combined(s) => Some(s.signature_confirmation_latency),
            Self::Rps(_) => None,
        }
    }

    pub fn transactions_per_second(&self) -> Option<ObservationsStats> {
        match self {
            Self::Tps(s) => Some(s.transactions_per_second),
            Self::Combined(s) => Some(s.transactions_per_second),
            Self::Rps(_) => None,
        }
    }

    pub fn requests_per_second(&self) -> Option<ObservationsStats> {
        match self {
            Self::Tps(_) => None,
            Self::Combined(s) => Some(s.requests_per_second),
            Self::Rps(s) => Some(s.requests_per_second),
        }
    }

    pub fn get_request_latency(&self) -> Option<ObservationsStats> {
        match self {
            Self::Tps(_) => None,
            Self::Combined(s) => Some(s.get_request_latency),
            Self::Rps(s) => Some(s.latency),
        }
    }
}

impl TpsBenchStatistics {
    pub fn merge(mut stats: Vec<Self>) -> BenchStatistics {
        let configuration = std::mem::take(&mut stats.first_mut().unwrap().configuration);

        let send_txn_requests_latency =
            ObservationsStats::merge(stats.iter().map(|s| s.send_txn_requests_latency).collect());
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

        BenchStatistics::Tps(Self {
            configuration,
            send_txn_requests_latency,
            account_update_latency,
            signature_confirmation_latency,
            transactions_per_second,
        })
    }
}

impl RpsBenchStatistics {
    pub fn merge(mut stats: Vec<Self>) -> Self {
        let configuration = std::mem::take(&mut stats.first_mut().unwrap().configuration);

        let latency = ObservationsStats::merge(stats.iter().map(|s| s.latency).collect());
        let requests_per_second =
            ObservationsStats::merge(stats.iter().map(|s| s.requests_per_second).collect());

        Self {
            configuration,
            latency,
            requests_per_second,
        }
    }
}

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
