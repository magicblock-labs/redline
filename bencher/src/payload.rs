use core::types::AccountEncoding;

use base64::{prelude::BASE64_STANDARD, Engine};
use pubkey::Pubkey;
use transaction::Transaction;

pub fn airdrop(pubkey: Pubkey, amount: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"requestAirdrop","params":["{}",{}]}}"#,
        pubkey, amount
    )
}

pub fn blockhash() -> String {
    r#"{"jsonrpc":"2.0","id":1,"method":"getLatestBlockhash","params":[{"commitment":"processed"}]}"#.into()
}

pub fn account_subscription(pubkey: Pubkey, encoding: AccountEncoding, id: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"method":"accountSubscribe","params":["{}",{{"encoding":"{}","commitment":"processed"}}]}}"#,
        id,
        pubkey,
        encoding.as_str()
    )
}

// TODO: use in getSignatureStatuses implementation
#[allow(unused)]
pub fn signature_status(txn: &Transaction) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatuses","params":[["{}"]]}}"#,
        &txn.signatures[0]
    )
}

pub fn signature_subscription(transaction: &Transaction, id: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"method":"signatureSubscribe","params":["{}",{{"commitment":"processed"}}]}}"#,
        id, &transaction.signatures[0],
    )
}

pub fn transaction(transaction: &Transaction, check: bool) -> String {
    let serialized = bincode::serialize(transaction).expect("transaction should serialize");
    let encoded = BASE64_STANDARD.encode(serialized);
    format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":["{}",{{"skipPreflight":{},"encoding":"base64", "preflightCommitment": "processed"}}]}}"#,
        encoded, !check
    )
}
