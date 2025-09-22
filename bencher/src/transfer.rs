use core::{config::Config, types::ConnectionType};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use hyper::Request;
use keypair::Keypair;
use pubkey::Pubkey;

use crate::{
    blockhash::BlockHashProvider, extractor::signature_response_extractor, http::Connection,
    payload,
};

/// # Transfer Manager
///
/// Manages the periodic transfer of lamports to Program Derived Addresses (PDAs)
/// to trigger account cloning. This is a crucial component of the "clone trigger"
/// benchmark mode, designed to test the validator's ability to handle account
/// state duplication under load.
pub struct TransferManager {
    /// The keypair for the vault account, which is the source of the funds for the transfers.
    vault: Keypair,
    /// A queue of PDAs to which lamports will be transferred. The PDAs are rotated
    /// to ensure an even distribution of transfers.
    pdas: VecDeque<Pubkey>,
    /// The HTTP connection to the Solana cluster's RPC endpoint.
    chain: Connection,
    /// The timestamp of the last transfer, used to determine when the next transfer should occur.
    last: Instant,
    /// The frequency at which transfers should be sent, configured in milliseconds.
    frequency: Duration,
    /// A provider for fetching and caching the latest blockhash.
    blockhash: BlockHashProvider,
}

impl TransferManager {
    /// # New Transfer Manager
    ///
    /// Creates a new `TransferManager` instance, initializing it with the provided
    /// configuration, vault keypair, and list of PDAs.
    pub async fn new(
        config: &Config,
        vault: Keypair,
        pdas: &[Pubkey],
        blockhash: BlockHashProvider,
    ) -> Self {
        // The frequency of the transfers is configured in the benchmark settings.
        let frequency = Duration::from_millis(config.benchmark.clone_frequency_ms);
        let last = Instant::now();
        let chain = Connection::new(&config.connection.chain_url, ConnectionType::Http2)
            .await
            .expect("failed to connect to chain endpoint");
        Self {
            vault,
            pdas: pdas.iter().copied().collect(),
            chain,
            last,
            frequency,
            blockhash,
        }
    }

    /// # Airdrop to PDA
    ///
    /// Periodically sends a small amount of lamports to a PDA to trigger account cloning.
    /// The transfer is sent as a transaction to the Solana cluster. Which in turn triggers
    /// the cloning of the PDA account on the ER
    pub fn transfer(&mut self) {
        // If the transfer frequency is zero, this feature is disabled.
        if self.frequency.is_zero() {
            return;
        }
        // If the time since the last transfer is less than the configured frequency, do nothing.
        if self.last.elapsed() < self.frequency {
            return;
        }

        // Get the next PDA from the queue. If the queue is empty, do nothing.
        let Some(pda) = self.pdas.pop_front() else {
            return;
        };
        // Get the latest blockhash from the provider.
        let blockhash = self.blockhash.hash();
        // Create a new system transfer transaction.
        let txn = systransaction::transfer(&self.vault, &pda, 1, blockhash);
        let request = Request::new(payload::transaction(&txn, false));

        // Asynchronously send the transaction and handle the response.
        let response = self.chain.send(request, signature_response_extractor);
        tokio::task::spawn_local(async move {
            if let Err(err) = response.resolve().await {
                tracing::error!(%err, "failed to airdrop to pda (clone trigger)");
            }
        });
        // Add the PDA back to the end of the queue to be used again later.
        self.pdas.push_back(pda);
    }
}
