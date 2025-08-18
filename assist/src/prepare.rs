use core::{
    config::Config,
    consts::KEYPAIRS_PATH,
    types::{BenchResult, TpsBenchMode},
};
use std::{
    cell::Cell, collections::HashSet, fs, hash::Hash, path::PathBuf, rc::Rc, time::Duration,
};

use commitment::CommitmentConfig;
use instruction::{AccountMeta, Instruction as SolanaInstruction};
use keypair::Keypair;
use program::{
    instruction::Instruction, utils::derive_pda, DelegateAccountMetas, DelegateAccounts,
    DELEGATION_PROGRAM_ID,
};
use pubkey::Pubkey;
use rpc::nonblocking::rpc_client::RpcClient;
use signer::{EncodableKey, Signer};
use tokio::task::LocalSet;
use transaction::Transaction;

const LAMPORTS_PER_BENCH: u64 = 500_000_000;
const CONFIRMED: CommitmentConfig = CommitmentConfig::confirmed();

struct Preparator {
    config: Config,
    vault: Keypair,
    client: Rc<RpcClient>,
    keypairs: Vec<Keypair>,
}

pub async fn prepare(path: PathBuf) -> BenchResult<()> {
    tracing::info!("using config file at {path:?} to prepare the benchmark");
    let config = Config::from_path(path)?;
    Preparator::generate(&config)?;
    let keypairs: Vec<_> = (1..=config.parallelism)
        .map(|n| Keypair::read_from_file(format!("{KEYPAIRS_PATH}/{n}.json")))
        .collect::<BenchResult<_>>()
        .inspect_err(|e| tracing::error!("failed to read keypairs for bench: {e}"))?;
    let vault = Keypair::read_from_file(format!("{KEYPAIRS_PATH}/vault.json"))
        .inspect_err(|e| tracing::error!("failed to read keypair for vault: {e}"))?;
    let client =
        RpcClient::new_with_commitment(config.connection.chain_url.0.to_string(), CONFIRMED);

    let pk = &vault.pubkey();
    let lamports = client.get_balance(pk).await?;
    const FIVE_SOL: u64 = 1_000_000_000 * 5;
    if lamports < FIVE_SOL {
        tracing::info!("Air dropping SOLs to vault",);
        client.request_airdrop(pk, FIVE_SOL).await?;
    }
    let preparator = Preparator {
        config,
        vault,
        client: client.into(),
        keypairs,
    };
    preparator.fund().await?;
    preparator.init().await?;

    Ok(())
}

impl Preparator {
    fn generate(config: &Config) -> BenchResult<()> {
        if !fs::exists(KEYPAIRS_PATH)? {
            tracing::info!("Generating benchmark keypairs");
            fs::create_dir(KEYPAIRS_PATH)?;
        };
        for n in 1..=config.parallelism {
            let path = format!("{KEYPAIRS_PATH}/{n}.json");
            if fs::exists(&path)? {
                continue;
            }
            Keypair::new().write_to_file(path)?;
        }
        let vault = format!("{KEYPAIRS_PATH}/vault.json");
        if !fs::exists(&vault)? {
            Keypair::new().write_to_file(vault)?;
        }

        Ok(())
    }

    async fn fund(&self) -> BenchResult<()> {
        let count = self.keypairs.len();
        for (i, kp) in self.keypairs.iter().enumerate() {
            let pk = &kp.pubkey();
            let lamports = self.client.get_balance(pk).await?;
            if lamports < LAMPORTS_PER_BENCH {
                tracing::info!(
                    "{:>03}/{count:>03} Funding keypair for benchmark: {pk}",
                    i + 1
                );
                self.transfer(pk, LAMPORTS_PER_BENCH - lamports).await?;
            }
        }
        Ok(())
    }

