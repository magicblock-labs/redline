use sdk::{cpi::DelegateAccounts, utils::create_pda};
use sha2::{Digest, Sha256};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
};

use crate::SEEDS;

/// # Prepare Buffer
///
/// A helper function to copy data into a buffer at a specified index.
fn prepare_buffer(index: &mut usize, target: &mut [u8], data: &[u8]) {
    target[*index..*index + data.len()].copy_from_slice(data);
    *index += data.len();
}

/// # Initialize Account
///
/// Initializes a new Program Derived Address (PDA) with the specified space and seeds.
pub fn init_account(
    iter: &mut std::slice::Iter<AccountInfo>,
    space: u32,
    seed: u8,
    bump: u8,
) -> ProgramResult {
    let payer = next_account_info(iter)?;
    let pda = next_account_info(iter)?;
    let mut extra_seeds = space.to_le_bytes().to_vec();
    extra_seeds.push(seed);
    create_pda(
        pda,
        &crate::ID,
        space as usize,
        &[&[payer.key.as_ref(), SEEDS, &extra_seeds, &[bump]]],
        next_account_info(iter)?,
        payer,
    )?;
    msg!("initialized PDA: {}", pda.key);
    Ok(())
}

/// # Delegate Account
///
/// Delegates a PDA to the Ephemeral Rollup (ER) program.
pub fn delegate_account(accounts: &[AccountInfo], seed: u8) -> ProgramResult {
    let accounts = DelegateAccounts::try_from(accounts)?;
    let mut extra_seeds = (accounts.pda.data_len() as u32).to_le_bytes().to_vec();
    extra_seeds.push(seed);
    let seeds = [accounts.payer.key.as_ref(), SEEDS, &extra_seeds];
    let pda = *accounts.pda.key;
    sdk::cpi::delegate_account(accounts, &seeds, Default::default())?;
    msg!("delegated PDA: {}", pda);
    Ok(())
}

/// # Simple Byte Set
///
/// Fills an account's data with a simple byte pattern.
pub fn simple_byte_set(iter: &mut std::slice::Iter<AccountInfo>, id: u64) -> ProgramResult {
    let pda = next_account_info(iter)?;
    if pda.lamports() == 0 {
        return Err(ProgramError::UninitializedAccount);
    }
    let mut data = pda.try_borrow_mut_data()?;
    let buffer = id.to_le_bytes();
    let mut index = 0;

    while index + buffer.len() <= data.len() {
        prepare_buffer(&mut index, &mut data, &buffer);
    }

    msg!(
        "filled {} PDA data with {}, using {} bytes",
        pda.key,
        id,
        index
    );
    Ok(())
}

/// # Multi-Account Read
///
/// Reads the data from multiple accounts and writes the sum of their lengths to a PDA.
pub fn multi_account_read(
    iter: &mut std::slice::Iter<AccountInfo>,
    accounts: &[AccountInfo],
    id: u64,
) -> ProgramResult {
    let pda = next_account_info(iter)?;
    if pda.lamports() == 0 {
        return Err(ProgramError::UninitializedAccount);
    }
    let mut data = pda.try_borrow_mut_data()?;
    let sum = iter.clone().map(|a| a.data_len() as u64).sum::<u64>();
    let buffer = id.to_le_bytes();
    let buffer_sum = sum.to_le_bytes();
    let mut index = 0;

    prepare_buffer(&mut index, &mut data, &buffer);
    prepare_buffer(&mut index, &mut data, &buffer_sum);

    msg!(
        "computed sum of {} accounts' data: {}, txn: {}",
        accounts.len() - 1,
        sum,
        id
    );
    Ok(())
}

/// # Expensive Hash Compute
///
/// Performs a computationally expensive hash calculation to stress the validator.
pub fn expensive_hash_compute(
    iter: &mut std::slice::Iter<AccountInfo>,
    id: u64,
    mut hash: [u8; 32],
    iters: u32,
) -> ProgramResult {
    let pda = next_account_info(iter)?;
    msg!("Starting compute-intensive operation...");

    for i in 0..iters {
        let mut hasher = Sha256::new();
        hasher.update(hash);
        hasher.update(i.to_le_bytes());
        hash.copy_from_slice(&hasher.finalize());
    }

    let mut data = pda.try_borrow_mut_data()?;
    let buffer = id.to_le_bytes();
    let mut index = 0;

    prepare_buffer(&mut index, &mut data, &buffer);
    prepare_buffer(&mut index, &mut data, &hash);

    msg!("computed SHA-256 hash {} times, txn: {}", iters, id);
    Ok(())
}

/// # Account Data Copy
///
/// Copies the data from one account to another.
pub fn account_data_copy(iter: &mut std::slice::Iter<AccountInfo>, id: u64) -> ProgramResult {
    let source = next_account_info(iter)?;
    let destination = next_account_info(iter)?;
    let src = source.try_borrow_data()?;
    let mut dst = destination.try_borrow_mut_data()?;

    let buffer = id.to_le_bytes();
    let mut index = 0;

    prepare_buffer(&mut index, &mut dst, &buffer);
    dst[index..].copy_from_slice(&src[index..]);

    msg!(
        "copied {} bytes from {} to {}, txn: {}",
        dst.len() - buffer.len(),
        source.key,
        destination.key,
        id
    );
    Ok(())
}

/// # Read Accounts Data
///
/// Reads the data from a list of accounts and logs their sizes.
pub fn read_accounts_data(iter: &mut std::slice::Iter<AccountInfo>, id: u64) -> ProgramResult {
    while let Ok(account) = next_account_info(iter) {
        msg!(
            "account {} has {} space, txn: {}",
            account.key,
            account.data_len(),
            id
        );
    }
    Ok(())
}

/// # Commit Accounts
///
/// Commits a list of accounts to the base chain.
pub fn commit_accounts(iter: &mut std::slice::Iter<AccountInfo>, id: u64) -> ProgramResult {
    let payer = next_account_info(iter)?;
    let magic_context = next_account_info(iter)?;
    let magic_program = next_account_info(iter)?;
    let accounts: Vec<_> = iter.collect();
    let count = accounts.len();
    sdk::ephem::commit_accounts(payer, accounts, magic_context, magic_program)?;
    msg!("committed {} accounts to chain txn: {}", count, id);
    Ok(())
}
