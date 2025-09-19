use base64::{prelude::BASE64_STANDARD, Engine};
use core::types::AccountEncoding;
use pubkey::Pubkey;
use signature::Signature;
use transaction::Transaction;

/// # Blockhash Payload
///
/// Creates a JSON payload for a `getLatestBlockhash` RPC request.
pub fn blockhash() -> String {
    r#"{"jsonrpc":"2.0","id":1,"method":"getLatestBlockhash","params":[{"commitment":"processed"}]}"#.into()
}

/// # Account Subscription Payload
///
/// Creates a JSON payload for an `accountSubscribe` RPC request.
pub fn account_subscription(pubkey: Pubkey, encoding: AccountEncoding, id: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"method":"accountSubscribe","params":["{}",{{"encoding":"{}","commitment":"processed"}}]}}"#,
        id,
        pubkey,
        encoding.as_str()
    )
}

/// # Signature Status Payload
///
/// Creates a JSON payload for a `getSignatureStatuses` RPC request.
#[allow(unused)]
pub fn signature_status(txn: &Transaction) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatuses","params":[["{}"]]}}"#,
        &txn.signatures[0]
    )
}

/// # Signature Subscription Payload
///
/// Creates a JSON payload for a `signatureSubscribe` RPC request.
pub fn signature_subscription(signature: Signature, id: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"method":"signatureSubscribe","params":["{signature}",{{"commitment":"processed"}}]}}"#,
        id,
    )
}

/// # Transaction Payload
///
/// Creates a JSON payload for a `sendTransaction` RPC request.
pub fn transaction(transaction: &Transaction, check: bool) -> String {
    let serialized = bincode::serialize(transaction).expect("transaction should serialize");
    let encoded = BASE64_STANDARD.encode(serialized);
    format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":["{}",{{"skipPreflight":{},"encoding":"base64", "preflightCommitment": "processed"}}]}}"#,
        encoded, !check
    )
}

/// # Get Account Info Payload
///
/// Creates a JSON payload for a `getAccountInfo` RPC request.
pub fn get_account_info(pubkey: Pubkey, encoding: AccountEncoding, id: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"getAccountInfo","params":["{pubkey}",{{"encoding":"{}"}}]}}"#,
        encoding.as_str()
    )
}

/// # Get Multiple Accounts Payload
///
/// Creates a JSON payload for a `getMultipleAccounts` RPC request.
pub fn get_multiple_accounts(pubkeys: &[Pubkey], encoding: AccountEncoding, id: u64) -> String {
    let pubkeys: Vec<String> = pubkeys.iter().map(|pk| pk.to_string()).collect();
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"getMultipleAccounts","params":[{pubkeys:?},{{"encoding":"{}"}}]}}"#,
        encoding.as_str()
    )
}

/// # Get Balance Payload
///
/// Creates a JSON payload for a `getBalance` RPC request.
pub fn get_balance(pubkey: Pubkey, id: u64) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"getBalance","params":["{pubkey}"]}}"#,)
}

/// # Get Token Account Balance Payload
///
/// Creates a JSON payload for a `getTokenAccountBalance` RPC request.
pub fn get_token_account_balance(pubkey: Pubkey, id: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"getTokenAccountBalance","params":["{pubkey}"]}}"#,
    )
}
