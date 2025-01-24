use std::{cell::RefCell, rc::Rc};

use benchprog::{instruction::Instruction, SEEDS};
use futures::{stream, StreamExt};
use pubsub::nonblocking::pubsub_client::PubsubClient;
use sdk::{
    consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID},
    delegate_args::{DelegateAccountMetas, DelegateAccounts},
};
use solana::{
    account::ReadableAccount,
    instruction::{AccountMeta, Instruction as SolanaInstruction},
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    system_program,
    system_transaction::transfer,
    transaction::Transaction,
};
use tokio::sync::{Notify, OwnedSemaphorePermit};

use crate::{client::SolanaClient, stats::LatencyCollection};

pub struct Pda {
    pub payer: Keypair,
    pub pubkey: Pubkey,
    pub bump: u8,
    pub clones: Option<Vec<Pubkey>>,
    pub shutdown: Rc<Notify>,
    pub subscriptions: bool,
    pub confirmations: bool,
}

impl Pda {
    pub async fn new(
        client: &SolanaClient,
        payer: Keypair,
        subscriptions: bool,
        confirmations: bool,
    ) -> Self {
        let pubkey = payer.pubkey();
        if client
            .get_account(&pubkey)
            .await
            .map(|a| a.lamports())
            .unwrap_or_default()
            == 0
        {
            let _ = client.request_airdrop(&pubkey, LAMPORTS_PER_SOL).await;
        }
        let (pubkey, bump) =
            Pubkey::find_program_address(&[pubkey.as_ref(), SEEDS], &benchprog::ID);
        Self {
            payer,
            pubkey,
            bump,
            clones: None,
            shutdown: Default::default(),
            subscriptions,
            confirmations,
        }
    }

    pub fn subscribe(
        &self,
        ws: Rc<PubsubClient>,
        latency: Rc<RefCell<LatencyCollection>>,
        offset: u64,
    ) {
        let mut id = offset;
        let pubkey = self.pubkey;
        let shutdown = self.shutdown.clone();
        let task = async move {
            let (mut s, _) = ws
                .account_subscribe(&pubkey, None)
                .await
                .expect("failed to subscribe to PDA");
            loop {
                tokio::select! {
                    Some(_) = s.next() => {
                        latency.borrow_mut().update.confirm(&id);
                    }
                    _ = shutdown.notified() => {
                        break;
                    }
                }
                id += offset + 1;
            }
            //println!("subscriptions are closed: {offset}");
        };
        tokio::task::spawn_local(task);
    }

    pub async fn init(&self, client: &SolanaClient, space: u32) {
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

        txn.sign(&[&self.payer], client.hash());
        let r = client.send_and_confirm_transaction(&txn).await;
        //println!("initialized account: {r:?}");
    }

    pub async fn close(&self, client: &SolanaClient) {
        let pk = self.payer.pubkey();
        let metas = vec![
            AccountMeta::new(pk, true),
            AccountMeta::new(self.pubkey, false),
        ];
        let ix = Instruction::Close;
        let ix = SolanaInstruction::new_with_borsh(benchprog::ID, &ix, metas);
        let mut txn = Transaction::new_with_payer(&[ix], Some(&pk));

        txn.sign(&[&self.payer], client.hash());
        let r = client.send_and_confirm_transaction(&txn).await;
        //println!("closed account: {r:?}");
    }

    pub async fn delegate(&self, client: &SolanaClient) {
        let pk = self.payer.pubkey();
        let accounts = DelegateAccounts::new(self.pubkey, benchprog::ID);
        let metas = DelegateAccountMetas::from(accounts).into_vec(pk);
        let ix = Instruction::Delegate;
        let ix = SolanaInstruction::new_with_borsh(benchprog::ID, &ix, metas);
        let mut txn = Transaction::new_with_payer(&[ix], Some(&pk));

        txn.sign(&[&self.payer], client.hash());
        let r = client.send_and_confirm_transaction(&txn).await;
        //println!("delegated account: {r:?}");
    }

    pub async fn undelegate(&self, client: &SolanaClient) {
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

        txn.sign(&[&self.payer], client.hash());
        let r = client.send_and_confirm_transaction(&txn).await;
        //println!("undelegated account: {r:?}");
    }

    pub async fn fill_space(
        self: Rc<Self>,
        client: Rc<SolanaClient>,
        ws: Rc<PubsubClient>,
        value: u64,
        latency: Rc<RefCell<LatencyCollection>>,
        _guard: OwnedSemaphorePermit,
        id: u64,
    ) {
        let metas = vec![AccountMeta::new(self.pubkey, false)];
        let ix = Instruction::FillSpace { value };
        self.send_transaction(ix, metas, latency, ws, &client, id)
            .await;
    }

