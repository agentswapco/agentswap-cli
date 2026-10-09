// HTTP client module for AgentSwap API calls.
// Exports: Client.
// Deps: reqwest, serde_json, eyre, crate::{routes, signer, x402}.
mod transport;
#[cfg(test)]
mod redirect_tests;
#[cfg(test)]
mod test_server;
#[cfg(test)]
mod pinned_quote_tests;

use eyre::Result;
use std::sync::Arc;

/// Thin HTTP client for the sr-service API.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    x402: crate::x402::Config,
    x402_signer: Option<Arc<dyn crate::signer::Signer>>,
    pinned_quote: Option<(serde_json::Value, serde_json::Value)>,
}

impl Client {
    pub fn new(base_url: &str, api_key: Option<String>) -> Self {
        Self {
            // Custom API/payment headers are not stripped by reqwest on redirects.
            // This also covers payment retries enabled later by with_x402.
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("HTTP client initialization failed"),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            x402: crate::x402::Config::disabled(),
            x402_signer: None,
            pinned_quote: None,
        }
    }

    pub fn with_x402(
        mut self,
        config: crate::x402::Config,
        signer: Option<Arc<dyn crate::signer::Signer>>,
    ) -> Self {
        self.x402 = config;
        self.x402_signer = signer;
        self
    }

    /// Bind one cloned client to a checked quote request; mismatches fail before any HTTP call.
    pub(crate) fn with_pinned_quote(mut self, request: serde_json::Value, response: serde_json::Value) -> Self {
        self.pinned_quote = Some((request, response));
        self
    }

    // --- Consumer endpoints ---

    pub async fn quote(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        if let Some((request, response)) = &self.pinned_quote {
            eyre::ensure!(body == request, "trade request differs from the checked sweep quote");
            return Ok(response.clone());
        }
        self.post(crate::routes::QUOTE, body).await
    }

    pub async fn health(&self) -> Result<serde_json::Value> {
        self.get(crate::routes::HEALTH).await
    }

    pub async fn tokens(&self) -> Result<serde_json::Value> {
        self.get(crate::routes::TOKENS).await
    }

    pub async fn pool(&self, chain_id: u64, address: &str) -> Result<serde_json::Value> {
        self.get(&crate::routes::pool(chain_id, address)).await
    }

    pub async fn challenge(&self, address: &str) -> Result<serde_json::Value> {
        self.post(
            crate::routes::AUTH_CHALLENGE,
            &serde_json::json!({ "address": address }),
        )
        .await
    }

    pub async fn register_key(
        &self,
        address: &str,
        nonce: &str,
        signature: &str,
    ) -> Result<serde_json::Value> {
        self.post(
            crate::routes::AUTH_REGISTER,
            &serde_json::json!({
                "address": address,
                "nonce": nonce,
                "signature": signature,
            }),
        )
        .await
    }

    pub async fn key_info(&self) -> Result<serde_json::Value> {
        self.get(crate::routes::AUTH_KEY_INFO).await
    }

    pub async fn pricing(&self) -> Result<serde_json::Value> {
        self.get(crate::routes::AUTH_PRICING).await
    }

    pub async fn quota_claim(&self, chain_id: u64, tx_hash: &str) -> Result<serde_json::Value> {
        self.post(
            crate::routes::AUTH_QUOTA_CLAIM,
            &serde_json::json!({
                "chain_id": chain_id,
                "tx_hash": tx_hash,
            }),
        )
        .await
    }

    // --- Admin/investigation endpoints ---

    pub async fn status(&self) -> Result<serde_json::Value> {
        self.get(crate::routes::STATUS).await
    }

    pub async fn quote_lookup(&self, hash: &str) -> Result<serde_json::Value> {
        self.get(&crate::routes::QUOTE_LOOKUP.replace(":hash", hash))
            .await
    }

    pub async fn announce_intent(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        self.post(crate::routes::INTENT_ANNOUNCE, body).await
    }
}
