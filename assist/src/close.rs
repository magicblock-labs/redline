use commitment::CommitmentConfig;
use core::{config::Config, types::BenchResult};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    path::PathBuf,
    rc::Rc,
};

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
const CLOSE_MAX_ATTEMPTS: u32 = 3;

/// Closes benchmark accounts and refunds rent to vault.
pub async fn close(path: PathBuf) -> BenchResult<()> {
    tracing::info!("using config file at {path:?} to close benchmark accounts");
    let config = Config::from_path(path)?;
    let vault = crate::common::load_vault(&config)?;
    let accounts = {
        let keypairs = crate::common::load_payers(&config)?;
        crate::common::iter_pdas(
            &keypairs,
            usize::from(config.payers),
            config.benchmark.accounts_count,
            config.data.account_size as u32,
            config.authority,
        )
        .map(|(pubkey, _, _, _)| pubkey)
        .collect()
    };
    let closer = Rc::new(Closer {
        vault,
        ephem_client: crate::common::create_ephem_client(&config),
        chain_client: crate::common::create_chain_client(&config),
    });
    closer.close_accounts(accounts).await
}

/// Manages the closing of benchmark accounts.
struct Closer {
    vault: Keypair,
    ephem_client: Rc<RpcClient>,
    chain_client: Rc<RpcClient>,
}

impl Closer {
    async fn close_accounts(self: &Rc<Self>, accounts: HashSet<Pubkey>) -> BenchResult<()> {
        let total = accounts.len();
        let vault = self.vault.pubkey();
        let balance = self.chain_client.get_balance(&vault).await?;
        tracing::info!(%vault, balance_lamports = balance, accounts = total, "starting account cleanup");

        let (delegated, non_delegated) = self.separate_by_delegation(accounts).await?;
        tracing::info!(
            "found {} delegated, {} non-delegated and {} absent accounts",
            delegated.len(),
            non_delegated.len(),
            total - delegated.len() - non_delegated.len()
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
            let txn = Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&self.vault], hash);
            let signature = txn.signatures[0];
            tracing::info!(%signature, batch = idx + 1, accounts = ?batch, "sending commit-and-undelegate to ER");
            self.ephem_client
                .send_and_confirm_transaction(&txn)
                .await
                .map_err(|err| {
                    format!(
                        "ER commit-and-undelegate batch {} transaction {signature}: {err:?}",
                        idx + 1
                    )
                })?;
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
                let Some(owner) = this.account_owner(pubkey, "base-chain lookup").await? else {
                    tracing::info!(account = %pubkey, "skipping absent account");
                    return Ok(());
                };
                if owner == DELEGATION_PROGRAM_ID {
                    delegated.borrow_mut().push(pubkey);
                } else if owner == program::id() {
                    non_delegated.borrow_mut().push(pubkey);
                } else {
                    return Err(format!(
                        "account {pubkey} has unexpected base-chain owner {owner}"
                    )
                    .into());
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

            let still_delegated = Rc::new(Cell::new(0));

            crate::common::run_concurrent(accounts.iter().map(|&pubkey| {
                let still_delegated = still_delegated.clone();
                let this = self.clone();

                move || async move {
                    if this.account_owner(pubkey, "undelegation lookup").await?
                        == Some(DELEGATION_PROGRAM_ID)
                    {
                        tracing::warn!(account = %pubkey, "account still delegated on base chain");
                        still_delegated.set(still_delegated.get() + 1);
                    }
                    Ok(())
                }
            }))
            .await?;

            let count = still_delegated.get();
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

    /// Reads base-chain ownership at the same commitment used to confirm transactions.
    /// Missing accounts are expected when resuming a partially completed cleanup.
    async fn account_owner(&self, pubkey: Pubkey, context: &str) -> BenchResult<Option<Pubkey>> {
        Ok(self
            .chain_client
            .get_account_with_commitment(&pubkey, CommitmentConfig::confirmed())
            .await
            .map_err(|err| format!("{context} for {pubkey}: {err:?}"))?
            .value
            .map(|account| account.owner))
    }

    async fn close_on_chain(&self, accounts: &[Pubkey]) -> BenchResult<()> {
        tracing::info!("closing {} accounts on base chain", accounts.len());

        // Every close writes the vault; serialize to avoid shared-account contention.
        for &pubkey in accounts {
            self.close_account(pubkey).await?;
        }
        Ok(())
    }

    async fn close_account(&self, pubkey: Pubkey) -> BenchResult<()> {
        let payer = self.vault.pubkey();
        for attempt in 1..=CLOSE_MAX_ATTEMPTS {
            // A previous attempt may have landed despite a confirmation error.
            if self
                .account_owner(pubkey, "base-chain lookup")
                .await?
                .is_none()
            {
                tracing::info!(account = %pubkey, "account already absent on base chain");
                break;
            }
            let hash = self
                .chain_client
                .get_latest_blockhash()
                .await
                .map_err(|err| format!("base-chain blockhash for closing {pubkey}: {err:?}"))?;
            let ix = self.build_close_ix(payer, pubkey);
            let txn = Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&self.vault], hash);
            let signature = txn.signatures[0];
            tracing::info!(account = %pubkey, %signature, attempt, "sending close to base chain");
            let Err(err) = self.chain_client.send_and_confirm_transaction(&txn).await else {
                tracing::info!(account = %pubkey, %signature, "closed account on base chain");
                break;
            };
            tracing::warn!(account = %pubkey, %signature, attempt, error = ?err, "base-chain close failed");
            if attempt == CLOSE_MAX_ATTEMPTS || err.get_transaction_error().is_some() {
                let context =
                    format!("base-chain close for {pubkey}, transaction {signature}: {err:?}");
                if self
                    .account_owner(pubkey, "state lookup")
                    .await
                    .map_err(|lookup_err| format!("{context}; {lookup_err}"))?
                    .is_none()
                {
                    tracing::info!(account = %pubkey, %signature, "account absent after close confirmation error");
                    break;
                }
                return Err(context.into());
            }
            sleep(Duration::from_secs(2)).await;
        }
        Ok(())
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
}
