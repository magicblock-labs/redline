use core::types::BenchMode;
use hash::Hash;
use instruction::{AccountMeta, Instruction as SolanaInstruction};
use keypair::Keypair;
use program::{instruction::Instruction, utils::derive_pda};
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
/// A trait for generating Solana transactions for different benchmark modes.
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
pub struct SimpleByteSetProvider {
    accounts: Vec<Pubkey>,
}

/// # HighCuCost Provider
///
/// Generates transactions with a high computational cost.
pub struct HighCuCostProvider {
    accounts: Vec<Pubkey>,
    iters: u32,
}

/// # TriggerClones Provider
///
/// Generates transactions that trigger account cloning.
pub struct TriggerClonesProvider {
    ro_accounts: Vec<Pubkey>,
    pda: Pubkey,
}

/// # ReadWrite Provider
///
/// Generates transactions that perform read and write operations across multiple accounts.
pub struct ReadWriteProvider {
    accounts: Vec<Pubkey>,
    rng: ThreadRng,
}

/// # ReadOnly Provider
///
/// Generates read-only transactions to test for parallel processing performance.
pub struct ReadOnlyProvider {
    accounts: Vec<Pubkey>,
    rng: ThreadRng,
    count: usize,
}

/// # Commit Provider
///
/// Generates transactions that commit the state to the base chain.
pub struct CommitProvider {
    accounts: Vec<Pubkey>,
    rng: ThreadRng,
    count: usize,
    payer: Pubkey,
}

/// # Mixed Provider
///
/// A transaction provider that combines multiple transaction providers to generate a mixed workload.
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

impl TransactionProvider for TriggerClonesProvider {
    fn name(&self) -> &'static str {
        "TriggerClones"
    }
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction {
        let ix = Instruction::MultiAccountRead { id };
        let mut accounts = vec![AccountMeta::new(self.pda, false)];
        accounts.extend(
            self.ro_accounts
                .iter()
                .map(|&a| AccountMeta::new_readonly(a, false)),
        );
        self.wrap_ix(ix, accounts)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        let mut accounts = self.ro_accounts.clone();
        accounts.push(self.pda);
        accounts
    }
}

impl TransactionProvider for ReadOnlyProvider {
    fn name(&self) -> &'static str {
        "ReadOnly"
    }
    fn generate_ix(&mut self, id: u64) -> SolanaInstruction {
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
    space: u32,
    accounts: Vec<Pubkey>,
) -> Box<dyn TransactionProvider> {
    match mode {
        BenchMode::Mixed(modes) => {
            let providers = modes
                .iter()
                .map(|m| make_provider(&m.mode, base, space, accounts.clone()))
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
        BenchMode::TriggerClones { .. } => Box::new(TriggerClonesProvider {
            pda: derive_pda(base, space, 1).0,
            ro_accounts: accounts,
        }),
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
        // This function is only for transaction-based modes
        _ => panic!("Unsupported mode for make_provider"),
    }
}
