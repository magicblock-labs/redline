use std::rc::Rc;
use std::time::{Duration, Instant};

use benchprog::instruction::Instruction;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use solana::instruction::{AccountMeta, Instruction as SolanaInstruction};
use solana::pubkey::Pubkey;
use solana::signature::read_keypair_file;
use solana::signer::Signer;
use solana::transaction::Transaction;
use tokio::task::JoinHandle;
use tokio::time::interval;

use crate::accounts_reader::AccountReader;
use crate::config::{BenchDuration, BenchMode, Config};
use crate::http::{SolanaClient, TxnRequester};
use crate::pda::Pda;
use crate::stats::{LatencyCollection, TxnRequestStats};
use crate::ws::{WebsocketClient, WsNotification, WsNotificationType, WsSubscription};

enum BenchModeInner {
    RawSpeed,
    CloneSpeed { accounts: u8, reader: AccountReader },
}

impl From<BenchMode> for BenchModeInner {
    fn from(mode: BenchMode) -> Self {
        match mode {
            BenchMode::RawSpeed { .. } => Self::RawSpeed,
            BenchMode::CloneSpeed { accounts, pubkeys } => Self::CloneSpeed {
                accounts,
                reader: AccountReader::new(pubkeys),
            },
        }
    }
}

pub struct BenchRunner {
    chain: Rc<TxnRequester>,
    ephem: Rc<TxnRequester>,
    concurrency: usize,
    pdas: Vec<Rc<Pda>>,
    next: usize,
    mode: BenchModeInner,
    duration: BenchDuration,
    ws: WebsocketClient,
    latency: LatencyCollection,
    pending: FuturesUnordered<JoinHandle<TxnRequestStats>>,
}

impl BenchRunner {
    pub async fn new(config: Config) -> Self {
        let http = SolanaClient::default();
        let keypairs = config
            .keypairs
            .into_iter()
            .map(|p| read_keypair_file(p).expect("invalid payer keypair"));
        let mut pdas = Vec::with_capacity(keypairs.len());
        for k in keypairs {
            pdas.push(Pda::new(config.chain.clone(), &http, k).await);
        }
        let chain = Rc::new(TxnRequester::new(config.chain));
        let ephem = Rc::new(TxnRequester::new(config.ephem));
        let mode = config.mode;
        let space = match mode {
            BenchMode::RawSpeed { space } => space,
            BenchMode::CloneSpeed { .. } => size_of::<u64>() as u32,
        };
        let mut ws = WebsocketClient::connect(&config.ws)
            .await
            .expect("failed to connect to websocket");
        for (i, p) in pdas.iter_mut().enumerate() {
            let id = (u32::MAX - i as u32) as u64;
            ws.subscribe(WsSubscription::account(p.pubkey, id)).await;
            // we loop to ignore potential account update notifications
            loop {
                if let WsNotification {
                    id,
                    ty: WsNotificationType::Result(_),
                } = ws.next().await
                {
                    p.sub = id;
                    break;
                }
            }
            let account = http.info(chain.url.clone(), &p.pubkey).await;
            if account.size as u32 == space {
                continue;
            }
            if account.delegated {
                p.undelegate(ephem.clone()).await;
                // undelegation might take a while
                tokio::time::sleep(Duration::from_secs(12)).await;
            }
            p.close(chain.clone()).await;
            p.init(chain.clone(), space).await;
            p.delegate(chain.clone()).await;
        }
        Self {
            chain,
            ephem,
            concurrency: config.concurrency.unwrap_or(usize::MAX),
            pdas: pdas.into_iter().map(Into::into).collect(),
            next: 0,
            mode: mode.into(),
            duration: config.duration,
            ws,
            latency: Default::default(),
            pending: Default::default(),
        }
    }

    pub async fn run(mut self) {
        let (mut iters, limit) = match self.duration {
            BenchDuration::Time(d) => (u64::MAX, d),
            BenchDuration::Iters(i) => (i, Duration::MAX),
        };
        let start = Instant::now();
        let mut blockhash_refresher = interval(Duration::from_secs(45));
        while iters > 0 && start.elapsed() > limit {
            tokio::select! {
                biased;
                Some(Ok(stat)) = self.pending.next(), if !self.pending.is_empty() => {
                    if stat.success {
                        self.latency.delivery.confirm(&stat.id);
                    } else {
                        self.latency.record_error(&stat.id);
                    }
                }
                msg = self.ws.next() => {
                    match msg.ty {
                        WsNotificationType::Result(rid) => {
                            self.latency.confirmation.replace_id(rid, msg.id);
                        }
                        WsNotificationType::Signature => {
                            self.latency.confirmation.confirm(&msg.id);
                        }
                        WsNotificationType::Account => {
                            self.latency.update.confirm(&msg.id);
                        }
                    }
                }
                _ = blockhash_refresher.tick() => {
                    self.chain.refresh_blockhash().await;
                    self.ephem.refresh_blockhash().await;
                }
            }
            if self.pending.len() == self.concurrency {
                continue;
            }
            let pda = self.next();
            match self.mode {
                BenchModeInner::RawSpeed => self.fill_space(iters as u8, iters, pda).await,
                BenchModeInner::CloneSpeed {
                    accounts,
                    ref mut reader,
                } => {
                    let accounts = reader.next(accounts);
                    self.compute_sum(accounts, iters, pda).await;
                }
            }
            iters -= 1;
        }
    }

    fn next(&mut self) -> Rc<Pda> {
        self.next = (self.next + 1) % self.pdas.len();
        self.pdas[self.next].clone()
    }

    async fn transact(&mut self, ix: Instruction, pda: Rc<Pda>, metas: Vec<AccountMeta>, id: u64) {
        let payer = &pda.payer;
        let ix = SolanaInstruction::new_with_borsh(benchprog::ID, &ix, metas);

        let mut txn = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
        let hash = *self.ephem.hash.borrow();

        txn.sign(&[payer], hash);
        let sig = txn.signatures[0];
        self.ws.subscribe(WsSubscription::signature(sig, id)).await;
        self.latency.confirmation.track(id);
        self.latency.delivery.track(id);
        self.latency.update.track(pda.sub);
        let task = tokio::task::spawn_local(self.ephem.clone().send(txn, id));
        self.pending.push(task);
    }

    async fn fill_space(&mut self, value: u8, id: u64, pda: Rc<Pda>) {
        let ix = Instruction::FillSpace { value };
        let metas = vec![AccountMeta::new(pda.pubkey, false)];
        self.transact(ix, pda, metas, id).await;
    }

    async fn compute_sum(&mut self, accounts: Vec<Pubkey>, id: u64, pda: Rc<Pda>) {
        let ix = Instruction::ComputeSum { index: 0 };
        let mut metas = vec![AccountMeta::new(pda.pubkey, false)];
        metas.extend(
            accounts
                .into_iter()
                .map(|pk| AccountMeta::new_readonly(pk, false)),
        );
        self.transact(ix, pda, metas, id).await;
    }
}
