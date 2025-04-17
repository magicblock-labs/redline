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
use crate::config::{BenchDuration, BenchMode, ConfigPermuation};
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
    pdas: Vec<Rc<Pda>>,
    mode: BenchModeInner,
    duration: BenchDuration,
    latency: Rc<RefCell<LatencyCollection>>,
    concurrency: Arc<Semaphore>,
    inter_txn_lag: bool,
    ws: Rc<PubsubClient>,
}

impl BenchRunner {
    pub async fn new(config: ConfigPermuation) -> Self {
        let chain = SolanaClient::new(config.chain, 1);
        let ephem = SolanaClient::new(config.ephem, 35);
        let ws: Rc<_> = PubsubClient::new(&config.ws)
            .await
            .expect("failed to connect to ws")
            .into();
        let mut pdas = Vec::<Rc<Pda>>::with_capacity(config.keypairs.len());
        let space = config.mode.space();
        for path in config.keypairs {
            let payer = read_keypair_file(path).expect("failed to read keypair file");
            let mut pda = Pda::new(&chain, payer, !config.preflight_check).await;
            if let BenchMode::CloneSpeed { noise } = config.mode {
                pda.generate_clones(&chain, noise).await;
            }
            pdas.push(pda.into());
        }
        let latency = Rc::new(RefCell::new(LatencyCollection::new(
            config.duration.iters(),
        )));
        for (offset, pda) in pdas.iter().enumerate() {
            let (lamports, owner, size) = chain
                .get_account(&pda.pubkey)
                .await
                .map(|a| (a.lamports, a.owner, a.data().len() as u32))
                .unwrap_or_default();
            pda.subscribe(
                ws.clone(),
                latency.clone(),
                offset as u64,
                config.duration.iters() as u64 - 1,
            );
            if lamports == 0 {
                let _ = ephem
                    .request_airdrop(&pda.payer.pubkey(), LAMPORTS_PER_SOL)
                    .await;
                //pda.init(&ephem, space).await;
                continue;
            }

            if size == space && owner == DELEGATION_PROGRAM_ID
                || matches!(config.mode, BenchMode::CloneSpeed { .. })
            {
                continue;
            }
            if size != space {
                if owner == DELEGATION_PROGRAM_ID {
                    //println!("undelegating PDA: {}", pda.pubkey);
                    pda.undelegate(&chain).await;
                    tokio::time::sleep(Duration::from_secs(15)).await;
                }
                println!("closing PDA: {}", pda.pubkey);
                pda.close(&chain).await;
                tokio::time::sleep(Duration::from_secs(10)).await;
                //println!("reopening PDA: {}", pda.pubkey);
                pda.init(&chain, space).await;
                tokio::time::sleep(Duration::from_secs(15)).await;
            }
            println!("delegating PDA: {}", pda.pubkey);
            //pda.delegate(&chain).await;
        }
        let mode = BenchModeInner::from(config.mode);
        Self {
            chain,
            ephem,
            ws,
            mode,
            pdas,
            duration: config.duration,
            latency,
            concurrency: Arc::new(Semaphore::new(config.concurrency)),
            inter_txn_lag: config.inter_txn_lag,
        }
    }

    pub async fn run(self) -> (Rc<RefCell<LatencyCollection>>, f64) {
        let (duration, iters) = match self.duration {
            BenchDuration::Time(d) => (d, u64::MAX),
            BenchDuration::Iters(i) => (Duration::MAX, i),
        };
        tokio::time::sleep(Duration::from_secs(1)).await;
        let start = Instant::now();
        let mut i = 0;
        while start.elapsed() < duration && i < iters {
            let pda = self.pdas[iters as usize % self.pdas.len()].clone();
            //if self.inter_txn_lag && self.concurrency.available_permits() % 10 == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
            //}
            let guard = self.concurrency.clone().acquire_owned().await.unwrap();
            let client = self.ephem.clone();
            let latency = self.latency.clone();
            match self.mode {
                BenchModeInner::RawSpeed => {
                    let task = pda.fill_space(client, self.ws.clone(), i, latency, guard, i);
                    tokio::task::spawn_local(task);
                }
                BenchModeInner::CloneSpeed => {
                    if i % 100 == 0 {
                        pda.topup(1, self.chain.clone());
                    }
                    let task = pda.compute_sum(client, self.ws.clone(), latency, guard, i);
                    tokio::task::spawn_local(task);
                }
            }
            i += 1;
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
