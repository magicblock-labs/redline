use serde::{Deserialize, Serialize};
use solana_program::pubkey::Pubkey;

/// Instructions to simulate some activity
#[derive(Serialize, Deserialize)]
pub enum Instruction {
    /// Initialize writable account
    InitAccount {
        space: u32,
        seed: u8,
        bump: u8,
    },
    /// Delegate an account
    Delegate {
        seed: u8,
    },
    /// Fill all the bytes in the account data with the given value
    SimpleByteSet {
        id: u64,
    },
    ExpensiveHashCompute {
        id: u64,
        init: Pubkey,
        iters: u32,
    },
    /// Compute the sum of length of data of all argument accounts and
    /// write them to the data offset (of writable PDA) given at index
    /// this should trigger cloning of all the readonly provided accounts
    MultiAccountRead {
        id: u64,
    },
    AccountDataCopy {
        id: u64,
    },
}
