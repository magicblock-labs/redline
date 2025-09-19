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

use crate::{
    blockhash::BlockHashProvider,
    extractor::{signature_response_extractor, value_extractor},
    payload,
    transaction::TransactionProvider,
};
use rand::{distributions::WeightedIndex, prelude::Distribution, rngs::ThreadRng, thread_rng};

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
    signer: Keypair,
    blockhash_provider: BlockHashProvider,
    signature: Option<Signature>,
}

impl RequestBuilder for TransactionRequestBuilder {
    fn name(&self) -> &'static str {
        self.provider.name()
    }
    fn build(&mut self, id: u64) -> Request<String> {
        let blockhash = self.blockhash_provider.hash();
        let tx = self.provider.generate(id, blockhash, &self.signer);
        self.signature.replace(tx.signatures[0]);
        Request::new(payload::transaction(&tx, false))
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

/// # Get Account Info Request Builder
///
/// A request builder that generates `getAccountInfo` RPC requests.
pub struct GetAccountInfoRequestBuilder {
    accounts: Vec<Pubkey>,
    encoding: AccountEncoding,
}

impl RequestBuilder for GetAccountInfoRequestBuilder {
    fn name(&self) -> &'static str {
        "GetAccountInfo"
    }
    fn build(&mut self, id: u64) -> Request<String> {
        let pubkey = self.accounts[id as usize % self.accounts.len()];
        Request::new(payload::get_account_info(pubkey, self.encoding, id))
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

/// # Get Balance Request Builder
///
/// A request builder that generates `getBalance` RPC requests.
pub struct GetBalanceRequestBuilder {
    accounts: Vec<Pubkey>,
}

impl RequestBuilder for GetBalanceRequestBuilder {
    fn name(&self) -> &'static str {
        "GetBalance"
    }
    fn build(&mut self, id: u64) -> Request<String> {
        let pubkey = self.accounts[id as usize % self.accounts.len()];
        Request::new(payload::get_balance(pubkey, id))
    }
    fn extractor(&self) -> fn(json::LazyValue) -> Option<bool> {
        value_extractor
    }
}

/// # Get Token Account Balance Request Builder
///
/// A request builder that generates `getTokenAccountBalance` RPC requests.
pub struct GetTokenAccountBalanceRequestBuilder {
    accounts: Vec<Pubkey>,
}

impl RequestBuilder for GetTokenAccountBalanceRequestBuilder {
    fn name(&self) -> &'static str {
        "GetTokenAccountBalance"
    }
    fn build(&mut self, id: u64) -> Request<String> {
        let pubkey = self.accounts[id as usize % self.accounts.len()];
        Request::new(payload::get_token_account_balance(pubkey, id))
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
    distribution: WeightedIndex<u8>,
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
            .first()
            .map(|p| p.accounts())
            .unwrap_or_default()
    }
    fn extractor(&self) -> fn(json::LazyValue) -> Option<bool> {
        self.providers
            .iter()
            .find(|p| p.name() == self.last_name)
            .map(|p| p.extractor())
            .unwrap_or(value_extractor)
    }
}

// --- Factory Function ---

/// # Make Builder
///
/// A factory function that creates a request builder based on the provided benchmark configuration.
pub fn make_builder(
    config: &Config,
    signer: Keypair,
    blockhash_provider: BlockHashProvider,
) -> Box<dyn RequestBuilder> {
    let base = signer.pubkey();
    let space = config.data.account_size as u32;
    let encoding = config.data.account_encoding;
    let accounts: Vec<Pubkey> = (1..=config.benchmark.accounts_count)
        .map(|seed| derive_pda(base, space, seed).0)
        .collect();

    make_mode_provider(
        &config.benchmark.mode,
        accounts,
        encoding,
        base,
        space,
        signer,
        blockhash_provider,
        config.benchmark.accounts_count,
    )
}

fn make_mode_provider(
    mode: &BenchMode,
    accounts: Vec<Pubkey>,
    encoding: AccountEncoding,
    base: Pubkey,
    space: u32,
    signer: Keypair,
    blockhash_provider: BlockHashProvider,
    accounts_count: u8,
) -> Box<dyn RequestBuilder> {
    match mode {
        BenchMode::GetAccountInfo => Box::new(GetAccountInfoRequestBuilder { accounts, encoding }),
        BenchMode::GetMultipleAccounts => {
            Box::new(GetMultipleAccountsRequestBuilder { accounts, encoding })
        }
        BenchMode::GetBalance => Box::new(GetBalanceRequestBuilder { accounts }),
        BenchMode::GetTokenAccountBalance => {
            Box::new(GetTokenAccountBalanceRequestBuilder { accounts })
        }
        BenchMode::Mixed(modes) => {
            let (providers, weights): (Vec<_>, Vec<_>) = modes
                .iter()
                .map(|m| {
                    (
                        make_mode_provider(
                            &m.mode,
                            accounts.clone(),
                            encoding,
                            base,
                            space,
                            signer.insecure_clone(),
                            blockhash_provider.clone(),
                            accounts_count,
                        ),
                        m.weight,
                    )
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
        tps_mode => Box::new(TransactionRequestBuilder {
            provider: crate::transaction::make_provider(tps_mode, base, space, accounts),
            signer,
            blockhash_provider,
            signature: None,
        }),
    }
}
