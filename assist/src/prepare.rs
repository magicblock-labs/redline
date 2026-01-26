use core::{config::Config, types::BenchResult};
use std::{
    cell::RefCell, collections::HashSet, fs, hash::Hash, ops::AddAssign, path::PathBuf, rc::Rc,
};

use decoder_types::UiAccountEncoding;
use dlp::{args::DelegateArgs, instruction_builder::delegate};
use instruction::{AccountMeta, Instruction as SolanaInstruction};
use keypair::Keypair;
use program::{
    instruction::Instruction, DelegateAccountMetas, DelegateAccounts, DELEGATION_PROGRAM_ID,
};
use pubkey::Pubkey;
use rpc::nonblocking::rpc_client::RpcClient;
use rpc_types::config::RpcAccountInfoConfig;
use signer::{EncodableKey, Signer};
use solana_system_interface::instruction as sysinstruction;
use transaction::Transaction;

const BENCH_FUNDING: u64 = 200_000_000; // 0.2 SOL
const VAULT_AIRDROP: u64 = 5_000_000_000; // 5 SOL

/// Prepares the benchmark environment.
pub async fn prepare(path: PathBuf) -> BenchResult<()> {
    tracing::info!("using config file at {path:?} to prepare the benchmark");
    let config = Config::from_path(path)?;
    Preparator::generate_keypairs(&config)?;

    let preparator = Preparator::new(&config).await?;
    if !config.gasless {
        preparator.fund_accounts().await?;
    }
    preparator.initialize_pdas().await
}

/// Manages benchmark environment preparation.
struct Preparator {
    config: Config,
    vault: Keypair,
    client: Rc<RpcClient>,
    keypairs: Vec<Keypair>,
}

impl Preparator {
    async fn new(config: &Config) -> BenchResult<Rc<Self>> {
        let vault = crate::common::load_vault(config)?;
        let keypairs = crate::common::load_payers(config)?;
        let client = crate::common::create_chain_client(config);

        Self::ensure_vault_funded(&client, &vault).await?;

        Ok(Self {
            config: config.clone(),
            vault,
            client,
            keypairs,
        }
        .into())
    }

    async fn ensure_vault_funded(client: &RpcClient, vault: &Keypair) -> BenchResult<()> {
        let lamports = client.get_balance(&vault.pubkey()).await?;
        if lamports < VAULT_AIRDROP / 2 {
            tracing::info!("airdropping {} lamports to vault", VAULT_AIRDROP);
            client
                .request_airdrop(&vault.pubkey(), VAULT_AIRDROP)
                .await?;
        }
        Ok(())
    }

    fn generate_keypairs(config: &Config) -> BenchResult<()> {
        let keypath = &config.keypairs;
        if !fs::exists(keypath)? {
            tracing::info!("generating benchmark keypairs");
            fs::create_dir(keypath)?;
        }

        for n in 1..=config.parallelism * config.payers {
            let path = keypath.join(format!("{n}.json"));
            if !fs::exists(&path)? {
                Keypair::new().write_to_file(path)?;
            }
        }

        let vault_path = keypath.join("vault.json");
        if !fs::exists(&vault_path)? {
            Keypair::new().write_to_file(vault_path)?;
        }

        Ok(())
    }

    async fn fund_accounts(&self) -> BenchResult<()> {
        for (i, kp) in self.keypairs.iter().enumerate() {
            let pk = kp.pubkey();
            let account = self
                .client
                .get_account_with_commitment(&pk, Default::default())
                .await?
                .value
                .unwrap_or_default();

            let lamports_needed = BENCH_FUNDING.saturating_sub(account.lamports);
            if lamports_needed > 0 {
                tracing::info!(
                    "{:>03}/{:>03} funding keypair: {pk}",
                    i + 1,
                    self.keypairs.len()
                );
                self.transfer(&pk, lamports_needed).await?;
            }

            if account.owner != DELEGATION_PROGRAM_ID {
                self.delegate_oncurve(kp).await?;
            }
        }
        Ok(())
    }

    async fn initialize_pdas(self: &Rc<Self>) -> BenchResult<()> {
        let pdas = self.extract_pdas();
        let count = pdas.len();
        tracing::info!("initializing {} PDAs", count);

        let counter = Rc::new(RefCell::new(0u32));

        crate::common::run_concurrent(pdas.into_iter().map(|pda| {
            let this = self.clone();
            let counter = counter.clone();

            move || async move {
                this.ensure_pda_ready(&pda).await?;

                counter.borrow_mut().add_assign(1);
                let i = *counter.borrow();
                tracing::info!("{i}/{count} PDA {} is ready", pda.pubkey);
                Ok(())
            }
        }))
        .await?;

        tracing::info!("prepared {count} PDA accounts");
        Ok(())
    }

