use core::types::TpsBenchMode;
use std::time::{Duration, Instant};

use hash::Hash;
use hyper::Request;
use instruction::{AccountMeta, Instruction as SolanaInstruction};
use json::LazyValue;
use keypair::Keypair;
use program::{instruction::Instruction, utils::derive_pda};
use pubkey::Pubkey;
use rand::{rngs::ThreadRng, seq::SliceRandom, thread_rng};
use sdk::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use signer::Signer;
use transaction::Transaction;

use crate::{http::Connection, payload::airdrop};

pub trait TransactionProvider {
    fn generateix(&mut self, id: u64) -> SolanaInstruction;

    fn wrapix(&self, ix: Instruction, accounts: Vec<AccountMeta>) -> SolanaInstruction {
        SolanaInstruction::new_with_bincode(program::ID, &ix, accounts)
    }

    fn generate(&mut self, id: u64, blockhash: Hash, signer: &Keypair) -> Transaction {
        let ix = self.generateix(id);
        let mut tx = Transaction::new_with_payer(&[ix], Some(&signer.pubkey()));
        tx.sign(&[signer], blockhash);

        tx
    }

    fn accounts(&self) -> Vec<Pubkey>;

    fn bookkeep(&mut self, _chain: &mut Connection, _iteration: u64) {}
}

pub struct SimpleTransaction {
    pda: Pubkey,
}

pub struct ExpensiveTransaction {
    pda: Pubkey,
    iters: u32,
}

pub struct TriggerCloneTransaction {
    ro_accounts: Vec<Pubkey>,
    last_chain_update: Instant,
    frequency: Duration,
    pda: Pubkey,
}

pub struct ReadWriteAccountsTransaction {
    accounts: Vec<Pubkey>,
    rng: ThreadRng,
}

pub struct ReadOnlyTransaction {
    accounts: Vec<Pubkey>,
    rng: ThreadRng,
    count: usize,
}

pub struct CommitTransaction {
    accounts: Vec<Pubkey>,
    rng: ThreadRng,
    count: usize,
    payer: Pubkey,
}

impl TransactionProvider for SimpleTransaction {
    fn generateix(&mut self, id: u64) -> SolanaInstruction {
        let ix = Instruction::SimpleByteSet { id };
        let accounts = vec![AccountMeta::new(self.pda, false)];
        self.wrapix(ix, accounts)
    }
    fn accounts(&self) -> Vec<Pubkey> {
        vec![self.pda]
    }
}

impl TransactionProvider for ExpensiveTransaction {
    fn generateix(&mut self, id: u64) -> SolanaInstruction {
        let ix = Instruction::ExpensiveHashCompute {
            id,
            init: self.pda,
            iters: self.iters,
        };
        let accounts = vec![AccountMeta::new(self.pda, false)];
        self.wrapix(ix, accounts)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        vec![self.pda]
    }
}

impl TransactionProvider for ReadWriteAccountsTransaction {
    fn generateix(&mut self, id: u64) -> SolanaInstruction {
        let mut accounts = self.accounts.choose_multiple(&mut self.rng, 2).copied();
        let ix = Instruction::AccountDataCopy { id };
        let ro = AccountMeta::new_readonly(accounts.next().unwrap(), false);
        let rw = AccountMeta::new(accounts.next().unwrap(), false);
        self.wrapix(ix, vec![ro, rw])
    }

    fn accounts(&self) -> Vec<Pubkey> {
        self.accounts.clone()
    }
}

impl TransactionProvider for TriggerCloneTransaction {
    fn generateix(&mut self, id: u64) -> SolanaInstruction {
        let ix = Instruction::MultiAccountRead { id };
        let mut accounts = vec![AccountMeta::new(self.pda, false)];
        accounts.extend(
            self.ro_accounts
                .iter()
                .map(|&a| AccountMeta::new_readonly(a, false)),
        );
        self.wrapix(ix, accounts)
    }

    fn bookkeep(&mut self, chain: &mut Connection, iteration: u64) {
        if self.last_chain_update.elapsed() < self.frequency {
            return;
        }
        self.last_chain_update = Instant::now();
        let account = self.ro_accounts[iteration as usize % self.ro_accounts.len()];

        let request = Request::new(airdrop(account, iteration));
        let response = chain.send(request, |_: LazyValue| None::<()>);
        tokio::task::spawn_local(response.resolve());
    }

