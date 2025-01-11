use std::{cell::RefCell, rc::Rc, str::FromStr};

use base64::{prelude::BASE64_STANDARD, Engine};
use json::Value;
use reqwest::{header::CONTENT_TYPE, Client, Url};
use solana::{
    hash::Hash,
    pubkey::Pubkey,
    transaction::{Transaction, VersionedTransaction},
};

use crate::stats::TxnRequestStats;

pub struct TxnRequester {
    client: Client,
    pub url: Url,
    pub hash: RefCell<Hash>,
}

impl TxnRequester {
    pub fn new(url: Url) -> Self {
        Self {
            client: Client::new(),
            url,
            hash: Default::default(),
        }
    }

    pub async fn refresh_blockhash(&self) {
        let response: Value = self
            .client
            .post(self.url.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(r#"{"id":1,"jsonrpc":"2.0","method":"getLatestBlockhash"}"#)
            .send()
            .await
            .expect("failed to fetch blockhash")
            .json()
            .await
            .expect("failed to json parse response");
        let hash = response
            .get("result")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.get("blockhash"))
            .and_then(|v| v.as_str())
            .and_then(|h| Hash::from_str(h).ok())
            .expect("failed to extract hash value");
        self.hash.replace(hash);
    }

    pub async fn send(self: Rc<Self>, txn: Transaction, id: u64) -> TxnRequestStats {
        let versioned = VersionedTransaction::from(txn);
        let txn = bincode::serialize(&versioned).unwrap();
        let txn = BASE64_STANDARD.encode(txn);
        let serialized = format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"sendTransaction","params":["{txn}", {{"encoding":"base64"}}]}}"#
        );

        let url = self.url.clone();
        let builder = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json");
        let request = builder.body(serialized);
        let response = request.send().await;
        let mut stats = TxnRequestStats::new(id);
        let Ok(response) = response else {
            println!("failed to request anything");
            return stats;
        };
        if !response.status().is_success() {
            println!("bad response: {response:?}");
            return stats;
        } else {
            stats.success = true;
        }
        stats
    }
}

#[derive(Default, Clone)]
pub struct SolanaClient(Client);

#[derive(Default)]
pub struct AccountInfo {
    pub lamports: u64,
    pub size: usize,
    pub delegated: bool,
}

impl SolanaClient {
    pub async fn info(&self, url: Url, pubkey: &Pubkey) -> AccountInfo {
        let body = format!(
            r#"{{"id":1,"jsonrpc":"2.0","method":"getAccountInfo","params":["{pubkey}",{{"encoding":"base64"}}]}}"#
        );
        println!("B: {body}");
        let response: Value = self
            .0
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .expect("failed to fetch account from chain")
            .json()
            .await
            .expect("failed to json parse response");
        let data = |v: &Value| {
            v.get("data")
                .and_then(|d| d.as_str())
                .and_then(|d| BASE64_STANDARD.decode(d).ok())
                .unwrap_or_default()
        };
        let lamports = |v: &Value| {
            let lamports = v
                .get("lamports")
                .and_then(|l| l.as_u64())
                .unwrap_or_default();
            println!("lamports: {lamports} - {v}");
            lamports
        };
        let delegated = |v: &Value| {
            v.get("owner")
                .and_then(|l| l.as_str())
                .map(|owner| owner == "DELeGGvXpWV2fqJUhqcF5ZSYMS4JTLjteaAMARRSaeSh")
                .unwrap_or_default()
        };
        println!("{response}");
        response
            .get("result")
            .and_then(|v| v.get("value"))
            .map(|v| (lamports(v), data(v).len(), delegated(v)))
            .map(|(lamports, size, delegated)| AccountInfo {
                lamports,
                size,
                delegated,
            })
            .unwrap_or_default()
    }

    pub async fn airdrop(&self, url: Url, pubkey: &Pubkey) {
        let response: Value = self
            .0
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .body(format!(r#"{{"id":1,"jsonrpc":"2.0","method":"requestAirdrop","params":["{pubkey}",1000000000]}}"#))
            .send()
            .await
            .expect("failed to airdrop to account")
            .json()
            .await
            .expect("failed to json parse response");
        assert!(
            response
                .get("result")
                .and_then(|t| t.as_str())
                .map(|s| s.len())
                .unwrap_or_default()
                > 1,
            "failed to airdrop sol to {pubkey}: {response}"
        )
    }
}
