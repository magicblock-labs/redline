use core::types::BenchMode;
use hash::Hash;
use instruction::{AccountMeta, Instruction as SolanaInstruction};
use keypair::Keypair;
use program::instruction::Instruction;
use pubkey::Pubkey;
use rand::{rngs::ThreadRng, seq::SliceRandom, thread_rng};
use sdk::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use signer::Signer;
use transaction::Transaction;

/// # Transaction Provider Trait
///
/// A generic trait for building requests, designed to unify both transaction-based
/// and RPC-based request generation.
pub trait TransactionProvider {
    /// Returns the name of the benchmark mode.
    fn name(&self) -> &'static str;
    /// Generates the instruction for the transaction.
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction;

    /// Wraps a given instruction in a `SolanaInstruction`.
    fn wrap_ix(&self, ix: Instruction, accounts: Vec<AccountMeta>) -> SolanaInstruction {
        SolanaInstruction::new_with_bincode(program::ID, &ix, accounts)
    }

    /// Generates a complete, signed transaction.
    fn generate(&mut self, id: u64, blockhash: Hash, signer: &Keypair) -> Transaction {
        let ix = self.generate_ix(id);
        let mut tx = Transaction::new_with_payer(&[ix], Some(&signer.pubkey()));
        tx.sign(&[signer], blockhash);
        tx
    }

    /// Returns a list of accounts used by the transaction provider.
    fn accounts(&self) -> Vec<Pubkey>;
}

/// # Base Provider
///
/// Generic provider for simple transaction patterns with consistent account selection.
struct BaseProvider<const READONLY: bool = false> {
    accounts: Vec<Pubkey>,
    count: usize,
    rng: ThreadRng,
}

impl<const RO: bool> BaseProvider<RO> {
    fn new(accounts: Vec<Pubkey>, count: usize) -> Self {
        Self {
            accounts,
            count,
            rng: thread_rng(),
        }
    }

    fn random_accounts(&mut self) -> Vec<AccountMeta> {
        self.accounts
            .choose_multiple(&mut self.rng, self.count)
            .map(|&pk| {
                if RO {
                    AccountMeta::new_readonly(pk, false)
                } else {
                    AccountMeta::new(pk, false)
                }
            })
            .collect()
    }
}

/// # SimpleByteSet Provider
///
/// Generates simple transactions that write a small set of bytes to multiple accounts.
/// This is useful for basic throughput testing.
pub struct SimpleByteSetProvider(BaseProvider<false>);

/// # HighCuCost Provider
///
/// Generates transactions with a high computational cost to stress the validator's
/// processing capabilities, writing results to multiple accounts.
pub struct HighCuCostProvider {
    accounts: Vec<Pubkey>,
    iters: u32,
    accounts_per_transaction: usize,
    rng: ThreadRng,
}

/// # ReadWrite Provider
///
/// Generates transactions that perform read and write operations across multiple accounts,
/// which is useful for testing lock contention.
pub struct ReadWriteProvider {
    accounts: Vec<Pubkey>,
    accounts_per_transaction: usize,
    rng: ThreadRng,
}

/// # ReadOnly Provider
///
/// Generates read-only transactions to measure parallel processing performance.
pub struct ReadOnlyProvider(BaseProvider<true>);

/// # Commit Provider
///
/// Generates transactions that commit the state to the base chain in the Ephemeral Rollup.
pub struct CommitProvider(BaseProvider<true>);

impl TransactionProvider for SimpleByteSetProvider {
    fn name(&self) -> &'static str {
        "SimpleByteSet"
    }
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction {
        let ix = Instruction::SimpleByteSet { id };
        let accounts = self.0.random_accounts();
        self.wrap_ix(ix, accounts)
    }
    fn accounts(&self) -> Vec<Pubkey> {
        self.0.accounts.clone()
    }
}

impl TransactionProvider for HighCuCostProvider {
    fn name(&self) -> &'static str {
        "HighCuCost"
    }
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction {
        // Use first account as init parameter
        let init = self.accounts[0];
        let ix = Instruction::ExpensiveHashCompute {
            id,
            init,
            iters: self.iters,
        };
        // Randomly select accounts for this transaction
        let selected = self
            .accounts
            .choose_multiple(&mut self.rng, self.accounts_per_transaction)
            .copied();
        let accounts = selected.map(|pda| AccountMeta::new(pda, false)).collect();
        self.wrap_ix(ix, accounts)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        self.accounts.clone()
    }
}

impl TransactionProvider for ReadWriteProvider {
    fn name(&self) -> &'static str {
        "ReadWrite"
    }
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction {
        let ix = Instruction::AccountDataCopy { id };
        // Randomly select accounts for this transaction
        let selected = self
            .accounts
            .choose_multiple(&mut self.rng, self.accounts_per_transaction)
            .copied();

        // First half read-only, second half writable (50/50 split)
        let all: Vec<_> = selected.collect();
        let split = all.len() / 2;
        let split = if split == 0 { 1 } else { split };

        let mut accounts = Vec::new();
        for i in 0..split {
            accounts.push(AccountMeta::new_readonly(all[i], false));
        }
        for i in split..all.len() {
            accounts.push(AccountMeta::new(all[i], false));
        }

        self.wrap_ix(ix, accounts)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        self.accounts.clone()
    }
}

impl TransactionProvider for ReadOnlyProvider {
    fn name(&self) -> &'static str {
        "ReadOnly"
    }
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction {
        let ix = Instruction::ReadAccountsData { id };
        let accounts = self.0.random_accounts();
        self.wrap_ix(ix, accounts)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        self.0.accounts.clone()
    }
}

impl TransactionProvider for CommitProvider {
    fn name(&self) -> &'static str {
        "Commit"
    }
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction {
        let ix = Instruction::CommitAccounts { id };
        let mut accounts = vec![
            AccountMeta::new(MAGIC_CONTEXT_ID, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false),
        ];
        accounts.extend(self.0.random_accounts());
        self.wrap_ix(ix, accounts)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        self.0.accounts.clone()
    }
}

/// # Make Provider
///
/// A factory function that creates a transaction provider based on the provided benchmark mode.
pub fn make_provider(mode: &BenchMode, accounts: Vec<Pubkey>) -> Box<dyn TransactionProvider> {
    match mode {
        BenchMode::SimpleByteSet {
            accounts_per_transaction,
        } => Box::new(SimpleByteSetProvider(BaseProvider::new(
            accounts,
            *accounts_per_transaction as usize,
        ))),
        BenchMode::ReadWrite {
            accounts_per_transaction,
        } => Box::new(ReadWriteProvider {
            accounts,
            accounts_per_transaction: *accounts_per_transaction as usize,
            rng: thread_rng(),
        }),
        BenchMode::HighCuCost {
            iters,
            accounts_per_transaction,
        } => Box::new(HighCuCostProvider {
            accounts,
            iters: *iters,
            accounts_per_transaction: *accounts_per_transaction as usize,
            rng: thread_rng(),
        }),
        BenchMode::ReadOnly {
            accounts_per_transaction,
        } => Box::new(ReadOnlyProvider(BaseProvider::new(
            accounts,
            *accounts_per_transaction as usize,
        ))),
        BenchMode::Commit {
            accounts_per_transaction,
        } => Box::new(CommitProvider(BaseProvider::new(
            accounts,
            *accounts_per_transaction as usize,
        ))),
        // This function is only for transaction-based modes, so it will panic
        // if an RPC-based mode is provided.
        _ => panic!("Unsupported mode for make_provider"),
    }
}
