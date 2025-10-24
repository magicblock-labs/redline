use redline::utils::derive_pda;
use redline::{instruction::Instruction, DELEGATION_PROGRAM_ID};
use sdk::delegate_args::{DelegateAccountMetas, DelegateAccounts};
use solana_program_test::*;
#[allow(deprecated)]
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction as SolanaInstruction},
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::Transaction,
    transport::TransportError,
};

#[tokio::test]
async fn test_combined_operations() -> Result<(), TransportError> {
    let (client, payer) = setup().await;
    let mut account_keys = Vec::new();
    let id = 1_u64;

    for seed in 0..17 {
        let (key, bump) = derive_pda(payer.pubkey(), 128, seed, Pubkey::new_unique());

        process_transaction(
            &client,
            &[init_account_ix(&payer, &key, 128, seed, bump)],
            &payer,
        )
        .await?;

        if seed == 16 {
            process_transaction(&client, &[delegate_account_ix(&payer, &key, seed)], &payer)
                .await?;
        }

        account_keys.push(key);
    }

    process_transaction(&client, &[simple_byte_set_ix(&account_keys[0], id)], &payer).await?;
    process_transaction(
        &client,
        &[multi_account_read_ix(
            &account_keys[0],
            &account_keys[1..],
            id,
        )],
        &payer,
    )
    .await?;
    process_transaction(
        &client,
        &[expensive_hash_compute_ix(&account_keys[0], id, 28)],
        &payer,
    )
    .await?;
    process_transaction(
        &client,
        &[account_data_copy_ix(&account_keys[0], &account_keys[1], id)],
        &payer,
    )
    .await?;

    Ok(())
}

async fn setup() -> (BanksClient, Keypair) {
    let mut program_test = ProgramTest::default();
    std::env::set_var("SBF_OUT_DIR", "fixtures");

    program_test.add_program("dlp", DELEGATION_PROGRAM_ID, None);
    program_test.add_program("redline", redline::id(), None);

    let payer = Keypair::new();
    program_test.add_account(
        payer.pubkey(),
        Account::new(LAMPORTS_PER_SOL, 0, &system_program::ID),
    );

    let (test_client, ..) = program_test.start().await;
    (test_client, payer)
}

async fn process_transaction(
    client: &BanksClient,
    instructions: &[SolanaInstruction],
    payer: &Keypair,
) -> Result<(), TransportError> {
    let latest_blockhash = client.get_latest_blockhash().await?;
    let transaction = Transaction::new_signed_with_payer(
        instructions,
        Some(&payer.pubkey()),
        &[payer],
        latest_blockhash,
    );
    client
        .process_transaction(transaction)
        .await
        .map_err(Into::into)
}

fn init_account_ix(
    payer: &Keypair,
    key: &Pubkey,
    space: u32,
    seed: u8,
    bump: u8,
) -> SolanaInstruction {
    let mut instruction = create_instruction(
        key,
        Instruction::InitAccount {
            space,
            seed,
            bump,
            authority: payer.pubkey(),
        },
    );
    instruction
        .accounts
        .insert(0, AccountMeta::new(payer.pubkey(), true));
    instruction
        .accounts
        .push(AccountMeta::new_readonly(system_program::id(), false));
    instruction
}

fn delegate_account_ix(payer: &Keypair, key: &Pubkey, seed: u8) -> SolanaInstruction {
    let mut instruction = create_instruction(
        key,
        Instruction::Delegate {
            seed,
            authority: payer.pubkey(),
        },
    );

    let accounts = DelegateAccounts::new(*key, redline::ID);
    instruction.accounts = DelegateAccountMetas::from(accounts).into_vec(payer.pubkey());

    instruction
}

fn simple_byte_set_ix(key: &Pubkey, id: u64) -> SolanaInstruction {
    create_instruction(key, Instruction::SimpleByteSet { id })
}

fn multi_account_read_ix(key: &Pubkey, other_keys: &[Pubkey], id: u64) -> SolanaInstruction {
    let mut instruction = create_instruction(key, Instruction::MultiAccountRead { id });

    for &other_key in other_keys {
        instruction
            .accounts
            .push(AccountMeta::new_readonly(other_key, false));
    }

    instruction
}

fn expensive_hash_compute_ix(key: &Pubkey, id: u64, iterations: u32) -> SolanaInstruction {
    let init_key = Pubkey::default();
    create_instruction(
        key,
        Instruction::ExpensiveHashCompute {
            id,
            init: init_key,
            iters: iterations,
        },
    )
}

fn account_data_copy_ix(source: &Pubkey, destination: &Pubkey, id: u64) -> SolanaInstruction {
    create_instruction_with_two_accounts(source, destination, Instruction::AccountDataCopy { id })
}

fn create_instruction(key: &Pubkey, data: Instruction) -> SolanaInstruction {
    SolanaInstruction {
        program_id: redline::id(),
        accounts: vec![AccountMeta::new(*key, false)],
        data: bincode::serialize(&data).unwrap(),
    }
}

fn create_instruction_with_two_accounts(
    key1: &Pubkey,
    key2: &Pubkey,
    data: Instruction,
) -> SolanaInstruction {
    SolanaInstruction {
        program_id: redline::id(),
        accounts: vec![
            AccountMeta::new_readonly(*key1, false),
            AccountMeta::new(*key2, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: bincode::serialize(&data).unwrap(),
    }
}
