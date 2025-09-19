// bencher/src/confirmation.rs

use core::stats::ObservationsStats;
use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Instant};

use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    oneshot,
};

use crate::ShutDownListener;

/// A type alias for a reference-counted, interior-mutable `Confirmations` struct.
pub type ConfirmationsDB<V> = Rc<RefCell<Confirmations<V>>>;
/// A type alias for the sender part of a confirmation channel.
type ConfirmationSender<V> = Sender<(u64, V)>;
/// A type alias for the receiver part of a confirmation channel.
type ConfirmationReceiver<V> = Receiver<(u64, V)>;

/// # Confirmations
///
/// A generic struct for tracking and managing confirmations for any type of event.
#[derive(Debug)]
pub struct Confirmations<V> {
    /// A map of pending confirmations, with the key being the request ID.
    pending: HashMap<u64, PendingConfirmation<V>>,
    /// A vector of observed latencies, in microseconds.
    observations: Vec<u32>,
    /// The sender part of the confirmation channel.
    pub tx: ConfirmationSender<V>,
}

/// # Pending Confirmation
///
/// Holds the state of a pending confirmation, including the start time and an optional `oneshot` sender.
#[derive(Debug)]
pub struct PendingConfirmation<V> {
    /// The time when the request was initiated.
    start: Instant,
    /// An optional `oneshot` sender to notify when the confirmation is received.
    tx: Option<oneshot::Sender<V>>,
}

/// # Event Confirmer
///
/// A generic struct for confirming events, with support for graceful shutdown.
pub struct EventConfirmer<V> {
    /// The database of confirmations.
    pub db: ConfirmationsDB<V>,
    /// The receiver part of the confirmation channel.
    rx: ConfirmationReceiver<V>,
    /// A listener for the shutdown signal.
    shutdown: ShutDownListener,
}

impl<V> EventConfirmer<V> {
    /// # New Event Confirmer
    ///
    /// Creates a new `EventConfirmer` instance.
    pub fn new(shutdown: ShutDownListener) -> Self {
        let (db, rx) = Confirmations::new();
        Self { db, rx, shutdown }
    }

    /// # Confirm by ID
    ///
    /// An asynchronous method that listens for confirmations and records them in the database.
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
    }
}

impl EventConfirmer<u64> {
    /// # Confirm by Value
    ///
    /// An asynchronous method that listens for confirmations and records them by value.
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
    }
}

impl<V> Confirmations<V> {
    /// # New Confirmations
    ///
    /// Creates a new `Confirmations` instance, along with its corresponding receiver.
    pub fn new() -> (ConfirmationsDB<V>, ConfirmationReceiver<V>) {
        let (tx, rx) = mpsc::channel(1024);
        let confirmations = Confirmations {
            pending: HashMap::new(),
            observations: Vec::new(),
            tx,
        };
        (Rc::new(confirmations.into()), rx)
    }

    /// # Track Confirmation
    ///
    /// Starts tracking a new confirmation, adding it to the pending map.
    ///
    /// ### Arguments
    ///
    /// * `id` - The unique identifier for the request.
    /// * `tx` - An optional `oneshot` sender to be notified upon confirmation.
    pub fn track(&mut self, id: u64, tx: Option<oneshot::Sender<V>>) {
        let pending = PendingConfirmation {
            start: Instant::now(),
            tx,
        };
        self.pending.insert(id, pending);
    }

    /// # Observe Confirmation
    ///
    /// Records a received confirmation, calculating the latency and sending a notification if requested.
    ///
    /// ### Arguments
    ///
    /// * `id` - The unique identifier for the request.
    /// * `v` - The value associated with the confirmation.
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

    /// # Finalize Statistics
    ///
    /// Calculates and returns the final `ObservationsStats` for all recorded confirmations.
    pub fn finalize(self) -> ObservationsStats {
        ObservationsStats::new(self.observations, false)
    }
}
