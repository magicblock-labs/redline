//! # Redline Program Processors
//!
//! ## Account Ownership Model
//!
//! All accounts managed by this program follow a consistent ownership model:
//! - The first 32 bytes store the **owner pubkey** (the account that pays rent)
//! - Only the owner can **delegate** or **close** the account
//! - Benchmark operations (write/compute) preserve this reserved space
//!
//! ## Account Layout
//! ```text
//! [0..32]   - Owner pubkey (reserved, set on init, never modified)
//! [32..]    - User data (available for benchmark operations)
//! ```
//!
//! ## Ownership Verification
//! Operations that modify account lifecycle (delegate/close) verify that:
//! 1. The transaction is signed by the owner
//! 2. The signer's pubkey matches the stored owner pubkey

use pubkey::Pubkey;
use sdk::{
    cpi::{DelegateAccounts, DelegateConfig},
    utils::create_pda,
};
use sha2::{Digest, Sha256};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
};

/// Size of the owner pubkey stored at the start of each account (in bytes).
const OWNER_PUBKEY_SIZE: usize = 32;

/// # Prepare Buffer
///
/// A helper function to copy data into a buffer at a specified index.
fn prepare_buffer(index: &mut usize, target: &mut [u8], data: &[u8]) {
    target[*index..*index + data.len()].copy_from_slice(data);
    *index += data.len();
}

/// # Get Data Offset
///
/// Returns the starting offset for writing user data (after the owner pubkey).
const fn data_offset() -> usize {
    OWNER_PUBKEY_SIZE
}

