use serde::{Deserialize, Serialize};
use solana_program::pubkey::Pubkey;

/// # Instructions
///
/// Defines the set of instructions that can be sent to the Redline program. Each variant
/// corresponds to a specific action that the program can perform, from initializing accounts
/// to executing complex, computationally expensive operations.
#[derive(Serialize, Deserialize)]
pub enum Instruction {
    /// ## Commit Accounts
    ///
    /// Commits a list of accounts to the base chain, effectively finalizing their state.
    CommitAccounts { id: u64 },
    /// ## Initialize Account
    ///
    /// Initializes a new Program Derived Address (PDA) with a specified size and seed.
    InitAccount { space: u32, seed: u8, bump: u8 },
    /// ## Delegate Account
    ///
    /// Delegates a PDA to the Ephemeral Rollup (ER) program, allowing it to be used
    /// in ER-specific operations.
    Delegate { seed: u8 },
    /// ## Simple Byte Set
    ///
    /// Fills an account's data with a simple, repeating byte pattern. This is useful for
    /// basic throughput testing.
    SimpleByteSet { id: u64 },
    /// ## Expensive Hash Compute
    ///
    /// Performs a computationally expensive hash calculation to simulate high-compute-cost
    /// transactions and stress the validator's processing capabilities.
    ExpensiveHashCompute { id: u64, init: Pubkey, iters: u32 },
    /// ## Multi-Account Read
    ///
    /// Reads the data from multiple accounts and writes the sum of their lengths to a PDA.
    /// This is useful for testing read-only account handling and cloning.
    MultiAccountRead { id: u64 },
    /// ## Account Data Copy
    ///
    /// Copies the data from one account to another, simulating read-write operations and
    /// testing for lock contention.
    AccountDataCopy { id: u64 },
    /// ## Read Accounts Data
    ///
    /// Reads the data from a list of accounts and logs their sizes, useful for testing
    /// read-only account performance.
    ReadAccountsData { id: u64 },
}
