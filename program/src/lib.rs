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

/// # Process Instruction
///
/// The main entry point for the program, responsible for processing all incoming instructions.
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
        Instruction::InitAccount {
            space,
            seed,
            bump,
            authority,
        } => {
            init_account(&mut iter, space, seed, bump, authority)?;
        }
        Instruction::Delegate { seed, authority } => {
            delegate_account(accounts, seed, authority)?;
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
        Instruction::CommitAccounts { id } => {
            commit_accounts(&mut iter, id)?;
        }
        Instruction::CloseAccount => {
            close_account(&mut iter)?;
        }
    }

    Ok(())
}

pub mod instruction;
mod processors;
pub mod utils;
