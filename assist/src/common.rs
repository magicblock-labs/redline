use commitment::CommitmentConfig;
use core::{config::Config, types::BenchResult};
use keypair::Keypair;
use program::utils::derive_pda;
use pubkey::Pubkey;
use rpc::nonblocking::rpc_client::RpcClient;
use signer::{EncodableKey, Signer};
use std::rc::Rc;

const CONFIRMED: CommitmentConfig = CommitmentConfig::confirmed();

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
