use core::{config::Config, types::BenchResult};
use std::{collections::HashSet, path::PathBuf, rc::Rc};

use instruction::{AccountMeta, Instruction as SolanaInstruction};
use keypair::Keypair;
use program::instruction::Instruction;
use pubkey::Pubkey;
use rpc::nonblocking::rpc_client::RpcClient;
use signer::Signer;
use transaction::Transaction;

/// # Close Command
///
/// The main entry point for the `close` command, responsible for closing benchmark accounts
/// and refunding rent to vault.json (the original payer).
pub async fn close(path: PathBuf) -> BenchResult<()> {
    tracing::info!("using config file at {path:?} to close benchmark accounts");
    let config = Config::from_path(path)?;
    let closer = Closer::new(&config).await?;

    closer.close_accounts().await?;

    Ok(())
}

/// # Closer
///
/// A struct that encapsulates the state and logic for closing benchmark accounts.
struct Closer {
    config: Config,
    vault: Keypair,
    client: Rc<RpcClient>,
    keypairs: Vec<Keypair>,
}

impl Closer {
    /// Creates a new `Closer` instance.
    async fn new(config: &Config) -> BenchResult<Self> {
        let vault = crate::common::load_vault(config)?;
        let keypairs = crate::common::load_payers(config)?;
        let client = crate::common::create_ephem_client(config);

        Ok(Self {
            config: config.clone(),
            vault,
            client,
            keypairs,
        })
    }

    /// Closes all benchmark accounts and refunds rent to vault (the original payer).
    async fn close_accounts(&self) -> BenchResult<()> {
        let accounts = self.extract_accounts();
        tracing::info!("closing {} accounts", accounts.len());

        let hash = self.client.get_latest_blockhash().await?;
        let payer = self.vault.pubkey();

        for pda_pubkey in accounts {
            let ix = Instruction::CloseAccount;
            let metas = vec![
                AccountMeta::new(payer, true), // vault as payer/signer
                AccountMeta::new(pda_pubkey, false),
            ];
            let close_ix = SolanaInstruction::new_with_bincode(program::id(), &ix, metas);

            let txn = Transaction::new_signed_with_payer(
                &[close_ix],
                Some(&payer),
                &[&self.vault], // vault signs
                hash,
            );

            match self.client.send_and_confirm_transaction(&txn).await {
                Ok(_) => {
                    tracing::info!("closed account {}", pda_pubkey);
                }
                Err(e) => {
                    tracing::warn!("failed to close account {}: {}", pda_pubkey, e);
                }
            }
        }

        tracing::info!("finished closing accounts, rent refunded to vault");
        Ok(())
    }

    /// Extracts all the necessary PDA addresses for closing from the configuration.
    fn extract_accounts(&self) -> HashSet<Pubkey> {
        let space = self.config.data.account_size as u32;
        self.derive_pdas(self.config.benchmark.accounts_count, space)
    }

    /// Derives a set of PDA addresses for a given number of accounts.
    fn derive_pdas(&self, count: u8, space: u32) -> HashSet<Pubkey> {
        crate::common::iter_pdas(
            &self.keypairs,
            self.config.payers as usize,
            count,
            space,
            self.config.authority,
        )
        .map(|(pubkey, _, _, _)| pubkey)
        .collect()
    }
}
