use commitment::CommitmentConfig;
use core::{config::Config, types::BenchResult};
use keypair::Keypair;
use program::utils::derive_pda;
use pubkey::Pubkey;
use rpc::nonblocking::rpc_client::RpcClient;
use signer::{EncodableKey, Signer};
use std::{cell::RefCell, rc::Rc};
use tokio::task::LocalSet;

const CONFIRMED: CommitmentConfig = CommitmentConfig::confirmed();
const MAX_CONCURRENT_REQUESTS: usize = 8;

/// # Load Vault Keypair
///
/// Loads the vault keypair from the configured keypairs directory.
pub fn load_vault(config: &Config) -> BenchResult<Keypair> {
    let path = format!("{}/vault.json", config.keypairs.display());
    Keypair::read_from_file(&path).map_err(|e| {
        format!("failed to read vault keypair from {path}, did you run `prepare` first? Error: {e}")
            .into()
    })
}

/// # Load Payer Keypairs
///
/// Loads all benchmark payer keypairs (base accounts for PDA derivation).
/// These are NOT used as signers outside benchmarking, only for deriving PDA addresses.
pub fn load_payers(config: &Config) -> BenchResult<Vec<Keypair>> {
    let count = config.payers * config.parallelism;
    let mut keypairs = Vec::with_capacity(count as usize);

    for i in 1..=count {
        let path = format!("{}/{i}.json", config.keypairs.display());
        let keypair = Keypair::read_from_file(&path).map_err(|e| {
            format!(
                "failed to read payer keypair from {path}, did you run `prepare` first? Error: {e}"
            )
        })?;
        keypairs.push(keypair);
    }

    Ok(keypairs)
}

/// # Create Ephemeral RPC Client
///
/// Creates an RPC client connected to the ephemeral rollup validator.
pub fn create_ephem_client(config: &Config) -> Rc<RpcClient> {
    Rc::new(RpcClient::new_with_commitment(
        config.connection.ephem_url.0.to_string(),
        CONFIRMED,
    ))
}

/// # Create Chain RPC Client
///
/// Creates an RPC client connected to the base chain validator.
pub fn create_chain_client(config: &Config) -> Rc<RpcClient> {
    Rc::new(RpcClient::new_with_commitment(
        config.connection.chain_url.0.to_string(),
        CONFIRMED,
    ))
}

/// # Iterate PDAs
///
/// Creates an iterator over derived PDAs for the given keypairs and configuration.
/// Returns tuples of (pubkey, bump, seed, payer) for each derived PDA.
pub fn iter_pdas(
    keypairs: &[Keypair],
    payers_step: usize,
    count: u8,
    space: u32,
    authority: Pubkey,
) -> impl Iterator<Item = (Pubkey, u8, u8, Keypair)> + '_ {
    keypairs.iter().step_by(payers_step).flat_map(move |kp| {
        (1..=count).map(move |seed| {
            let (pk, bump) = derive_pda(kp.pubkey(), space, seed, authority);
            (pk, bump, seed, kp.insecure_clone())
        })
    })
}

/// Executes tasks concurrently with a maximum of 8 inflight requests at a time.
/// Returns the first error encountered, or Ok(()) if all tasks succeed.
pub async fn run_concurrent<F, Fut>(tasks: impl IntoIterator<Item = F>) -> BenchResult<()>
where
    F: FnOnce() -> Fut + 'static,
    Fut: std::future::Future<Output = BenchResult<()>> + 'static,
{
    let error = Rc::new(RefCell::new(None));
    let mut tasks = tasks.into_iter();

    loop {
        let local = LocalSet::new();
        let mut has_tasks = false;

        for task in tasks.by_ref().take(MAX_CONCURRENT_REQUESTS) {
            has_tasks = true;
            let error = error.clone();
            let fut = async move {
                if let Err(e) = task().await {
                    tracing::error!("{}", e);
                    if error.borrow().is_none() {
                        *error.borrow_mut() = Some(e.to_string());
                    }
                }
            };
            local.spawn_local(fut);
        }

        if !has_tasks {
            break;
        }

        local.await;

        if error.borrow().is_some() {
            break;
        }
    }

    if let Some(err) = error.borrow().as_ref() {
        return Err(err.clone().into());
    }

    Ok(())
}
