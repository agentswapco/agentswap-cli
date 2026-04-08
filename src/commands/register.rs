// Register command for API-key onboarding via wallet signature.
// Exports: Args, run.
// Deps: crate::{client, credentials}, alloy signers, eyre, hex.

use eyre::{eyre, Result};
use crate::client::Client;

pub struct Args {
    pub address: String,
    pub private_key: Option<String>,
    pub key_file: Option<String>,
    pub json: bool,
}

pub async fn run(client: &Client, args: Args) -> Result<()> {
    let challenge_resp = client.challenge(&args.address).await?;
    let message = challenge_resp["message"]
        .as_str()
        .ok_or_else(|| eyre!("missing 'message' in challenge response"))?;
    let nonce = challenge_resp["nonce"]
        .as_str()
        .ok_or_else(|| eyre!("missing 'nonce' in challenge response"))?;

    let pk_value = if let Some(path) = &args.key_file {
        let content = std::fs::read_to_string(path)
            .map_err(|e| eyre!("failed to read key file '{path}': {e}"))?;
        let trimmed = content.trim().to_string();
        if trimmed.is_empty() {
            return Err(eyre!("key file '{path}' is empty"));
        }
        if args.private_key.is_some() {
            eprintln!("Warning: --key-file takes precedence over --private-key");
        }
        Some(trimmed)
    } else {
        args.private_key.clone()
    };

    let signature = if let Some(pk) = &pk_value {
        use alloy::signers::{local::PrivateKeySigner, SignerSync};
        let signer: PrivateKeySigner = pk.parse().map_err(|e| eyre!("invalid private key: {e}"))?;
        let sig = signer
            .sign_message_sync(message.as_bytes())
            .map_err(|e| eyre!("signing failed: {e}"))?;
        format!("0x{}", hex::encode(sig.as_bytes()))
    } else {
        eprintln!("Sign this message with your wallet:\n");
        eprintln!("{message}\n");
        eprintln!("Paste the signature (hex, with or without 0x prefix):");
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| eyre!("failed to read input: {e}"))?;
        input.trim().to_string()
    };

    let register_resp = client
        .register_key(&args.address, nonce, &signature)
        .await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&register_resp)?);
    } else {
        let api_key = register_resp["api_key"].as_str().unwrap_or("?");
        let quota = register_resp["quota_total"].as_i64().unwrap_or(0);
        let scopes = register_resp["scopes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();

        eprintln!("Registration successful!\n");
        println!("API Key:  {api_key}");
        println!("Scopes:   {scopes}");
        println!("Quota:    {quota} requests");
        eprintln!("\nSet your key: export SR_API_KEY={api_key}");
        if let Err(e) = crate::credentials::save_api_key(api_key) {
            eprintln!("Warning: could not cache API key: {e}");
        } else {
            eprintln!("API key saved to ~/.agentswap/credentials");
        }
    }

    Ok(())
}
