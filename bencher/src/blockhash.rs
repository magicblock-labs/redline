use std::{cell::RefCell, rc::Rc, time::Duration};

use hash::Hash;
use hyper::Request;

use crate::{
    extractor::blockhash_extractor, http::Connection, payload, BenchResult, ShutDownListener,
};

/// Blockhash refresh interval (23 seconds).
/// Solana blockhashes expire at ~60s; 23s provides safety margin for multiple refreshes.
const BLOCKHASH_REFRESH: Duration = Duration::from_secs(23);

/// # Blockhash Provider
///
/// A provider for fetching and caching the latest blockhash from the Solana RPC endpoint.
/// It uses a background task to periodically refresh the blockhash, ensuring that it
/// remains up-to-date.
#[derive(Clone)]
pub struct BlockHashProvider {
    /// A reference-counted, interior-mutable cell holding the latest blockhash.
    hash: Rc<RefCell<Hash>>,
}

impl BlockHashProvider {
    /// # New Blockhash Provider
    ///
    /// Creates a new `BlockHashProvider`, fetches the initial blockhash, and spawns a
    /// background task to keep it refreshed.
    pub async fn new(mut ephem: Connection, shutdown: ShutDownListener) -> BenchResult<Self> {
        let hash = Self::request(&mut ephem).await?;
        let hash = Rc::new(RefCell::new(hash));
        tokio::task::spawn_local(Self::refresher(ephem, hash.clone(), shutdown));
        Ok(Self { hash })
    }

    /// # Get Blockhash
    ///
    /// Returns the latest cached blockhash.
    pub fn hash(&self) -> Hash {
        *self.hash.borrow()
    }

    /// # Request Blockhash
    ///
    /// Sends a request to the RPC endpoint to fetch the latest blockhash.
    async fn request(ephem: &mut Connection) -> BenchResult<Hash> {
        let request = Request::new(payload::blockhash());
        ephem
            .send(request, blockhash_extractor)
            .resolve()
            .await
            .inspect_err(|err| tracing::error!(%err, "error fetching blockhash"))?
            .ok_or("blockhash was not found in response for getLatestBlockhash".into())
    }

    /// # Blockhash Refresher
    ///
    /// A background task that periodically refreshes the blockhash, ensuring that it
    /// remains up-to-date.
    async fn refresher(
        mut ephem: Connection,
        hash: Rc<RefCell<Hash>>,
        mut shutdown: ShutDownListener,
    ) {
        let mut interval = tokio::time::interval(BLOCKHASH_REFRESH);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match Self::request(&mut ephem).await {
                        Ok(h) => { hash.replace(h); },
                        Err(err) => tracing::warn!("failed to request hash from ephem: {err}"),
                    };
                }
                _ = shutdown.recv() => {
                    break;
                }
            }
        }
    }
}
