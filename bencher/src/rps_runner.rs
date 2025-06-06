use core::{
    config::Config,
    stats::{ObservationsStats, RpsBenchStatistics},
    types::{AccountEncoding, BenchResult, RpsBenchMode},
};
use std::rc::Rc;

use hyper::Request;
use program::utils::derive_pda;
use pubkey::Pubkey;

use crate::{
    confirmation::{Confirmations, ConfirmationsDB},
    extractor::value_extractor,
    http::ConnectionPool,
    payload,
    rps::RpsManager,
    ShutDown, ShutDownSender,
};

pub trait GetRequestProvider {
    fn generate_request(&self, id: u64) -> Request<String>;
}

pub struct GetAccountInfoRequest {
    accounts: Vec<Pubkey>,
    encoding: AccountEncoding,
}

pub struct GetMultipleAccountsRequest {
    accounts: Vec<Pubkey>,
    encoding: AccountEncoding,
}

pub struct GetBalanceRequest {
    accounts: Vec<Pubkey>,
}

pub struct GetTokenAccountBalanceRequest {
    accounts: Vec<Pubkey>,
}

pub struct MixedRequestsProviders {
    providers: Vec<Box<dyn GetRequestProvider>>,
}

impl GetRequestProvider for GetAccountInfoRequest {
    fn generate_request(&self, id: u64) -> Request<String> {
        let pubkey = self.accounts[id as usize % self.accounts.len()];
        let payload = payload::get_account_info(pubkey, self.encoding, id);
        Request::new(payload)
    }
}

impl GetRequestProvider for GetMultipleAccountsRequest {
    fn generate_request(&self, id: u64) -> Request<String> {
        let payload = payload::get_multiple_accounts(&self.accounts, self.encoding, id);
        Request::new(payload)
    }
}

impl GetRequestProvider for GetBalanceRequest {
    fn generate_request(&self, id: u64) -> Request<String> {
        let pubkey = self.accounts[id as usize % self.accounts.len()];
        let payload = payload::get_balance(pubkey, id);
        Request::new(payload)
    }
}

impl GetRequestProvider for GetTokenAccountBalanceRequest {
    fn generate_request(&self, id: u64) -> Request<String> {
        let pubkey = self.accounts[id as usize % self.accounts.len()];
        let payload = payload::get_token_account_balance(pubkey, id);
        Request::new(payload)
    }
}

impl GetRequestProvider for MixedRequestsProviders {
    fn generate_request(&self, id: u64) -> Request<String> {
        self.providers[id as usize % self.providers.len()].generate_request(id)
    }
}

pub struct RpsBenchRunner {
    provider: Box<dyn GetRequestProvider>,
    connections: ConnectionPool,
    rps_manager: RpsManager,
    iterations: u64,
    shutdown: ShutDown,
    latencies: ConfirmationsDB<()>,

    config: json::Value,
}

impl RpsBenchRunner {
    pub async fn new(base: Pubkey, config: &Config) -> BenchResult<Self> {
        let connections = ConnectionPool::new(&config.connection).await?;
        let accounts_count = config.rps_benchmark.accounts_count;
        let space = config.data.account_size as u32;
        let encoding = config.data.account_encoding;
        let config = &config.rps_benchmark;
        let accounts = (0..accounts_count)
            .map(|seed| derive_pda(base, space, seed).0)
            .collect();
        let provider = make_provider(accounts, encoding, config.mode.clone());
        let rps_manager = RpsManager::new(config.concurrency, config.rps);
        let shutdown = ShutDownSender::init();
        Ok(Self {
            provider,
            connections,
            iterations: config.iterations,
            latencies: Confirmations::new().0,
            rps_manager,
            shutdown,

            config: json::to_value(&config).unwrap(),
        })
    }

    pub async fn run(mut self) -> RpsBenchResults {
        for i in 0..self.iterations {
            self.step(i).await;
        }
        println!(
            "The Benchmark run is complete, number of requests sent: {}",
            self.iterations
        );

        RpsBenchResults {
            configuration: self.config,
            latencies: self.latencies,
            rps: self.rps_manager.stats(),
        }
    }

    #[inline(always)]
    async fn step(&mut self, id: u64) {
        let mut con = self
            .connections
            .connection()
            .await
            .expect("connection closed");

        let permit = self.rps_manager.tick().await;
        let request = self.provider.generate_request(id);

        let response = con.send(request, value_extractor);
        let latency = self.latencies.clone();
        latency.borrow_mut().track(id, None);

        let shutdown = self.shutdown.clone();
        let task = async move {
            match response.resolve().await {
                Ok(Some(false)) => {
                    eprintln!("get request failed to be processed");
                }
                Err(err) => {
                    eprintln!("get request failed to be delivered: {err}");
                }
                _ => (),
            }
            latency.borrow_mut().observe(id, ());
            drop(permit);
            drop(shutdown)
        };
        tokio::task::spawn_local(task);
    }
}

fn make_provider(
    accounts: Vec<Pubkey>,
    encoding: AccountEncoding,
    mode: RpsBenchMode,
) -> Box<dyn GetRequestProvider> {
    use RpsBenchMode::*;
    match mode {
        GetAccountInfo => Box::new(GetAccountInfoRequest { accounts, encoding }),
        GetMutlipleAccounts => Box::new(GetMultipleAccountsRequest { accounts, encoding }),
        GetBalance => Box::new(GetBalanceRequest { accounts }),
        GetTokenAccountBalance => Box::new(GetTokenAccountBalanceRequest { accounts }),
        Mixed(modes) => {
            let mut providers = Vec::with_capacity(modes.len());
            for mode in modes {
                providers.push(make_provider(accounts.clone(), encoding, mode));
            }
            Box::new(MixedRequestsProviders { providers })
        }
    }
}

pub struct RpsBenchResults {
    configuration: json::Value,
    latencies: ConfirmationsDB<()>,
    rps: ObservationsStats,
}

impl RpsBenchResults {
    pub fn stats(self) -> RpsBenchStatistics {
        RpsBenchStatistics {
            configuration: self.configuration,
            latency: Rc::try_unwrap(self.latencies)
                .unwrap()
                .into_inner()
                .finalize(),
            requests_per_second: self.rps,
        }
    }
}
