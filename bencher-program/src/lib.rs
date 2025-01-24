use core::mem::size_of;

use borsh::BorshDeserialize;
use instruction::Instruction;
use sdk::{
    consts::EXTERNAL_UNDELEGATE_DISCRIMINATOR,
    cpi::{undelegate_account, DelegateAccounts, UndelegateAccounts},
    ephem,
    utils::{close_pda, create_pda},
};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    declare_id,
    entrypoint::{entrypoint, ProgramResult},
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
};

entrypoint!(process_instruction);
declare_id!("3JnJ727jWEmPVU8qfXwtH63sCNDX7nMgsLbg8qy8aaPX");

pub const SEEDS: &[u8] = b"bencher-pda";

fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.len() > EXTERNAL_UNDELEGATE_DISCRIMINATOR.len() {
        let (disc, seeds_data) = instruction_data.split_at(EXTERNAL_UNDELEGATE_DISCRIMINATOR.len());
        if disc == EXTERNAL_UNDELEGATE_DISCRIMINATOR {
            let seeds = <Vec<Vec<u8>>>::try_from_slice(seeds_data)?;
            let accounts = UndelegateAccounts::try_from_accounts(accounts, program_id)?;
            return undelegate_account(accounts, seeds);
        }
    }
    let instruction = Instruction::try_from_slice(instruction_data)?;
    let mut iter = accounts.iter();
    match instruction {
        Instruction::InitAccount { space, bump } => {
            let payer = next_account_info(&mut iter)?;
            let pda = next_account_info(&mut iter)?;
            create_pda(
                pda,
                &crate::ID,
                space as usize,
                &[&[payer.key.as_ref(), SEEDS, &[bump]]],
                next_account_info(&mut iter)?,
                payer,
            )?;
            msg!("initialized PDA: {}", pda.key)
        }
        Instruction::InitClonable { space, seed, bump } => {
            let payer = next_account_info(&mut iter)?;
            let pda = next_account_info(&mut iter)?;
            create_pda(
                pda,
                &crate::ID,
                space as usize,
                &[&[payer.key.as_ref(), SEEDS, &[seed], &[bump]]],
                next_account_info(&mut iter)?,
                payer,
            )?;
            msg!("initialized clonable PDA: {}", pda.key)
        }
        Instruction::Delegate => {
            let accounts = DelegateAccounts::try_from(accounts)?;
            let seeds = [accounts.payer.key.as_ref(), SEEDS];
            let pda = *accounts.pda.key;
            sdk::cpi::delegate_account(accounts, &seeds, Default::default())?;
            msg!("delegated PDA: {}", pda)
        }
        Instruction::FillSpace { value } => {
            let pda = next_account_info(&mut iter)?;
            if pda.lamports() == 0 {
                Err(ProgramError::UninitializedAccount)?;
            }
            let mut data = pda.try_borrow_mut_data()?;
            let len = data.len() - 1;
            data[..len].fill(value as u8);
            // just playing around, nothing meaningful really
            data[len] = data[len].wrapping_add(1);
            msg!("filled {} PDA data with {}", pda.key, value)
        }
        Instruction::ComputeSum { index } => {
            let pda = next_account_info(&mut iter)?;
            if pda.lamports() == 0 {
                Err(ProgramError::UninitializedAccount)?;
            }
            let mut data = pda.try_borrow_mut_data()?;
            let align = size_of::<u64>();
            let index = (index as usize % data.len()) / align * align;
            let sum = iter.map(|a| a.data_len() as u64).sum::<u64>();
            data[index..index + align].copy_from_slice(&sum.to_le_bytes());
            msg!(
                "computed sum of {} accounts' data: {}",
                accounts.len() - 1,
                sum
            )
        }
        Instruction::CommitUndelegate => {
            let payer = next_account_info(&mut iter)?;
            let pda = vec![next_account_info(&mut iter)?];
            let magic_context = next_account_info(&mut iter)?;
            let magic_program = next_account_info(&mut iter)?;
            ephem::commit_and_undelegate_accounts(payer, pda, magic_context, magic_program)?;
        }
        Instruction::Close => {
            let payer = next_account_info(&mut iter)?;
            let pda = next_account_info(&mut iter)?;
            close_pda(pda, payer)?;
            msg!("closed pda {}", pda.key);
        }
    }

    Ok(())
}

pub mod instruction;
