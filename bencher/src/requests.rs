use core::{
    config::Config,
    types::{AccountEncoding, BenchMode},
};
use hyper::Request;
use keypair::Keypair;
use program::utils::derive_pda;
use pubkey::Pubkey;
use signature::Signature;
use signer::Signer;
use std::collections::HashSet;

use crate::{
    blockhash::BlockHashProvider,
    extractor::{signature_response_extractor, value_extractor},
    payload,
    transaction::TransactionProvider,
};
use rand::{
    distributions::WeightedIndex, prelude::Distribution, rngs::ThreadRng, seq::SliceRandom,
    thread_rng,
};

/// # Request Builder Trait
///
/// A generic trait for building requests, designed to unify both transaction-based
/// and RPC-based request generation.
pub trait RequestBuilder {
    /// Returns the name of the benchmark mode.
    fn name(&self) -> &'static str;
    /// Builds a request to be sent to the Solana RPC endpoint.
    ///
    /// ### Arguments
    ///
    /// * `id` - A unique identifier for the request, used for tracking and payload generation.
    fn build(&mut self, id: u64) -> Request<String>;
    /// Returns the signature of the last generated transaction, if applicable.
    fn signature(&self) -> Option<Signature> {
        None
    }
    /// Returns a list of accounts used by the request builder.
    fn accounts(&self) -> Vec<Pubkey> {
        vec![]
    }
    /// Returns the extractor function for the request builder.
    fn extractor(&self) -> fn(json::LazyValue) -> Option<bool>;
}

// --- Transaction Request Builders ---

/// # Transaction Request Builder
///
/// A request builder that generates transaction-based requests.
pub struct TransactionRequestBuilder {
    provider: Box<dyn TransactionProvider>,
    signers: Vec<Keypair>,
    blockhash_provider: BlockHashProvider,
    signature: Option<Signature>,
    preflight: bool,
    rng: ThreadRng,
}

impl RequestBuilder for TransactionRequestBuilder {
    fn name(&self) -> &'static str {
        self.provider.name()
    }
    fn build(&mut self, id: u64) -> Request<String> {
        let blockhash = self.blockhash_provider.hash();
        let signer = self
            .signers
            .choose(&mut self.rng)
            .expect("should have at least one signer");
        let tx = self.provider.generate(id, blockhash, signer);
        self.signature.replace(tx.signatures[0]);
        Request::new(payload::transaction(&tx, self.preflight))
    }
    fn signature(&self) -> Option<Signature> {
        self.signature
    }
    fn accounts(&self) -> Vec<Pubkey> {
        self.provider.accounts()
    }
    fn extractor(&self) -> fn(json::LazyValue) -> Option<bool> {
        signature_response_extractor
    }
}

// --- RPC Request Builders ---

/// # Generic RPC Request Builder
///
/// A generic request builder for RPC calls that select a single account.
struct RpcRequestBuilder<F> {
    accounts: Vec<Pubkey>,
    payload_fn: F,
    name: &'static str,
}

impl<F> RpcRequestBuilder<F>
where
    F: FnMut(Pubkey, u64) -> String,
{
    fn new(accounts: Vec<Pubkey>, payload_fn: F, name: &'static str) -> Self {
        Self {
            accounts,
            payload_fn,
            name,
        }
    }
}

impl<F> RequestBuilder for RpcRequestBuilder<F>
where
    F: FnMut(Pubkey, u64) -> String,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn build(&mut self, id: u64) -> Request<String> {
        let pubkey = self.accounts[id as usize % self.accounts.len()];
        Request::new((self.payload_fn)(pubkey, id))
    }

    fn extractor(&self) -> fn(json::LazyValue) -> Option<bool> {
        value_extractor
    }
}

/// # Get Multiple Accounts Request Builder
///
/// A request builder that generates `getMultipleAccounts` RPC requests.
pub struct GetMultipleAccountsRequestBuilder {
    accounts: Vec<Pubkey>,
    encoding: AccountEncoding,
}