    async fn init(&self) -> BenchResult<()> {
        let space = self.config.data.account_size as u32;
        let tps_accounts = if self.config.tps_benchmark.enabled {
            self.extract_accounts_tps(&self.config.tps_benchmark.mode)
        } else {
            vec![]
        };
        let rps_accounts = if self.config.rps_benchmark.enabled {
            self.extract_accounts_rps(self.config.rps_benchmark.accounts_count)
        } else {
            vec![]
        };
        let accounts = tps_accounts
            .into_iter()
            .chain(rps_accounts.into_iter())
            .collect::<HashSet<_>>();
        let count = accounts.len();
        let local = LocalSet::new();
        let counter = Rc::new(Cell::new(0));
        for pda in accounts {
            let client = self.client.clone();
            let counter = counter.clone();
            let fut = async move {
                let response = client
                    .get_account_with_commitment(&pda.pubkey, Default::default())
                    .await?;
                let mut attempt = 0;
                match response.value {
                    Some(acc) if acc.owner == DELEGATION_PROGRAM_ID => {}
                    None => loop {
                        attempt += 1;
                        tokio::time::sleep(Duration::from_millis(
                            counter.get() as u64 * 50 * attempt,
                        ))
                        .await;
                        let hash = client.get_latest_blockhash().await?;
                        let ix = Instruction::InitAccount {
                            space,
                            seed: pda.seed,
                            bump: pda.bump,
                        };
                        let payer = pda.payer.pubkey();
                        let metas = vec![
                            AccountMeta::new(payer, true),
                            AccountMeta::new(pda.pubkey, false),
                            AccountMeta::new_readonly(Pubkey::default(), false),
                        ];
                        let ix = SolanaInstruction::new_with_bincode(program::id(), &ix, metas);
                        let txn = Transaction::new_signed_with_payer(
                            &[ix],
                            Some(&payer),
                            &[&pda.payer],
                            hash,
                        );
                        if let Err(error) = client.send_and_confirm_transaction(&txn).await {
                            tracing::error!(%error, "failed to send PDA init transaction");
                            continue;
                        }
                        if let Err(error) = Self::delegate(&client, &pda).await {
                            tracing::error!(%error, "failed to delegate the PDA");
                            continue;
                        }
                        break;
                    },
                    Some(_) => {
                        while let Err(error) = Self::delegate(&client, &pda).await {
                            tracing::error!(%error, "failed to delegate the PDA");
                            tokio::time::sleep(Duration::from_secs(attempt)).await;
                        }
                    }
                }
                let c = counter.get() + 1;
                counter.set(c);
                tracing::info!("{c:>03}/{count:>03} PDA {} is ready", pda.pubkey);
                BenchResult::Ok(())
            };
            local.spawn_local(fut);
        }
        local.await;
        tracing::info!("Prepared {count} PDA accounts");
        Ok(())
    }

    fn extract_accounts_tps(&self, mode: &TpsBenchMode) -> Vec<Pda> {
        use TpsBenchMode::*;
        let space = self.config.data.account_size as u32;

        let derive_accounts = |count: u8| -> Vec<Pda> {
            self.keypairs
                .iter()
                .flat_map(|k| (1..=count).map(move |seed| (k, seed)))
                .map(|(k, seed)| {
                    let (pubkey, bump) = derive_pda(k.pubkey(), space, seed);
                    Pda {
                        payer: k.insecure_clone(),
                        pubkey,
                        seed,
                        bump,
                    }
                })
                .collect()
        };

        match mode {
            SimpleByteSet { accounts_count }
            | HighCuCost { accounts_count, .. }
            | TriggerClones { accounts_count, .. }
            | ReadWrite { accounts_count }
            | ReadOnly { accounts_count, .. }
            | Commit { accounts_count, .. } => derive_accounts(*accounts_count),
            Mixed(modes) => {
                let mut accounts: Vec<_> = modes
                    .iter()
                    .flat_map(|m| self.extract_accounts_tps(&m.mode))
                    .collect();
                accounts.dedup_by_key(|a| a.pubkey);
                accounts
            }
        }
    }

    fn extract_accounts_rps(&self, count: u8) -> Vec<Pda> {
        let space = self.config.data.account_size as u32;

        self.keypairs
            .iter()
            .flat_map(|k| (0..count).map(move |seed| (k, seed)))
            .map(|(k, seed)| {
                let (pubkey, bump) = derive_pda(k.pubkey(), space, seed);
                Pda {
                    payer: k.insecure_clone(),
                    pubkey,
                    seed,
                    bump,
                }
            })
            .collect()
    }

    async fn delegate(client: &RpcClient, pda: &Pda) -> BenchResult<()> {
        let ix = Instruction::Delegate { seed: pda.seed };
        let payer = pda.payer.pubkey();
        let hash = client.get_latest_blockhash().await?;

        let accounts = DelegateAccounts::new(pda.pubkey, program::id());
        let metas = DelegateAccountMetas::from(accounts).into_vec(payer);

        let ix = SolanaInstruction::new_with_bincode(program::id(), &ix, metas);
        let txn = Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&pda.payer], hash);

        client.send_and_confirm_transaction(&txn).await?;
        Ok(())
    }

    async fn transfer(&self, to: &Pubkey, amount: u64) -> BenchResult<()> {
        let hash = self.client.get_latest_blockhash().await?;
        let txn = systransaction::transfer(&self.vault, to, amount, hash);
        self.client.send_and_confirm_transaction(&txn).await?;
        Ok(())
    }
}

struct Pda {
    payer: Keypair,
    pubkey: Pubkey,
    seed: u8,
    bump: u8,
}

impl PartialEq for Pda {
    fn eq(&self, other: &Self) -> bool {
        self.pubkey.eq(&other.pubkey)
    }
}

impl Eq for Pda {}

impl Hash for Pda {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pubkey.hash(state);
    }
}
