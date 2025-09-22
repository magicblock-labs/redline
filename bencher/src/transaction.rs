use core::types::BenchMode;
use hash::Hash;
use instruction::{AccountMeta, Instruction as SolanaInstruction};
use keypair::Keypair;
use program::instruction::Instruction;
use pubkey::Pubkey;
use rand::{
    distributions::WeightedIndex, prelude::Distribution, rngs::ThreadRng, seq::SliceRandom,
    thread_rng,
};
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

/// # SimpleByteSet Provider
///
/// Generates simple transactions that write a small set of bytes to an account.
/// This is useful for basic throughput testing.
pub struct SimpleByteSetProvider {
    accounts: Vec<Pubkey>,
}

/// # HighCuCost Provider
///
/// Generates transactions with a high computational cost to stress the validator's
/// processing capabilities.
pub struct HighCuCostProvider {
    accounts: Vec<Pubkey>,
    iters: u32,
}

/// # ReadWrite Provider
///
/// Generates transactions that perform read and write operations across multiple accounts,
/// which is useful for testing lock contention.
pub struct ReadWriteProvider {
    accounts: Vec<Pubkey>,
    rng: ThreadRng,
}

/// # ReadOnly Provider
///
/// Generates read-only transactions to measure parallel processing performance.
pub struct ReadOnlyProvider {
    accounts: Vec<Pubkey>,
    rng: ThreadRng,
    count: usize,
}

/// # Commit Provider
///
/// Generates transactions that commit the state to the base chain in the Ephemeral Rollup.
pub struct CommitProvider {
    accounts: Vec<Pubkey>,
    rng: ThreadRng,
    count: usize,
    payer: Pubkey,
}

/// # Mixed Provider
///
/// A transaction provider that combines multiple transaction providers to generate a mixed workload.
/// The distribution of transactions is determined by the weights assigned to each provider.
pub struct MixedProvider {
    providers: Vec<Box<dyn TransactionProvider>>,
    rng: ThreadRng,
    distribution: WeightedIndex<u8>,
    last_name: &'static str,
}

impl TransactionProvider for SimpleByteSetProvider {
    fn name(&self) -> &'static str {
        "SimpleByteSet"
    }
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction {
        let ix = Instruction::SimpleByteSet { id };
        // The PDA is selected based on the request ID, which ensures that the
        // transactions are distributed across all available accounts.
        let pda = self.accounts[id as usize % self.accounts.len()];
        let accounts = vec![AccountMeta::new(pda, false)];
        self.wrap_ix(ix, accounts)
    }
    fn accounts(&self) -> Vec<Pubkey> {
        self.accounts.clone()
    }
}

impl TransactionProvider for HighCuCostProvider {
    fn name(&self) -> &'static str {
        "HighCuCost"
    }
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction {
        let init = self.accounts[id as usize % self.accounts.len()];
        let ix = Instruction::ExpensiveHashCompute {
            id,
            init,
            iters: self.iters,
        };
        let accounts = vec![AccountMeta::new(init, false)];
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
        // Randomly selects two accounts for the read-write operation.
        let mut accounts = self.accounts.choose_multiple(&mut self.rng, 2).copied();
        let ix = Instruction::AccountDataCopy { id };
        let ro = AccountMeta::new_readonly(accounts.next().unwrap(), false);
        let rw = AccountMeta::new(accounts.next().unwrap(), false);
        self.wrap_ix(ix, vec![ro, rw])
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
        // Randomly selects a set of accounts for the read-only operation.
        let accounts = self
            .accounts
            .choose_multiple(&mut self.rng, self.count)
            .copied();
        let ix = Instruction::ReadAccountsData { id };
        let accounts = accounts
            .map(|acc| AccountMeta::new_readonly(acc, false))
            .collect();
        self.wrap_ix(ix, accounts)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        self.accounts.clone()
    }
}

impl TransactionProvider for CommitProvider {
    fn name(&self) -> &'static str {
        "Commit"
    }
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction {
        let ix = Instruction::CommitAccounts { id };
        let mut accounts = vec![
            AccountMeta::new(self.payer, true),
            AccountMeta::new(MAGIC_CONTEXT_ID, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false),
        ];
        // Randomly selects a set of accounts to be committed.
        accounts.extend(
            self.accounts
                .choose_multiple(&mut self.rng, self.count)
                .copied()
                .map(|acc| AccountMeta::new_readonly(acc, false)),
        );
        self.wrap_ix(ix, accounts)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        self.accounts.clone()
    }
}

impl TransactionProvider for MixedProvider {
    fn name(&self) -> &'static str {
        self.last_name
    }
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction {
        // Selects a provider based on the weighted distribution.
        let index = self.distribution.sample(&mut self.rng);
        let generator = &mut self.providers[index];
        self.last_name = generator.name();
        generator.generate_ix(id)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        self.providers
            .first()
            .map(|tp| tp.accounts())
            .unwrap_or_default()
    }
}

/// # Make Provider
///
/// A factory function that creates a transaction provider based on the provided benchmark mode.
pub fn make_provider(
    mode: &BenchMode,
    base: Pubkey,
    accounts: Vec<Pubkey>,
) -> Box<dyn TransactionProvider> {
    match mode {
        BenchMode::Mixed(modes) => {
            let providers = modes
                .iter()
                .map(|m| make_provider(&m.mode, base, accounts.clone()))
                .collect::<Vec<_>>();
            let weights = modes.iter().map(|m| m.weight).collect::<Vec<_>>();
            let distribution = WeightedIndex::new(weights).unwrap();
            let rng = thread_rng();
            Box::new(MixedProvider {
                providers,
                distribution,
                rng,
                last_name: "Mixed",
            })
        }
        BenchMode::SimpleByteSet => Box::new(SimpleByteSetProvider { accounts }),
        BenchMode::ReadWrite => Box::new(ReadWriteProvider {
            accounts,
            rng: thread_rng(),
        }),
        BenchMode::HighCuCost { iters } => Box::new(HighCuCostProvider {
            accounts,
            iters: *iters,
        }),
        BenchMode::ReadOnly {
            accounts_per_transaction,
        } => Box::new(ReadOnlyProvider {
            accounts,
            count: *accounts_per_transaction as usize,
            rng: thread_rng(),
        }),
        BenchMode::Commit {
            accounts_per_transaction,
        } => Box::new(CommitProvider {
            accounts,
            count: *accounts_per_transaction as usize,
            rng: thread_rng(),
            payer: base,
        }),
        // This function is only for transaction-based modes, so it will panic
        // if an RPC-based mode is provided.
        _ => panic!("Unsupported mode for make_provider"),
    }
}