/// # Verify Account Owner
///
/// Verifies that the signer matches the account owner stored in the first 32 bytes.
/// Returns the owner pubkey if valid.
fn verify_account_owner(
    signer: &AccountInfo,
    account: &AccountInfo,
) -> Result<Pubkey, ProgramError> {
    // Verify the signer is actually signing
    if !signer.is_signer {
        msg!("Error: Account owner must be a signer");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Read the stored owner pubkey from the first 32 bytes
    let data = account.try_borrow_data()?;
    if data.len() < OWNER_PUBKEY_SIZE {
        msg!("Error: Account data too small to contain owner pubkey");
        return Err(ProgramError::InvalidAccountData);
    }

    let stored_owner = Pubkey::try_from(&data[..OWNER_PUBKEY_SIZE])
        .map_err(|_| ProgramError::InvalidAccountData)?;

    // Verify the signer matches the stored owner
    if stored_owner != *signer.key {
        msg!(
            "Error: Signer {} does not match stored owner {}",
            signer.key,
            stored_owner
        );
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(stored_owner)
}

/// # Initialize Account
///
/// Initializes a new Program Derived Address (PDA) with the specified space and seeds.
/// The payer becomes the account owner, with their pubkey stored in the first 32 bytes.
pub fn init_account(
    iter: &mut std::slice::Iter<AccountInfo>,
    space: u32,
    seed: u8,
    bump: u8,
    authority: Pubkey,
) -> ProgramResult {
    let payer = next_account_info(iter)?;
    let pda = next_account_info(iter)?;
    let base = next_account_info(iter)?;
    let mut seeds = space.to_le_bytes().to_vec();
    seeds.push(seed);
    seeds.extend_from_slice(&authority.as_ref()[..16]);
    let seeds = [base.key.as_ref(), &seeds, &[bump]];

    create_pda(
        pda,
        &crate::ID,
        space as usize,
        &[&seeds],
        next_account_info(iter)?,
        payer,
        true,
    )?;

    // Store the owner pubkey in the first bytes for ownership tracking
    let mut data = pda.try_borrow_mut_data()?;
    data[..OWNER_PUBKEY_SIZE].copy_from_slice(payer.key.as_ref());

    msg!("initialized PDA: {} with owner: {}", pda.key, payer.key);
    Ok(())
}

/// # Delegate Account
///
/// Delegates a PDA to the Ephemeral Rollup (ER) program.
/// Only the account owner can delegate.
pub fn delegate_account(accs: &[AccountInfo], seed: u8, authority: Pubkey) -> ProgramResult {
    let owner = accs
        .first()
        .ok_or_else(|| ProgramError::NotEnoughAccountKeys)?;

    let accounts = DelegateAccounts::try_from(accs)?;

    // Verify ownership before delegating
    verify_account_owner(owner, accounts.pda)?;

    let base = accs
        .last()
        .ok_or_else(|| ProgramError::NotEnoughAccountKeys)?;
    let mut seeds = (accounts.pda.data_len() as u32).to_le_bytes().to_vec();
    seeds.push(seed);
    seeds.extend_from_slice(&authority.as_ref()[..16]);
    let seeds = [base.key.as_ref(), &seeds];
    let pda = *accounts.pda.key;
    let config = DelegateConfig {
        commit_frequency_ms: u32::MAX,
        validator: Some(authority),
    };
    sdk::cpi::delegate_account(accounts, &seeds, config)?;
    msg!("delegated PDA: {} to {}", pda, authority);
    Ok(())
}

/// # Simple Byte Set
///
/// Fills multiple accounts' data with a simple byte pattern.
/// Preserves the owner pubkey stored at the beginning.
pub fn simple_byte_set(iter: &mut std::slice::Iter<AccountInfo>, id: u64) -> ProgramResult {
    let mut count = 0;
    let mut total_bytes = 0;
    let offset = data_offset();

    while let Ok(pda) = next_account_info(iter) {
        if pda.lamports() == 0 {
            continue; // Skip uninitialized accounts
        }
        let mut data = pda.try_borrow_mut_data()?;
        let buffer = id.to_le_bytes();
        let mut index = offset;

        while index + buffer.len() <= data.len() {
            prepare_buffer(&mut index, &mut data, &buffer);
        }

        total_bytes += index - offset;
        count += 1;
    }

    msg!(
        "filled {} accounts with id {}, using {} total bytes",
        count,
        id,
        total_bytes
    );
    Ok(())
}

/// # Multi-Account Read
///
/// Reads the data from multiple accounts and writes the sum of their lengths to a PDA.
/// Preserves the owner pubkey stored at the beginning.
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
    let mut index = data_offset();

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
/// Performs a computationally expensive hash calculation and writes results to multiple accounts.
/// Preserves the owner pubkey stored at the beginning.
pub fn expensive_hash_compute(
    iter: &mut std::slice::Iter<AccountInfo>,
    id: u64,
    mut hash: [u8; 32],
    iters: u32,
) -> ProgramResult {
    msg!("Starting compute-intensive operation...");

    // Perform computation once
    for i in 0..iters {
        let mut hasher = Sha256::new();
        hasher.update(hash);
        hasher.update(i.to_le_bytes());
        hash.copy_from_slice(&hasher.finalize());
    }

    // Write result to all accounts
    let mut count = 0;
    let offset = data_offset();
    while let Ok(pda) = next_account_info(iter) {
        if pda.lamports() == 0 {
            continue; // Skip uninitialized accounts
        }
        let mut data = pda.try_borrow_mut_data()?;
        let buffer = id.to_le_bytes();
        let mut index = offset;

        prepare_buffer(&mut index, &mut data, &buffer);
        prepare_buffer(&mut index, &mut data, &hash);
        count += 1;
    }

    msg!(
        "computed SHA-256 hash {} times, wrote to {} accounts, txn: {}",
        iters,
        count,
        id
    );
    Ok(())
}

/// # Account Data Copy
///
/// Copies data from multiple source accounts to multiple destination accounts.
/// Preserves the owner pubkey stored at the beginning of each account.
pub fn account_data_copy(iter: &mut std::slice::Iter<AccountInfo>, id: u64) -> ProgramResult {
    let accounts: Vec<_> = iter.collect();
    if accounts.is_empty() {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    // Split accounts: first half are sources (read-only), second half are destinations (writable)
    let split = accounts.len() / 2;
    let split = if split == 0 { 1 } else { split };

    let sources = &accounts[..split];
    let destinations = &accounts[split..];
    let offset = data_offset();

    // Copy data from sources to destinations
    let mut total_bytes = 0;
    for (dest_idx, dest) in destinations.iter().enumerate() {
        if dest.lamports() == 0 {
            continue;
        }

        let src = sources[dest_idx % sources.len()];
        if src.lamports() == 0 {
            continue;
        }

        let src_data = src.try_borrow_data()?;
        let mut dst_data = dest.try_borrow_mut_data()?;

        let buffer = id.to_le_bytes();
        let mut index = offset;
        prepare_buffer(&mut index, &mut dst_data, &buffer);

        // Copy user data from source to destination
        let copy_len = (src_data.len() - offset).min(dst_data.len() - index);
        if copy_len > 0 {
            dst_data[index..index + copy_len].copy_from_slice(&src_data[offset..offset + copy_len]);
            total_bytes += copy_len;
        }
    }

    msg!(
        "copied data from {} sources to {} destinations, {} total bytes, txn: {}",
        sources.len(),
        destinations.len(),
        total_bytes,
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

/// # Close Account
///
/// Closes an account and refunds the rent to the owner.
/// Only the account owner can close it.
pub fn close_account(iter: &mut std::slice::Iter<AccountInfo>) -> ProgramResult {
    let owner = next_account_info(iter)?;
    let account_to_close = next_account_info(iter)?;

    // Verify ownership before closing
    verify_account_owner(owner, account_to_close)?;

    // Transfer all lamports to the owner (closing the account)
    let dest_starting_lamports = owner.lamports();
    **owner.lamports.borrow_mut() = dest_starting_lamports
        .checked_add(account_to_close.lamports())
        .ok_or(ProgramError::ArithmeticOverflow)?;
    **account_to_close.lamports.borrow_mut() = 0;

    msg!(
        "closed account {} and refunded rent to {}",
        account_to_close.key,
        owner.key
    );
    Ok(())
}
