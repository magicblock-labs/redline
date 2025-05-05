#![allow(unexpected_cfgs)]

use solana_program::{
    account_info::AccountInfo,
    declare_id,
    entrypoint::{self, ProgramResult},
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
};

use instruction::Instruction;
use processors::*;

entrypoint::entrypoint!(process_instruction);
declare_id!("3JnJ727jWEmPVU8qfXwtH63sCNDX7nMgsLbg8qy8aaPX");

pub const DELEGATION_PROGRAM_ID: Pubkey = sdk::id();

pub use sdk::delegate_args::DelegateAccountMetas;
pub use sdk::delegate_args::DelegateAccounts;

pub const SEEDS: &[u8] = b"bencher-pda";

fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction: Instruction = bincode::deserialize(instruction_data).map_err(|err| {
        msg!("failed to bincode deserialize instruction data: {}", err);
        ProgramError::InvalidInstructionData
    })?;
    let mut iter = accounts.iter();

    match instruction {
        Instruction::InitAccount { space, seed, bump } => {
            init_account(&mut iter, space, seed, bump)?;
        }
        Instruction::Delegate { seed } => {
            delegate_account(accounts, seed)?;
        }
        Instruction::SimpleByteSet { id } => {
            simple_byte_set(&mut iter, id)?;
        }
        Instruction::MultiAccountRead { id } => {
            multi_account_read(&mut iter, accounts, id)?;
        }
        Instruction::ExpensiveHashCompute { id, init, iters } => {
            expensive_hash_compute(&mut iter, id, init.to_bytes(), iters)?;
        }
        Instruction::AccountDataCopy { id } => {
            account_data_copy(&mut iter, id)?;
        }
        Instruction::ReadAccountsData { id } => read_accounts_data(&mut iter, id)?,
    }

    Ok(())
}

pub mod instruction;
mod processors;
pub mod utils;
