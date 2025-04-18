use std::{cell::RefCell, rc::Rc, time::Duration};

use hash::Hash;
use hyper::Request;

use crate::{
    extractor::blockhash_extractor, http::Connection, payload, BenchResult, ShutDownListener,
};

pub struct BlockHashProvider {
    hash: Rc<RefCell<Hash>>,
}

impl BlockHashProvider {
    pub async fn new(mut ephem: Connection, shutdown: ShutDownListener) -> BenchResult<Self> {
        let hash = Self::request(&mut ephem).await?;
        let hash = Rc::new(RefCell::new(hash));
        tokio::task::spawn_local(Self::refresher(ephem, hash.clone(), shutdown));
        Ok(Self { hash })
    }

    pub fn hash(&self) -> Hash {
        *self.hash.borrow()
    }

    async fn request(ephem: &mut Connection) -> BenchResult<Hash> {
        let request = Request::new(payload::blockhash());
        ephem
            .send(request, blockhash_extractor)
            .resolve()
            .await?
            .ok_or("blockhash was not found in response for getLatestBlockhash".into())
    }

    async fn refresher(
        mut ephem: Connection,
        hash: Rc<RefCell<Hash>>,
        mut shutdown: ShutDownListener,
    ) {
        let mut interval = tokio::time::interval(Duration::from_secs(23));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match Self::request(&mut ephem).await {
                        Ok(h) => { hash.replace(h); },
                        Err(err) => eprintln!("failed to request hash from ephem: {err}"),
                    };
                }
                _ = shutdown.recv() => {
                    break;
                }
            }
        }
    }
}
