use core::{config::Config, consts::KEYPAIRS_PATH, types::BenchResult};
use std::{
    cell::RefCell, collections::HashSet, fs, hash::Hash, ops::AddAssign, path::PathBuf, rc::Rc,
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
const FIVE_SOL: u64 = 1_000_000_000 * 5;

/// # Prepare Command
///
/// The main entry point for the `prepare` command, responsible for orchestrating the
/// entire preparation process.
pub async fn prepare(path: PathBuf) -> BenchResult<()> {
    tracing::info!("using config file at {path:?} to prepare the benchmark");
    let config = Config::from_path(path)?;
    let preparator = Preparator::new(&config).await?;

    preparator.generate_keypairs()?;
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
    async fn new(config: &Config) -> BenchResult<Self> {
        let keypairs: Vec<_> = (1..=config.parallelism)
            .map(|n| Keypair::read_from_file(format!("{KEYPAIRS_PATH}/{n}.json")))
            .collect::<BenchResult<_>>()
            .inspect_err(|e| tracing::error!("failed to read keypairs for bench: {e}"))?;
        let vault = Keypair::read_from_file(format!("{KEYPAIRS_PATH}/vault.json"))
            .inspect_err(|e| tracing::error!("failed to read keypair for vault: {e}"))?;
        let client = Rc::new(RpcClient::new_with_commitment(
            config.connection.chain_url.0.to_string(),
            CONFIRMED,
        ));

        let pk = &vault.pubkey();
        let lamports = client.get_balance(pk).await?;
        if lamports < FIVE_SOL {
            tracing::info!("Airdropping SOLs to vault");
            client.request_airdrop(pk, FIVE_SOL).await?;
        }

        Ok(Self {
            config: config.clone(),
            vault,
            client,
            keypairs,
        })
    }

    /// # Generate Keypairs
    ///
    /// Generates the necessary keypairs for the benchmark if they do not already exist.
    fn generate_keypairs(&self) -> BenchResult<()> {
        if !fs::exists(KEYPAIRS_PATH)? {
            tracing::info!("Generating benchmark keypairs");
            fs::create_dir(KEYPAIRS_PATH)?;
        }
        for n in 1..=self.config.parallelism {
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

    /// # Fund Accounts
    ///
    /// Ensures that all keypairs have sufficient funds for the benchmark.
    async fn fund_accounts(&self) -> BenchResult<()> {
        for (i, kp) in self.keypairs.iter().enumerate() {
            let pk = &kp.pubkey();
            let lamports = self.client.get_balance(pk).await?;
            if lamports < LAMPORTS_PER_BENCH {
                tracing::info!(
                    "{:>03}/{:>03} Funding keypair for benchmark: {pk}",
                    i + 1,
                    self.keypairs.len()
                );
                self.transfer(pk, LAMPORTS_PER_BENCH - lamports).await?;
            }
        }
        Ok(())
    }

    /// # Initialize PDAs
    ///
    /// Creates and delegates all the necessary Program Derived Addresses (PDAs) for the benchmark.
    async fn initialize_pdas(&self) -> BenchResult<()> {
        let accounts = self.extract_accounts();
        let count = accounts.len();
        let local = LocalSet::new();

        let counter = Rc::new(RefCell::new(0u32));
        for pda in accounts {
            let client = self.client.clone();
            let counter = counter.clone();
            let fut = async move {
                let response = client
                    .get_account_with_config(&pda.pubkey, Default::default())
                    .await?
                    .value;
                if response.is_none() {
                    Self::create_and_delegate_pda(&client, &pda).await?;
                } else if let Some(acc) = response {
                    if acc.owner != DELEGATION_PROGRAM_ID {
                        Self::delegate_pda(&client, &pda).await?;
                    }
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
    async fn create_and_delegate_pda(client: &Rc<RpcClient>, pda: &Pda) -> BenchResult<()> {
        let space = pda.space;
        let seed = pda.seed;
        let bump = pda.bump;
        let payer = pda.payer.insecure_clone();
        let pubkey = pda.pubkey;

        let hash = client.get_latest_blockhash().await?;
        let ix = Instruction::InitAccount { space, seed, bump };
        let metas = vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(pubkey, false),
            AccountMeta::new_readonly(Pubkey::default(), false),
        ];
        let ix = SolanaInstruction::new_with_bincode(program::id(), &ix, metas);
        let txn = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer], hash);
        client.send_and_confirm_transaction(&txn).await?;

        Self::delegate_pda(client, pda).await
    }

    /// # Delegate PDA
    ///
    /// A helper function to delegate a PDA.
    async fn delegate_pda(client: &Rc<RpcClient>, pda: &Pda) -> BenchResult<()> {
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
        let mut accounts = HashSet::new();
        for kp in &self.keypairs {
            for seed in 1..=count {
                let (pubkey, bump) = derive_pda(kp.pubkey(), space, seed);
                accounts.insert(Pda {
                    payer: kp.insecure_clone(),
                    pubkey,
                    seed,
                    bump,
                    space,
                });
            }
        }
        accounts
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
    payer: Keypair,
    pubkey: Pubkey,
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
