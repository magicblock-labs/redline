use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pubsub::nonblocking::pubsub_client::PubsubClient;
use sdk::consts::DELEGATION_PROGRAM_ID;
use solana::account::ReadableAccount;
use solana::native_token::LAMPORTS_PER_SOL;
use solana::signature::read_keypair_file;
use solana::signer::Signer;
use tokio::sync::Semaphore;

use crate::client::SolanaClient;
use crate::config::{BenchDuration, BenchMode, Config};
use crate::pda::Pda;
use crate::stats::LatencyCollection;

enum BenchModeInner {
    RawSpeed,
    CloneSpeed,
}

impl From<BenchMode> for BenchModeInner {
    fn from(mode: BenchMode) -> Self {
        match mode {
            BenchMode::RawSpeed { .. } => Self::RawSpeed,
            BenchMode::CloneSpeed { .. } => Self::CloneSpeed,
        }
    }
}

pub struct BenchRunner {
    chain: Rc<SolanaClient>,
    ephem: Rc<SolanaClient>,
    ws: Rc<PubsubClient>,
    pdas: Vec<Rc<Pda>>,
    mode: BenchModeInner,
    duration: BenchDuration,
    latency: Rc<RefCell<LatencyCollection>>,
    concurrency: Arc<Semaphore>,
    lag: Duration,
}

impl BenchRunner {
    pub async fn new(config: Config) -> Self {
        let chain = SolanaClient::new(config.chain);
        let ephem = SolanaClient::new(config.ephem);
        let ws: Rc<_> = PubsubClient::new(&config.ws)
            .await
            .expect("failed to connect to ws")
            .into();
        let mut pdas = Vec::<Rc<Pda>>::with_capacity(config.keypairs.len());
        let space = config.mode.space();
        for path in config.keypairs {
            let payer = read_keypair_file(path).expect("failed to read keypair file");
            let mut pda = Pda::new(&chain, payer, config.subscriptions, config.confirmations).await;
            if let BenchMode::CloneSpeed { noise } = config.mode {
                pda.generate_clones(&chain, noise).await;
            }
            pdas.push(pda.into());
        }
        let latency = Rc::new(RefCell::new(LatencyCollection::new(
            config.duration.iters(),
        )));
        for (offset, pda) in pdas.iter().enumerate() {
            println!("pda: {}", pda.pubkey);
            let (lamports, owner, size) = chain
                .get_account(&pda.pubkey)
                .await
                .map(|a| (a.lamports, a.owner, a.data().len() as u32))
                .unwrap_or_default();
            if config.subscriptions {
                pda.subscribe(ws.clone(), latency.clone(), offset as u64);
            }
            if matches!(config.mode, BenchMode::RawSpeed { local: true, .. }) {
                let _ = ephem
                    .request_airdrop(&pda.payer.pubkey(), LAMPORTS_PER_SOL)
                    .await;
                pda.init(&ephem, space).await;
                continue;
            }
            if lamports == 0 {
                pda.init(&chain, space).await
            }
            if size == space && owner == DELEGATION_PROGRAM_ID
                || matches!(config.mode, BenchMode::CloneSpeed { .. })
            {
                continue;
            }
            if size != space {
                if owner == DELEGATION_PROGRAM_ID {
                    //println!("undelegating PDA: {}", pda.pubkey);
                    pda.undelegate(&ephem).await;
                    tokio::time::sleep(Duration::from_secs(15)).await;
                }
                //println!("closing PDA: {}", pda.pubkey);
                pda.close(&chain).await;
                tokio::time::sleep(Duration::from_secs(15)).await;
                //println!("reopening PDA: {}", pda.pubkey);
                pda.init(&chain, space).await;
                tokio::time::sleep(Duration::from_secs(15)).await;
            }
            //println!("delegating PDA: {}", pda.pubkey);
            pda.delegate(&chain).await;
        }
        let mode = BenchModeInner::from(config.mode);
        Self {
            chain,
            ephem,
            mode,
            ws,
            pdas,
            duration: config.duration,
            latency,
            lag: Duration::from_micros(config.latency),
            concurrency: Arc::new(Semaphore::new(config.concurrency.unwrap_or(65536))),
        }
    }

    pub async fn run(self) -> (Rc<RefCell<LatencyCollection>>, f64) {
        let (duration, iters) = match self.duration {
            BenchDuration::Time(d) => (d, u64::MAX),
            BenchDuration::Iters(i) => (Duration::MAX, i),
        };
        let start = Instant::now();
        let mut i = 0;
        while start.elapsed() < duration && i < iters {
            let pda = self.pdas[iters as usize % self.pdas.len()].clone();
            let guard = self.concurrency.clone().acquire_owned().await.unwrap();
            let client = self.ephem.clone();
            let ws = self.ws.clone();
            let latency = self.latency.clone();
            match self.mode {
                BenchModeInner::RawSpeed => {
                    let task = pda.fill_space(client, ws, i, latency, guard, i);
                    tokio::task::spawn_local(task);
                }
                BenchModeInner::CloneSpeed => {
                    if i % 100 == 0 {
                        pda.topup(1, self.chain.clone());
                    }
                    let task = pda.compute_sum(client, ws, latency, guard, i);
                    tokio::task::spawn_local(task);
                }
            }
            i += 1;
            tokio::time::sleep(self.lag).await;
        }
        (
            self.latency.clone(),
            i as f64 / start.elapsed().as_secs_f64(),
        )
    }
}

impl Drop for BenchRunner {
    fn drop(&mut self) {
        self.ephem.shutdown();
        self.chain.shutdown();
    }
}
