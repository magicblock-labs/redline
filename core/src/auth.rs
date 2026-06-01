//! Private Ephemeral Rollup (PER) authentication.
//!
//! When the ephemeral rollup runs inside a TEE, RPC access is gated behind a
//! challenge/response auth flow: the client requests a challenge for its pubkey,
//! signs it, exchanges the signature for a session token, and appends that token
//! to the RPC URL as a `token` query parameter.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{config::ConnectionSettings, types::BenchResult};

const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct ChallengeResponse {
    challenge: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct LoginRequest<'a> {
    pubkey: &'a str,
    challenge: &'a str,
    signature: &'a str,
}

#[derive(Deserialize)]
struct LoginResponse {
    token: Option<String>,
    error: Option<String>,
}

/// Runs the PER auth flow against `base_url` and returns a session token.
///
/// `sign_base58` receives the raw challenge bytes and must return the detached
/// ed25519 signature encoded as base58.
pub async fn fetch_auth_token(
    base_url: &str,
    pubkey: &str,
    sign_base58: impl FnOnce(&[u8]) -> String,
) -> BenchResult<String> {
    let client = reqwest::Client::builder().timeout(AUTH_TIMEOUT).build()?;

    // 1. Request a challenge for this pubkey.
    let challenge: ChallengeResponse = client
        .get(format!("{base_url}/auth/challenge?pubkey={pubkey}"))
        .send()
        .await?
        .json()
        .await?;
    if let Some(error) = challenge.error.filter(|e| !e.is_empty()) {
        return Err(format!("TEE auth challenge failed: {error}").into());
    }
    let challenge = challenge
        .challenge
        .filter(|c| !c.is_empty())
        .ok_or("TEE auth endpoint returned no challenge")?;

    // 2. Sign the challenge bytes and base58-encode the signature.
    let signature = sign_base58(challenge.as_bytes());

    // 3. Exchange the signed challenge for a session token.
    let response = client
        .post(format!("{base_url}/auth/login"))
        .json(&LoginRequest {
            pubkey,
            challenge: &challenge,
            signature: &signature,
        })
        .send()
        .await?;
    let succeeded = response.status().is_success();
    let login: LoginResponse = response.json().await?;
    if !succeeded {
        let error = login.error.unwrap_or_else(|| "unknown error".into());
        return Err(format!("TEE authentication failed: {error}").into());
    }
    login
        .token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "TEE auth endpoint returned no token".into())
}

/// If `connection` targets a TEE, runs the auth flow and appends the resulting
/// token to `connection.ephem_url`. Otherwise does nothing.
pub async fn authenticate_tee(
    connection: &mut ConnectionSettings,
    pubkey: &str,
    sign_base58: impl FnOnce(&[u8]) -> String,
) -> BenchResult<()> {
    if !connection.tee {
        return Ok(());
    }
    let base_url = connection.ephem_url.origin();
    tracing::info!("ephemeral rollup is a TEE, authenticating at {base_url}");
    let token = fetch_auth_token(&base_url, pubkey, sign_base58).await?;
    connection.ephem_url = connection.ephem_url.with_token(&token)?;
    tracing::info!("TEE authentication succeeded, session token appended to ephemeral URL");
    Ok(())
}
