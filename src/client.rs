// HTTP client module for AgentSwap API calls.
// Exports: Client.
// Deps: reqwest, serde_json, eyre, crate::routes.
mod transport;

use eyre::Result;

/// Thin HTTP client for the sr-service API.
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl Client {
    pub fn new(base_url: &str, api_key: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        }
    }

    // --- Consumer endpoints ---

    pub async fn quote(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
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
}
