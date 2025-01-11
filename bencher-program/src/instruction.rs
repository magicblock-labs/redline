use borsh::{BorshDeserialize, BorshSerialize};

/// Instructions to simulate some activity
#[derive(BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    /// Initialize writable account
    InitAccount {
        space: u32,
        bump: u8,
    },
    /// Delegate an account
    Delegate,
    /// Fill all the bytes in account data with given value
    FillSpace {
        value: u8,
    },
    /// Compute the sum of length of data of all argument accounts and
    /// write them to the data offset (of writable PDA) given at index
    /// this should trigger cloning of all the readonly provided accounts
    ComputeSum {
        index: u32,
    },
    CommitUndelegate,
    Close,
}
