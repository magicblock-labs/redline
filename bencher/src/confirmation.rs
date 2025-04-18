use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Instant};

use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    oneshot,
};

use crate::{stats::ObservationsStats, ShutDownListener};

pub type ConfirmationsDB<V> = Rc<RefCell<Confirmations<V>>>;
type ConfirmationSender<V> = Sender<(u64, V)>;
type ConfirmationReceiver<V> = Receiver<(u64, V)>;

#[derive(Debug)]
pub struct Confirmations<V> {
    pending: HashMap<u64, PendingConfirmation<V>>,
    observations: Vec<u32>,
    pub tx: ConfirmationSender<V>,
}

#[derive(Debug)]
pub struct PendingConfirmation<V> {
    start: Instant,
    tx: Option<oneshot::Sender<V>>,
}

pub struct EventConfirmer<V> {
    pub db: ConfirmationsDB<V>,
    rx: ConfirmationReceiver<V>,
    shutdown: ShutDownListener,
}

impl<V> EventConfirmer<V> {
    pub fn new(shutdown: ShutDownListener) -> Self {
        let (db, rx) = Confirmations::new();
        Self { db, rx, shutdown }
    }

    pub async fn confirm_by_id(mut self) {
        loop {
            tokio::select! {
                Some((id, v)) = self.rx.recv() => {
                    self.db.borrow_mut().observe(id, v);
                },
                _ = self.shutdown.recv() => {
                    break;
                },
            }
        }
        println!("event confirmer has terminated id")
    }
}

impl EventConfirmer<u64> {
    pub async fn confirm_by_value(mut self) {
        loop {
            tokio::select! {
                Some((_, id)) = self.rx.recv() => {
                    self.db.borrow_mut().observe(id, id);
                },
                _ = self.shutdown.recv() => {
                    break;
                },
            }
        }
        println!("event confirmer has terminated value")
    }
}

impl<V> Confirmations<V> {
    pub fn new() -> (ConfirmationsDB<V>, ConfirmationReceiver<V>) {
        let (tx, rx) = mpsc::channel(1024);
        let confirmations = Confirmations {
            pending: HashMap::new(),
            observations: Vec::new(),
            tx,
        };
        (Rc::new(confirmations.into()), rx)
    }

    pub fn track(&mut self, id: u64, tx: Option<oneshot::Sender<V>>) -> ConfirmationSender<V> {
        let pending = PendingConfirmation {
            start: Instant::now(),
            tx,
        };
        self.pending.insert(id, pending);
        self.tx.clone()
    }

    pub fn observe(&mut self, id: u64, v: V) {
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        let took = pending.start.elapsed().as_micros() as u32;
        self.observations.push(took);
        if let Some(tx) = pending.tx {
            let _ = tx.send(v);
        }
    }

    pub fn finalize(self) -> ObservationsStats {
        ObservationsStats::new(self.observations)
    }
}

pub struct ConfirmationsBundle {
    pub accounts_updates: ConfirmationsDB<u64>,
    pub signature_confirmations: ConfirmationsDB<bool>,
    pub delivery_confirmations: ConfirmationsDB<()>,
}

impl ConfirmationsBundle {
    pub fn stats(self) -> json::Value {
        macro_rules! finalize {
            ($confirmation: expr) => {
                Rc::try_unwrap($confirmation)
                    .unwrap()
                    .into_inner()
                    .finalize()
            };
        }
        let stats = [
            finalize!(self.delivery_confirmations),
            finalize!(self.accounts_updates),
            finalize!(self.signature_confirmations),
        ];
        json::to_value(&stats).unwrap()
    }
}
