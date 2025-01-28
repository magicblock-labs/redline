use std::{cell::Cell, ops::Deref, rc::Rc, time::Duration};

use rpc::nonblocking::rpc_client::RpcClient;
use solana::hash::Hash;
use tokio::sync::Notify;

pub struct SolanaClient {
    inner: RpcClient,
    hash: Cell<Hash>,
    shutdown: Rc<Notify>,
}

impl Deref for SolanaClient {
    type Target = RpcClient;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl SolanaClient {
    pub fn new(url: String, interval: u64) -> Rc<Self> {
        let inner = RpcClient::new(url);
        let hash = Cell::default();
        let shutdown = Default::default();
        let this: Rc<_> = Self {
            inner,
            hash,
            shutdown,
        }
        .into();
        tokio::task::spawn_local(this.clone().refresh_hash(interval));
        this
    }

    pub async fn refresh_hash(self: Rc<Self>, interval: u64) {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval));
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let Ok(hash) = self.inner.get_latest_blockhash().await else {
                        eprintln!("failed to get latest blockhash from {}", self.inner.url());
                        continue;
                    };
                    self.hash.replace(hash);

                }
                _ = self.shutdown.notified() => {
                    //eprintln!("shutting down the hash refresher: {}", self.inner.url());
                    break
                }
            }
        }
    }

    #[inline]
    pub fn hash(&self) -> Hash {
        self.hash.get()
    }

    pub fn shutdown(&self) {
        self.shutdown.notify_one();
    }
}
