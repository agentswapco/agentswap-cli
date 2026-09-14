// Register command for API-key onboarding via wallet signature.
// Exports: Args, run.
// Deps: crate::{client, credentials, signer}, zeroize, eyre, hex.

use eyre::{eyre, Result};
use crate::client::Client;
use crate::signer::{local::LocalKey, Signer};
use zeroize::Zeroizing;

pub struct Args {
    pub address: String,
    pub private_key: Option<String>,
    pub key_file: Option<String>,
    pub json: bool,
}

pub async fn run(client: &Client, args: Args) -> Result<()> {
    let signer = registration_signer(args.key_file.as_deref(), args.private_key)?;
    let challenge_resp = client.challenge(&args.address).await?;
    let message = challenge_resp["message"]
        .as_str()
        .ok_or_else(|| eyre!("missing 'message' in challenge response"))?;
    let nonce = challenge_resp["nonce"]
        .as_str()
        .ok_or_else(|| eyre!("missing 'nonce' in challenge response"))?;

    let signature = if let Some(signer) = signer {
        let sig = signer.sign_message(message.as_bytes()).await?;
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
        let scopes = register_resp["scopes"].as_array().map(|a| {
            a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", ")
        }).unwrap_or_default();

        eprintln!("Registration successful.");
        println!("Scopes:   {scopes}");
        println!("Quota:    {quota} requests");
        if let Err(e) = crate::credentials::save_api_key(api_key) {
            eprintln!("Warning: could not cache API key: {e}");
        } else {
            eprintln!("API key saved to ~/.agentswap/credentials.");
        }
        eprintln!("To use it in this shell: export SR_API_KEY={api_key}");
    }

    Ok(())
}

fn registration_signer(key_file: Option<&str>, private_key: Option<String>) -> Result<Option<LocalKey>> {
    let private_key = private_key.map(Zeroizing::new);
    if let Some(path) = key_file {
        if private_key.is_some() {
            eprintln!("Warning: --key-file takes precedence over --private-key");
        }
        return LocalKey::from_key_file(path).map(Some);
    }
    private_key.as_ref().map(|key| LocalKey::from_private_key(key)).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_key_sources_preserve_precedence_and_manual_signing() {
        assert!(registration_signer(None, None).unwrap().is_none());
        assert!(registration_signer(None, Some("invalid".into())).is_err());
        let canary = "01".repeat(32);
        let expected = registration_signer(None, Some(canary.clone())).unwrap().unwrap();
        let path = std::env::temp_dir().join(format!("agentswap-register-key-{}", std::process::id()));
        std::fs::write(&path, format!(" \n{canary}\n")).unwrap();
        let loaded = registration_signer(path.to_str(), Some("invalid".into())).unwrap().unwrap();
        assert_eq!(loaded.address(), expected.address());
        std::fs::remove_file(&path).unwrap();
        assert!(registration_signer(path.to_str(), Some(canary)).is_err());
    }
}