    pub async fn compute_sum(
        self: Rc<Self>,
        client: Rc<SolanaClient>,
        ws: Rc<PubsubClient>,
        latency: Rc<RefCell<LatencyCollection>>,
        _guard: OwnedSemaphorePermit,
        id: u64,
    ) {
        let Some(accounts) = &self.clones else { return };
        let mut metas = vec![AccountMeta::new(self.pubkey, false)];
        metas.extend(
            accounts
                .iter()
                .map(|&a| AccountMeta::new_readonly(a, false)),
        );
        let ix = Instruction::ComputeSum { index: id as u32 };
        self.send_transaction(ix, metas, latency, ws, &client, id)
            .await;
    }

    pub async fn generate_clones(&mut self, client: &SolanaClient, noise: u8) {
        let pk = self.payer.pubkey();
        let mut pdas = Vec::with_capacity(noise as usize);
        for seed in 0..noise {
            let (pubkey, bump) =
                Pubkey::find_program_address(&[pk.as_ref(), SEEDS, &[seed]], &benchprog::ID);
            //println!("checking ro account: {pubkey}");
            //let lamports = client
            //    .get_account(&pubkey)
            //    .await
            //    .map(|a| (a.lamports))
            //    .unwrap_or_default();
            //if lamports != 0 {
            //    continue;
            //}
            //let ix = Instruction::InitClonable {
            //    space: noise as u32,
            //    bump,
            //    seed,
            //};
            //let metas = vec![
            //    AccountMeta::new(pk, true),
            //    AccountMeta::new(pubkey, false),
            //    AccountMeta::new_readonly(system_program::ID, false),
            //];
            //let ix = SolanaInstruction::new_with_borsh(benchprog::ID, &ix, metas);
            //
            //let mut txn = Transaction::new_with_payer(&[ix], Some(&pk));
            //
            //txn.sign(&[&self.payer], client.hash());
            //let sig = client
            //    .send_transaction_with_config(
            //        &txn,
            //        RpcSendTransactionConfig {
            //            skip_preflight: true,
            //            ..Default::default()
            //        },
            //    )
            //    .await
            //    .expect("failed to create RO pda");
            //println!("generated clonable RO pda: {pubkey}, {sig}");
            pdas.push(pubkey);
        }
        self.clones.replace(pdas);
    }

    pub fn topup(&self, lamports: u64, client: Rc<SolanaClient>) {
        let Some(pdas) = &self.clones else {
            return;
        };
        for pda in pdas {
            let hash = client.hash();
            let tx = transfer(&self.payer, pda, lamports, hash);
            let client = client.clone();
            let task = async move {
                client.send_transaction(&tx).await.unwrap();
            };
            tokio::task::spawn_local(task);
        }
    }

    pub async fn send_transaction(
        &self,
        ix: Instruction,
        metas: Vec<AccountMeta>,
        latency: Rc<RefCell<LatencyCollection>>,
        ws: Rc<PubsubClient>,
        client: &SolanaClient,
        id: u64,
    ) {
        let pk = self.payer.pubkey();
        let ix = SolanaInstruction::new_with_borsh(benchprog::ID, &ix, metas);
        let mut txn = Transaction::new_with_payer(&[ix], Some(&pk));

        txn.sign(&[&self.payer], client.hash());
        let mut s = if self.subscriptions {
            let (s, _) = ws
                .signature_subscribe(&txn.signatures[0], None)
                .await
                .expect("failed to subscribe to signature");
            s
        } else {
            Box::pin(stream::empty())
        };
        if self.subscriptions {
            latency.borrow_mut().confirmation.track(id);
            latency.borrow_mut().update.track(id);
        }
        latency.borrow_mut().delivery.track(id);
        let result = if self.confirmations {
            client.send_and_confirm_transaction(&txn).await
        } else {
            client.send_transaction(&txn).await
        };
        if let Err(error) = result {
            eprintln!("error sending transaction: {error}");
            latency.borrow_mut().record_error(&id);
        } else {
            //println!("result: {}", result.unwrap());
        }
        latency.borrow_mut().delivery.confirm(&id);
        if self.subscriptions && s.next().await.is_some() {
            latency.borrow_mut().confirmation.confirm(&id);
        }
    }
}

impl Drop for Pda {
    fn drop(&mut self) {
        self.shutdown.notify_one();
    }
}
