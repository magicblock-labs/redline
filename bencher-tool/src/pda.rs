use std::rc::Rc;

use benchprog::{instruction::Instruction, SEEDS};
use sdk::{
    consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID},
    delegate_args::{DelegateAccountMetas, DelegateAccounts},
};
use solana::{
    instruction::{AccountMeta, Instruction as SolanaInstruction},
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    system_program,
    transaction::Transaction,
};
use url::Url;

use crate::http::{SolanaClient, TxnRequester};

pub struct Pda {
    pub payer: Keypair,
    pub pubkey: Pubkey,
    pub bump: u8,
    pub sub: u64,
}

impl Pda {
    pub async fn new(url: Url, client: &SolanaClient, payer: Keypair) -> Self {
        let pubkey = payer.pubkey();
        if client.info(url.clone(), &pubkey).await.lamports == 0 {
            client.airdrop(url, &pubkey).await;
        }
        let (pubkey, bump) =
            Pubkey::find_program_address(&[pubkey.as_ref(), SEEDS], &benchprog::ID);
        Self {
            payer: payer.into(),
            pubkey,
            bump,
            sub: 0,
        }
    }

    pub async fn init(&self, tx: Rc<TxnRequester>, space: u32) {
        let pk = self.payer.pubkey();
        let metas = vec![
            AccountMeta::new(pk, true),
            AccountMeta::new(self.pubkey, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ];
        let ix = Instruction::InitAccount {
            space,
            bump: self.bump,
        };
        let ix = SolanaInstruction::new_with_borsh(benchprog::ID, &ix, metas);

        let mut txn = Transaction::new_with_payer(&[ix], Some(&pk));
        let hash = *tx.hash.borrow();

        txn.sign(&[&self.payer], hash);
        tx.send(txn, 1).await;
    }

    pub async fn close(&self, tx: Rc<TxnRequester>) {
        let pk = self.payer.pubkey();
        let metas = vec![
            AccountMeta::new(pk, true),
            AccountMeta::new(self.pubkey, false),
        ];
        let ix = Instruction::Close;
        let ix = SolanaInstruction::new_with_borsh(benchprog::ID, &ix, metas);
        let mut txn = Transaction::new_with_payer(&[ix], Some(&pk));
        let hash = *tx.hash.borrow();

        txn.sign(&[&self.payer], hash);
        tx.send(txn, 42).await;
    }

    pub async fn delegate(&self, tx: Rc<TxnRequester>) {
        let pk = self.payer.pubkey();
        let accounts = DelegateAccounts::new(self.pubkey, benchprog::ID);
        let metas = DelegateAccountMetas::from(accounts).into_vec(pk);
        let ix = Instruction::Delegate;
        let ix = SolanaInstruction::new_with_borsh(benchprog::ID, &ix, metas);
        let mut txn = Transaction::new_with_payer(&[ix], Some(&pk));
        let hash = *tx.hash.borrow();

        txn.sign(&[&self.payer], hash);
        tx.send(txn, 42).await;
    }

    pub async fn undelegate(&self, tx: Rc<TxnRequester>) {
        let pk = self.payer.pubkey();
        let metas = vec![
            AccountMeta::new(pk, true),
            AccountMeta::new(self.pubkey, false),
            AccountMeta::new(MAGIC_CONTEXT_ID, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false),
        ];
        let ix = Instruction::CommitUndelegate;
        let ix = SolanaInstruction::new_with_borsh(benchprog::ID, &ix, metas);
        let mut txn = Transaction::new_with_payer(&[ix], Some(&pk));
        let hash = *tx.hash.borrow();

        txn.sign(&[&self.payer], hash);
        tx.send(txn, 42).await;
    }
}
