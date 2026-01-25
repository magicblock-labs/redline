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
use tokio::task::LocalSet;
use transaction::Transaction;

/// Funding per benchmark account (0.2 SOL).
/// Sufficient for ~1000 transactions at 0.0002 SOL/tx fee.
const BENCH_FUNDING: u64 = 200_000_000; // 0.2 SOL

/// Airdrop amount for vault initialization.
const VAULT_AIRDROP: u64 = 5_000_000_000; // 5 SOL

/// # Prepare Command
///
/// The main entry point for the `prepare` command, responsible for orchestrating the
/// entire preparation process.
pub async fn prepare(path: PathBuf) -> BenchResult<()> {
    tracing::info!("using config file at {path:?} to prepare the benchmark");
    let config = Config::from_path(path)?;
    Preparator::generate_keypairs(&config)?;
    let preparator = Preparator::new(&config).await?;

    preparator.fund_accounts().await?;
    preparator.initialize_pdas().await?;

    Ok(())
}

/// # Preparator
///
/// A struct that encapsulates the state and logic for preparing the benchmark environment.
struct Preparator {
    config: Config,
    vault: Keypair,
    client: Rc<RpcClient>,
    keypairs: Vec<Keypair>,
}

impl Preparator {
    /// # New Preparator
    ///
    /// Creates a new `Preparator` instance, loading the necessary keypairs and establishing
    /// a connection to the Solana cluster.
    async fn new(config: &Config) -> BenchResult<Rc<Self>> {
        let vault = crate::common::load_vault(config)?;
        let keypairs = crate::common::load_payers(config)?;
        let client = crate::common::create_chain_client(config);

        let pk = &vault.pubkey();
        let lamports = client.get_balance(pk).await?;
        if lamports < VAULT_AIRDROP / 2 {
            tracing::info!("Airdropping {} lamports to vault", VAULT_AIRDROP);
            client.request_airdrop(pk, VAULT_AIRDROP).await?;
        }

        Ok(Self {
            config: config.clone(),
            vault,
            client,
            keypairs,
        }
        .into())
    }

    /// # Generate Keypairs
    ///
    /// Generates the necessary keypairs for the benchmark if they do not already exist.
    fn generate_keypairs(config: &Config) -> BenchResult<()> {
        let keypath = &config.keypairs;
        if !fs::exists(keypath)? {
            tracing::info!("Generating benchmark keypairs");
            fs::create_dir(keypath)?;
        }
        for n in 1..=config.parallelism * config.payers {
            let path = keypath.join(format!("{n}.json"));
            if fs::exists(&path)? {
                continue;
            }
            Keypair::new().write_to_file(path)?;
        }
        let vault = keypath.join("vault.json");
        if !fs::exists(&vault)? {
            Keypair::new().write_to_file(vault)?;
        }
        Ok(())
    }

    /// # Fund Accounts
    ///
    /// Ensures that all keypairs have sufficient funds for the benchmark.
    async fn fund_accounts(&self) -> BenchResult<()> {
        for (i, kp) in self.keypairs.iter().enumerate() {
            let pk = &kp.pubkey();
            let account = self
                .client
                .get_account_with_commitment(pk, Default::default())
                .await?
                .value
                .unwrap_or_default();
            let lamports = BENCH_FUNDING.saturating_sub(account.lamports);
            if lamports > 0 {
                tracing::info!(
                    "{:>03}/{:>03} Funding keypair for benchmark: {pk}",
                    i + 1,
                    self.keypairs.len()
                );
                self.transfer(pk, lamports).await?;
            }
            if account.owner != DELEGATION_PROGRAM_ID {
                self.delegate_oncurve(kp).await?;
            }
        }
        Ok(())
    }

