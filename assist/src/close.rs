use core::{config::Config, types::BenchResult};
use std::{cell::RefCell, collections::HashSet, path::PathBuf, rc::Rc};

use instruction::{AccountMeta, Instruction as SolanaInstruction};
use keypair::Keypair;
use program::{instruction::Instruction, DELEGATION_PROGRAM_ID};
use pubkey::Pubkey;
use rpc::nonblocking::rpc_client::RpcClient;
use sdk::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use signer::Signer;
use tokio::time::{sleep, Duration};
use transaction::Transaction;

const COMMIT_BATCH_SIZE: usize = 4;
const UNDELEGATION_WAIT_SECS: u64 = 10;
const VERIFICATION_MAX_RETRIES: u32 = 5;
const VERIFICATION_RETRY_DELAY_SECS: u64 = 20;

/// Closes benchmark accounts and refunds rent to vault.
pub async fn close(path: PathBuf) -> BenchResult<()> {
    tracing::info!("using config file at {path:?} to close benchmark accounts");
    let config = Config::from_path(path)?;
    let closer = Closer::new(&config).await?;
    closer.close_accounts().await
}

/// Manages the closing of benchmark accounts.
struct Closer {
    config: Config,
    vault: Keypair,
    ephem_client: Rc<RpcClient>,
    chain_client: Rc<RpcClient>,
    keypairs: Vec<Keypair>,
}

impl Closer {
    async fn new(config: &Config) -> BenchResult<Rc<Self>> {
        Ok(Self {
            config: config.clone(),
            vault: crate::common::load_vault(config)?,
            keypairs: crate::common::load_payers(config)?,
            ephem_client: crate::common::create_ephem_client(config),
            chain_client: crate::common::create_chain_client(config),
        }
        .into())
    }

    async fn close_accounts(self: &Rc<Self>) -> BenchResult<()> {
        let accounts = self.extract_accounts();
        tracing::info!("closing {} accounts", accounts.len());

        let (delegated, non_delegated) = self.separate_by_delegation(accounts).await?;
        tracing::info!(
            "found {} delegated and {} non-delegated accounts",
            delegated.len(),
            non_delegated.len()
        );

        if !delegated.is_empty() {
            self.process_delegated_accounts(&delegated).await?;
        }

        if !non_delegated.is_empty() {
            self.close_on_chain(&non_delegated).await?;
        }

        tracing::info!("finished closing accounts, rent refunded to vault");
        Ok(())
    }

    async fn process_delegated_accounts(self: &Rc<Self>, accounts: &[Pubkey]) -> BenchResult<()> {
        let payer = self.vault.pubkey();
        let total_batches = (accounts.len() + COMMIT_BATCH_SIZE - 1) / COMMIT_BATCH_SIZE;

        for (idx, batch) in accounts.chunks(COMMIT_BATCH_SIZE).enumerate() {
            // Commit and undelegate this batch
            let hash = self.ephem_client.get_latest_blockhash().await?;
            let ix = self.build_commit_undelegate_ix(idx as u64, payer, batch);
            let txn = Transaction::new_signed_with_payer(
                &[ix],
                Some(&payer),
                &[&self.vault],
                hash,
            );
            self.ephem_client.send_and_confirm_transaction(&txn).await?;
            tracing::info!(
                "batch {}/{}: committed and undelegated {} accounts",
                idx + 1,
                total_batches,
                batch.len()
            );

            // Wait for undelegation to finalize
            tracing::info!(
                "waiting {} seconds for batch {} undelegation to finalize...",
                UNDELEGATION_WAIT_SECS,
                idx + 1
            );
            sleep(Duration::from_secs(UNDELEGATION_WAIT_SECS)).await;

            // Verify this batch is undelegated
            self.verify_undelegation(batch).await?;
        }

        tracing::info!("all batches committed and undelegated successfully");
        self.close_on_chain(accounts).await
    }