impl RequestBuilder for GetMultipleAccountsRequestBuilder {
    fn name(&self) -> &'static str {
        "GetMultipleAccounts"
    }
    fn build(&mut self, id: u64) -> Request<String> {
        Request::new(payload::get_multiple_accounts(
            &self.accounts,
            self.encoding,
            id,
        ))
    }
    fn extractor(&self) -> fn(json::LazyValue) -> Option<bool> {
        value_extractor
    }
}

/// # Mixed Request Builder
///
/// A request builder that combines multiple request builders to generate a mixed workload.
pub struct MixedRequestBuilder {
    providers: Vec<Box<dyn RequestBuilder>>,
    distribution: WeightedIndex<u16>,
    rng: ThreadRng,
    last_name: &'static str,
    last_index: usize,
}

impl RequestBuilder for MixedRequestBuilder {
    fn name(&self) -> &'static str {
        self.last_name
    }
    fn build(&mut self, id: u64) -> Request<String> {
        let index = self.distribution.sample(&mut self.rng);
        let provider = &mut self.providers[index];
        self.last_name = provider.name();
        self.last_index = index;
        provider.build(id)
    }
    fn signature(&self) -> Option<Signature> {
        self.providers[self.last_index].signature()
    }
    fn accounts(&self) -> Vec<Pubkey> {
        self.providers
            .iter()
            .flat_map(|p| p.accounts())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
    }
    fn extractor(&self) -> fn(json::LazyValue) -> Option<bool> {
        self.providers
            .iter()
            .find(|p| p.name() == self.last_name)
            .map(|p| p.extractor())
            .unwrap_or(value_extractor)
    }
}

pub fn make_builder(
    mode: &BenchMode,
    config: &Config,
    signers: Vec<Keypair>,
    blockhash_provider: BlockHashProvider,
) -> Box<dyn RequestBuilder> {
    let base = signers
        .first()
        .expect("should have at least 1 payer")
        .pubkey();
    let space = config.data.account_size as u32;
    let encoding = config.data.account_encoding;
    let accounts: Vec<Pubkey> = (1..=config.benchmark.accounts_count)
        .map(|seed| derive_pda(base, space, seed, config.authority).0)
        .collect();
    match mode {
        BenchMode::GetAccountInfo => Box::new(RpcRequestBuilder::new(
            accounts,
            move |pk, id| payload::get_account_info(pk, encoding, id),
            "GetAccountInfo",
        )),
        BenchMode::GetMultipleAccounts => {
            Box::new(GetMultipleAccountsRequestBuilder { accounts, encoding })
        }
        BenchMode::GetBalance => Box::new(RpcRequestBuilder::new(
            accounts,
            payload::get_balance,
            "GetBalance",
        )),
        BenchMode::GetTokenAccountBalance => Box::new(RpcRequestBuilder::new(
            accounts,
            payload::get_token_account_balance,
            "GetTokenAccountBalance",
        )),
        BenchMode::Mixed(modes) => {
            let (providers, weights): (Vec<_>, Vec<_>) = modes
                .iter()
                .map(|m| {
                    let signers = signers.iter().map(|k| k.insecure_clone()).collect();
                    let blockhash = blockhash_provider.clone();
                    (make_builder(&m.mode, config, signers, blockhash), m.weight)
                })
                .unzip();
            let distribution = WeightedIndex::new(weights).unwrap();
            let rng = thread_rng();
            Box::new(MixedRequestBuilder {
                providers,
                distribution,
                rng,
                last_name: "",
                last_index: 0,
            })
        }
        // Handle TPS modes by creating a TransactionRequestBuilder
        mode => Box::new(TransactionRequestBuilder {
            provider: crate::transaction::make_provider(mode, accounts),
            signers,
            blockhash_provider,
            signature: None,
            preflight: config.benchmark.preflight_check,
            rng: thread_rng(),
        }),
    }
}
