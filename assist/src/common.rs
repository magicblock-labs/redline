use core::{config::Config, types::BenchResult};
use keypair::Keypair;
use signer::EncodableKey;

/// # Load Vault Keypair
///
/// Loads the vault keypair from the configured keypairs directory.
pub fn load_vault(config: &Config) -> BenchResult<Keypair> {
    let path = format!("{}vault.json", config.keypairs.display());
    Keypair::read_from_file(&path).map_err(|e| {
        format!(
            "failed to read vault keypair from {path}, did you run `prepare` first? Error: {e}"
        )
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

    for i in 0..count {
        let path = format!("{}payer-{i}.json", config.keypairs.display());
        let keypair = Keypair::read_from_file(&path).map_err(|e| {
            format!("failed to read payer keypair from {path}, did you run `prepare` first? Error: {e}")
        })?;
        keypairs.push(keypair);
    }

    Ok(keypairs)
}
