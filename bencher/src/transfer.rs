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

pub struct TransferManager {
    vault: Keypair,
    pdas: VecDeque<Pubkey>,
    chain: Connection,
    last: Instant,
    frequency: Duration,
    blockhash: BlockHashProvider,
}

impl TransferManager {
    pub async fn new(
        config: &Config,
        vault: Keypair,
        pdas: &[Pubkey],
        blockhash: BlockHashProvider,
    ) -> Self {
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

    pub fn transfer(&mut self) {
        if self.frequency.is_zero() {
            return;
        }
        if self.last.elapsed() < self.frequency {
            return;
        }

        let Some(pda) = self.pdas.pop_front() else {
            return;
        };
        let blockhash = self.blockhash.hash();
        let txn = systransaction::transfer(&self.vault, &pda, 1, blockhash);
        let request = Request::new(payload::transaction(&txn, false));

        let response = self.chain.send(request, signature_response_extractor);
        tokio::task::spawn_local(async move {
            if let Err(err) = response.resolve().await {
                tracing::error!(%err, "failed to airdrop to pda (clone trigger)");
            }
        });
        self.pdas.push_back(pda);
    }
}
