use std::{cell::RefCell, rc::Rc, time::Duration};

use hash::Hash;
use hyper::Request;

use crate::{extractor::blockhash_extractor, http::Connection, payload, BenchResult};

pub struct BlockHashProvider {
    hash: Rc<RefCell<Hash>>,
}

impl BlockHashProvider {
    pub async fn new(mut ephem: Connection) -> BenchResult<Self> {
        let hash = Self::request(&mut ephem).await?;
        let hash = Rc::new(RefCell::new(hash));
        tokio::task::spawn_local(Self::refresher(ephem, hash.clone()));
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

    async fn refresher(mut ephem: Connection, hash: Rc<RefCell<Hash>>) {
        let mut interval = tokio::time::interval(Duration::from_secs(23));
        loop {
            interval.tick().await;
            let h = match Self::request(&mut ephem).await {
                Ok(h) => h,
                Err(err) => {
                    eprintln!("failed to request hash from ephem: {err}");
                    continue;
                }
            };
            hash.replace(h);
        }
    }
}
