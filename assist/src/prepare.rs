use core::{
    config::Config,
    consts::KEYPAIRS_PATH,
    types::{BenchResult, TpsBenchMode},
};
use std::path::PathBuf;

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
use transaction::Transaction;

const LAMPORTS_PER_BENCH: u64 = 500_000_000;
const CONFIRMED: CommitmentConfig = CommitmentConfig::confirmed();

struct Preparator {
    config: Config,
    vault: Keypair,
    client: RpcClient,
    keypairs: Vec<Keypair>,
}

pub async fn prepare(path: PathBuf) -> BenchResult<()> {
    let config = Config::from_path(path)?;
    let keypairs: Vec<_> = (1..=config.parallelism)
        .map(|n| Keypair::read_from_file(format!("{KEYPAIRS_PATH}/{n}.json")))
        .collect::<BenchResult<_>>()?;
    let vault = Keypair::read_from_file(format!("{KEYPAIRS_PATH}/vault.json"))?;
    let client = RpcClient::new(config.connection.chain_url.0.to_string());
    let preparator = Preparator {
        config,
        vault,
        client,
        keypairs,
    };
    preparator.fund().await?;
    preparator.init().await?;

    Ok(())
}

impl Preparator {
    async fn fund(&self) -> BenchResult<()> {
        for kp in &self.keypairs {
            let pk = &kp.pubkey();
            let lamports = self.client.get_balance(pk).await?;
            if lamports < LAMPORTS_PER_BENCH {
                println!("Funding keypair for benchmark: {pk}");
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
        for pda in tps_accounts.into_iter().chain(rps_accounts.into_iter()) {
            let response = self
                .client
                .get_account_with_commitment(&pda.pubkey, Default::default())
                .await?;
            let hash = self.client.get_latest_blockhash().await?;
            match response.value {
                Some(acc) if acc.owner == DELEGATION_PROGRAM_ID => {
                    continue;
                }
                None => {
                    println!("Initializing PDA: {}", pda.pubkey);
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
                    self.client
                        .send_and_confirm_transaction_with_spinner_and_commitment(&txn, CONFIRMED)
                        .await?;
                }
                _ => {}
            }
            println!("Delegating PDA: {}", pda.pubkey);
            self.delegate(&pda).await?;
        }
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
            SimpleByteSet | HighCuCost { .. } => derive_accounts(1),
            TriggerClones { accounts_count, .. }
            | ReadWrite { accounts_count }
            | ReadOnly { accounts_count, .. }
            | Commit { accounts_count, .. } => derive_accounts(*accounts_count),
            Mixed(modes) => {
                let mut accounts: Vec<_> = modes
                    .iter()
                    .flat_map(|m| self.extract_accounts_tps(m))
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

    async fn delegate(&self, pda: &Pda) -> BenchResult<()> {
        let ix = Instruction::Delegate { seed: pda.seed };
        let payer = pda.payer.pubkey();
        let hash = self.client.get_latest_blockhash().await?;

        let accounts = DelegateAccounts::new(pda.pubkey, program::id());
        let metas = DelegateAccountMetas::from(accounts).into_vec(payer);

        let ix = SolanaInstruction::new_with_bincode(program::id(), &ix, metas);
        let txn = Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&pda.payer], hash);

        self.client
            .send_and_confirm_transaction_with_spinner_and_commitment(&txn, CONFIRMED)
            .await?;
        Ok(())
    }

    async fn transfer(&self, to: &Pubkey, amount: u64) -> BenchResult<()> {
        let hash = self.client.get_latest_blockhash().await?;
        let txn = systransaction::transfer(&self.vault, to, amount, hash);
        self.client
            .send_and_confirm_transaction_with_spinner_and_commitment(&txn, CONFIRMED)
            .await?;
        Ok(())
    }
}

struct Pda {
    payer: Keypair,
    pubkey: Pubkey,
    seed: u8,
    bump: u8,
}
