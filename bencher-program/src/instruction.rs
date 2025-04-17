use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

/// Instructions to simulate some activity
#[derive(BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    /// Initialize writable account
    InitAccount {
        space: u32,
        bump: u8,
    },
    InitClonable {
        space: u32,
        seed: u8,
        bump: u8,
    },
    /// Delegate an account
    Delegate,
    /// Fill all the bytes in the account data with the given value
    SimpleByteSet {
        value: u64,
    },
    ExpensiveHashCompute {
        init: Pubkey,
    },
    /// Compute the sum of length of data of all argument accounts and
    /// write them to the data offset (of writable PDA) given at index
    /// this should trigger cloning of all the readonly provided accounts
    MultiAccountRead {
        index: u32,
    },
    CommitUndelegate,
    Close,
}