    /// # Initialize PDAs
    ///
    /// Creates and delegates all the necessary Program Derived Addresses (PDAs) for the benchmark.
    async fn initialize_pdas(self: &Rc<Self>) -> BenchResult<()> {
        let accounts = self.extract_accounts();
        let count = accounts.len();
        let local = LocalSet::new();

        let counter = Rc::new(RefCell::new(0u32));
        for pda in accounts {
            let counter = counter.clone();
            let this = self.clone();
            let fut = async move {
                let response = this
                    .client
                    .get_account_with_config(
                        &pda.pubkey,
                        RpcAccountInfoConfig {
                            encoding: Some(UiAccountEncoding::Base64Zstd),
                            ..Default::default()
                        },
                    )
                    .await
                    .inspect_err(|err| tracing::error!(%err, "failed to fetch PDA state"))?
                    .value;
                if response.is_none() {
                    this.create_and_delegate_pda(&pda).await.inspect_err(
                        |err| tracing::error!(%err, "failed to create/delegate PDA"),
                    )?;
                }
                counter.borrow_mut().add_assign(1);
                let i = *counter.borrow();

                tracing::info!("{i}/{} PDA {} is ready", count, pda.pubkey);
                BenchResult::Ok(())
            };
            local.spawn_local(fut);
        }
        local.await;
        tracing::info!("Prepared {count} PDA accounts");
        Ok(())
    }

    /// # Create and Delegate PDA
    ///
    /// A helper function to create and delegate a PDA.
    async fn create_and_delegate_pda(self: Rc<Self>, pda: &Pda) -> BenchResult<()> {
        let space = pda.space;
        let seed = pda.seed;
        let bump = pda.bump;
        let pubkey = pda.pubkey;
        let payer = self.vault.pubkey();

        let authority = self.config.authority;

        let hash = self.client.get_latest_blockhash().await?;
        let ix = Instruction::InitAccount {
            space,
            seed,
            bump,
            authority,
        };
        let metas = vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(pubkey, false),
            AccountMeta::new_readonly(pda.payer.pubkey(), false),
            AccountMeta::new_readonly(Pubkey::default(), false),
        ];
        let init_ix = SolanaInstruction::new_with_bincode(program::id(), &ix, metas);
        let ix = Instruction::Delegate {
            seed: pda.seed,
            authority,
        };
        let accounts = DelegateAccounts::new(pda.pubkey, program::id());
        let mut metas = DelegateAccountMetas::from(accounts).into_vec(payer);
        metas.push(AccountMeta::new_readonly(pda.payer.pubkey(), false));
        let delegate_ix = SolanaInstruction::new_with_bincode(program::id(), &ix, metas);
        let ixs = [init_ix, delegate_ix];
        let txn = Transaction::new_signed_with_payer(&ixs, Some(&payer), &[&self.vault], hash);
        self.client.send_and_confirm_transaction(&txn).await?;
        Ok(())
    }

    /// # Delegate On Curve Account
    ///
    /// A helper function to delegate an on curve account.
    async fn delegate_oncurve(&self, account: &Keypair) -> BenchResult<()> {
        let hash = self.client.get_latest_blockhash().await?;
        let payer = &self.vault;

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

        let signers = [payer, account];
        let ixs = &[assign_ix, delegate_ix];
        let txn = Transaction::new_signed_with_payer(ixs, Some(&payer.pubkey()), &signers, hash);
        self.client.send_and_confirm_transaction(&txn).await?;
        Ok(())
    }

    /// # Extract Accounts
    ///
    /// Extracts all the necessary PDAs for the benchmark from the configuration.
    fn extract_accounts(&self) -> HashSet<Pda> {
        let space = self.config.data.account_size as u32;
        self.derive_pdas(self.config.benchmark.accounts_count, space)
    }

    /// # Derive PDAs
    ///
    /// Derives a set of PDAs for a given number of accounts.
    fn derive_pdas(&self, count: u8, space: u32) -> HashSet<Pda> {
        crate::common::iter_pdas(
            &self.keypairs,
            self.config.payers as usize,
            count,
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

    /// # Transfer Funds
    ///
    /// Transfers a specified amount of lamports to a given public key.
    async fn transfer(&self, to: &Pubkey, amount: u64) -> BenchResult<()> {
        let hash = self.client.get_latest_blockhash().await?;
        let txn = systransaction::transfer(&self.vault, to, amount, hash);
        self.client.send_and_confirm_transaction(&txn).await?;
        Ok(())
    }
}

/// # PDA
///
/// A struct to hold the information for a Program Derived Address.
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
