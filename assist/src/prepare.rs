use core::{AccountSize, BenchResult, Config};
use std::{path::PathBuf, sync::Arc};

use keypair::Keypair;
use pubkey::Pubkey;
use rpc::nonblocking::rpc_client::RpcClient;
use signer::{EncodableKey, Signer};

const LAMPORTS_PER_BENCH: u64 = 100_000_000;

struct Preparator {
    config: Config,
    vault: Keypair,
    client: RpcClient,
    keypairs: Vec<Keypair>,
}

pub async fn prepare(path: PathBuf) -> BenchResult<()> {
    let config = Config::from_path(path)?;
    let keypairs: Vec<_> = (1..=config.benchmark.parallelism)
        .map(|n| Keypair::read_from_file(format!("keypairs/{n:>03}.json")))
        .collect::<BenchResult<_>>()?;
    let vault = Keypair::read_from_file("keypairs/vault.json")?;
    let client = RpcClient::new(config.connection.chain_url.0.to_string());
    let preparator = Preparator {
        config,
        vault,
        client,
        keypairs,
    };
    preparator.fund().await?;

    Ok(())
}

impl Preparator {
    async fn fund(&self) -> BenchResult<()> {
        for kp in &self.keypairs {
            let pk = &kp.pubkey();
            let lamports = self.client.get_balance(pk).await?;
            if lamports < LAMPORTS_PER_BENCH {
                self.transfer(pk, LAMPORTS_PER_BENCH - lamports).await?;
            }
        }
        Ok(())
    }

    async fn init(&self) -> BenchResult<()> {
        todo!()
    }

    async fn delegate() -> BenchResult<()> {
        todo!()
    }

    async fn transfer(&self, to: &Pubkey, amount: u64) -> BenchResult<()> {
        let hash = self.client.get_latest_blockhash().await?;
        let txn = systransaction::transfer(&self.vault, to, amount, hash);
        self.client.send_transaction(&txn).await?;
        Ok(())
    }
}