    fn accounts(&self) -> Vec<Pubkey> {
        let mut accounts = self.ro_accounts.clone();
        accounts.push(self.pda);
        accounts
    }
}

impl TransactionProvider for ReadOnlyTransaction {
    fn generateix(&mut self, id: u64) -> SolanaInstruction {
        let accounts = self
            .accounts
            .choose_multiple(&mut self.rng, self.count)
            .copied();
        let ix = Instruction::ReadAccountsData { id };
        let accounts = accounts
            .map(|acc| AccountMeta::new_readonly(acc, false))
            .collect();
        self.wrapix(ix, accounts)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        self.accounts.clone()
    }
}

impl TransactionProvider for CommitTransaction {
    fn generateix(&mut self, id: u64) -> SolanaInstruction {
        let ix = Instruction::CommitAccounts { id };
        let mut accounts = vec![
            AccountMeta::new(self.payer, true),
            AccountMeta::new(MAGIC_CONTEXT_ID, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false),
        ];
        accounts.extend(
            self.accounts
                .choose_multiple(&mut self.rng, self.count)
                .copied()
                .map(|acc| AccountMeta::new_readonly(acc, false)),
        );
        self.wrapix(ix, accounts)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        self.accounts.clone()
    }
}

impl TransactionProvider for Vec<Box<dyn TransactionProvider>> {
    fn generateix(&mut self, id: u64) -> SolanaInstruction {
        let index = id as usize % self.len();
        let generator = &mut self[index];
        generator.generateix(id)
    }

    fn accounts(&self) -> Vec<Pubkey> {
        self.iter().flat_map(|tp| tp.accounts()).collect()
    }

    fn bookkeep(&mut self, chain: &mut Connection, iteration: u64) {
        for tp in self.iter_mut() {
            tp.bookkeep(chain, iteration);
        }
    }
}

pub fn make_provider(
    mode: &TpsBenchMode,
    base: Pubkey,
    space: u32,
) -> Box<dyn TransactionProvider> {
    match mode {
        TpsBenchMode::Mixed(modes) => Box::new(
            modes
                .iter()
                .map(|m| make_provider(m, base, space))
                .collect::<Vec<_>>(),
        ),
        TpsBenchMode::SimpleByteSet => Box::new(SimpleTransaction {
            pda: derive_pda(base, space, 1).0,
        }),
        TpsBenchMode::TriggerClones {
            clone_frequency_secs,
            accounts_count,
        } => {
            let ro_accounts = (1..=*accounts_count)
                .map(|seed| derive_pda(base, space, seed).0)
                .collect();
            Box::new(TriggerCloneTransaction {
                pda: derive_pda(base, space, 1).0,
                ro_accounts,
                frequency: Duration::from_secs(*clone_frequency_secs),
                last_chain_update: Instant::now(),
            })
        }
        TpsBenchMode::ReadWrite { accounts_count } => {
            let accounts = (1..=*accounts_count)
                .map(|seed| derive_pda(base, space, seed).0)
                .collect();
            Box::new(ReadWriteAccountsTransaction {
                accounts,
                rng: thread_rng(),
            })
        }
        TpsBenchMode::HighCuCost { iters } => Box::new(ExpensiveTransaction {
            pda: derive_pda(base, space, 1).0,
            iters: *iters,
        }),
        TpsBenchMode::ReadOnly {
            accounts_count,
            accounts_per_transaction,
        } => {
            let accounts = (1..=*accounts_count)
                .map(|seed| derive_pda(base, space, seed).0)
                .collect();
            Box::new(ReadOnlyTransaction {
                accounts,
                count: *accounts_per_transaction as usize,
                rng: thread_rng(),
            })
        }
        TpsBenchMode::Commit {
            accounts_count,
            accounts_per_transaction,
        } => {
            let accounts = (1..=*accounts_count)
                .map(|seed| derive_pda(base, space, seed).0)
                .collect();
            Box::new(CommitTransaction {
                accounts,
                count: *accounts_per_transaction as usize,
                rng: thread_rng(),
                payer: base,
            })
        }
    }
}