    async fn ensure_pda_ready(self: &Rc<Self>, pda: &Pda) -> BenchResult<()> {
        let account = self
            .client
            .get_account_with_config(
                &pda.pubkey,
                RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::Base64Zstd),
                    ..Default::default()
                },
            )
            .await?
            .value;

        match account {
            None => self.create_and_delegate_pda(pda).await,
            Some(acc) if acc.owner != DELEGATION_PROGRAM_ID => {
                tracing::info!(
                    "PDA {} exists but not delegated, delegating now",
                    pda.pubkey
                );
                self.delegate_pda(pda).await
            }
            Some(_) => {
                tracing::debug!("PDA {} already ready", pda.pubkey);
                Ok(())
            }
        }
    }

    async fn create_and_delegate_pda(self: &Rc<Self>, pda: &Pda) -> BenchResult<()> {
        let payer = self.vault.pubkey();
        let hash = self.client.get_latest_blockhash().await?;

        let init_ix = self.build_init_ix(payer, pda);
        let delegate_ix = self.build_delegate_ix(payer, pda);

        let txn = Transaction::new_signed_with_payer(
            &[init_ix, delegate_ix],
            Some(&payer),
            &[&self.vault],
            hash,
        );
        self.client.send_and_confirm_transaction(&txn).await?;
        Ok(())
    }

    async fn delegate_pda(self: &Rc<Self>, pda: &Pda) -> BenchResult<()> {
        let payer = self.vault.pubkey();
        let hash = self.client.get_latest_blockhash().await?;

        let delegate_ix = self.build_delegate_ix(payer, pda);
        let txn =
            Transaction::new_signed_with_payer(&[delegate_ix], Some(&payer), &[&self.vault], hash);
        self.client.send_and_confirm_transaction(&txn).await?;
        Ok(())
    }

    async fn delegate_oncurve(&self, account: &Keypair) -> BenchResult<()> {
        let payer = &self.vault;
        let hash = self.client.get_latest_blockhash().await?;

        let assign_ix = sysinstruction::assign(&account.pubkey(), &DELEGATION_PROGRAM_ID);
        let delegate_ix = delegate(
            payer.pubkey(),
            account.pubkey(),
            None,
            DelegateArgs {
                commit_frequency_ms: u32::MAX,
                seeds: vec![],
                validator: Some(self.config.authority),
            },
        );

        let txn = Transaction::new_signed_with_payer(
            &[assign_ix, delegate_ix],
            Some(&payer.pubkey()),
            &[payer, account],
            hash,
        );
        self.client.send_and_confirm_transaction(&txn).await?;
        Ok(())
    }

    async fn transfer(&self, to: &Pubkey, amount: u64) -> BenchResult<()> {
        let hash = self.client.get_latest_blockhash().await?;
        let txn = systransaction::transfer(&self.vault, to, amount, hash);
        self.client.send_and_confirm_transaction(&txn).await?;
        Ok(())
    }

    fn build_init_ix(&self, payer: Pubkey, pda: &Pda) -> SolanaInstruction {
        let ix = Instruction::InitAccount {
            space: pda.space,
            seed: pda.seed,
            bump: pda.bump,
            authority: self.config.authority,
        };
        let metas = vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(pda.pubkey, false),
            AccountMeta::new_readonly(pda.payer.pubkey(), false),
            AccountMeta::new_readonly(Pubkey::default(), false),
        ];
        SolanaInstruction::new_with_bincode(program::id(), &ix, metas)
    }

    fn build_delegate_ix(&self, payer: Pubkey, pda: &Pda) -> SolanaInstruction {
        let ix = Instruction::Delegate {
            seed: pda.seed,
            authority: self.config.authority,
        };
        let accounts = DelegateAccounts::new(pda.pubkey, program::id());
        let mut metas = DelegateAccountMetas::from(accounts).into_vec(payer);
        metas.push(AccountMeta::new_readonly(pda.payer.pubkey(), false));
        SolanaInstruction::new_with_bincode(program::id(), &ix, metas)
    }

    fn extract_pdas(&self) -> HashSet<Pda> {
        let space = self.config.data.account_size as u32;
        crate::common::iter_pdas(
            &self.keypairs,
            self.config.payers as usize,
            self.config.benchmark.accounts_count,
            space,
            self.config.authority,
        )
        .map(|(pubkey, bump, seed, payer)| Pda {
            pubkey,
            payer,
            seed,
            bump,
            space,
        })
        .collect()
    }
}

struct Pda {
    pubkey: Pubkey,
    payer: Keypair,
    seed: u8,
    bump: u8,
    space: u32,
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