    async fn separate_by_delegation(
        self: &Rc<Self>,
        accounts: HashSet<Pubkey>,
    ) -> BenchResult<(Vec<Pubkey>, Vec<Pubkey>)> {
        let delegated = Rc::new(RefCell::new(Vec::new()));
        let non_delegated = Rc::new(RefCell::new(Vec::new()));

        crate::common::run_concurrent(accounts.into_iter().map(|pubkey| {
            let delegated = delegated.clone();
            let non_delegated = non_delegated.clone();
            let this = self.clone();

            move || async move {
                let account = this.chain_client.get_account(&pubkey).await?;
                if account.owner == DELEGATION_PROGRAM_ID {
                    delegated.borrow_mut().push(pubkey);
                } else {
                    non_delegated.borrow_mut().push(pubkey);
                }
                Ok(())
            }
        }))
        .await?;

        Ok((
            Rc::try_unwrap(delegated).unwrap().into_inner(),
            Rc::try_unwrap(non_delegated).unwrap().into_inner(),
        ))
    }


    async fn verify_undelegation(self: &Rc<Self>, accounts: &[Pubkey]) -> BenchResult<()> {
        for retry in 0..VERIFICATION_MAX_RETRIES {
            tracing::info!(
                "verifying undelegation (attempt {}/{})",
                retry + 1,
                VERIFICATION_MAX_RETRIES
            );

            let still_delegated = Rc::new(RefCell::new(Vec::new()));

            crate::common::run_concurrent(accounts.iter().map(|&pubkey| {
                let still_delegated = still_delegated.clone();
                let this = self.clone();

                move || async move {
                    let account = this.chain_client.get_account(&pubkey).await?;
                    if account.owner == DELEGATION_PROGRAM_ID {
                        still_delegated.borrow_mut().push(pubkey);
                    }
                    Ok(())
                }
            }))
            .await?;

            let count = still_delegated.borrow().len();
            if count == 0 {
                tracing::info!("all accounts successfully undelegated");
                return Ok(());
            }

            tracing::warn!("{} accounts still delegated", count);
            if retry < VERIFICATION_MAX_RETRIES - 1 {
                sleep(Duration::from_secs(VERIFICATION_RETRY_DELAY_SECS)).await;
            }
        }

        Err("accounts still delegated after maximum retries".into())
    }

    async fn close_on_chain(self: &Rc<Self>, accounts: &[Pubkey]) -> BenchResult<()> {
        tracing::info!("closing {} accounts on base chain", accounts.len());

        let payer = self.vault.pubkey();

        crate::common::run_concurrent(accounts.iter().map(|&pubkey| {
            let this = self.clone();

            move || async move {
                let hash = this.chain_client.get_latest_blockhash().await?;
                let ix = this.build_close_ix(payer, pubkey);
                let txn =
                    Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&this.vault], hash);

                this.chain_client.send_and_confirm_transaction(&txn).await?;
                tracing::info!("closed account {} on base chain", pubkey);
                Ok(())
            }
        }))
        .await
    }

    fn build_commit_undelegate_ix(
        &self,
        id: u64,
        payer: Pubkey,
        accounts: &[Pubkey],
    ) -> SolanaInstruction {
        let ix = Instruction::CommitAndUndelegateAccounts { id };
        let mut metas = vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(MAGIC_CONTEXT_ID, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false),
        ];
        metas.extend(accounts.iter().map(|&pk| AccountMeta::new(pk, false)));
        SolanaInstruction::new_with_bincode(program::id(), &ix, metas)
    }

    fn build_close_ix(&self, payer: Pubkey, account: Pubkey) -> SolanaInstruction {
        let ix = Instruction::CloseAccount;
        let metas = vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(account, false),
        ];
        SolanaInstruction::new_with_bincode(program::id(), &ix, metas)
    }

    fn extract_accounts(&self) -> HashSet<Pubkey> {
        let space = self.config.data.account_size as u32;
        crate::common::iter_pdas(
            &self.keypairs,
            self.config.payers as usize,
            self.config.benchmark.accounts_count,
            space,
            self.config.authority,
        )
        .map(|(pubkey, _, _, _)| pubkey)
        .collect()
    }
}
