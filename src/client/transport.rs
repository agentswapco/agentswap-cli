// HTTP transport helpers backing the AgentSwap client.
// Exports: internal Client get/post helpers.
// Deps: super::Client, reqwest, serde_json, eyre.

use super::Client;
use eyre::{eyre, Result};
use serde::de::DeserializeOwned;

impl Client {
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{path}", self.base_url);
        self.get_raw(&url).await
    }

    pub(super) async fn get_raw<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let mut req = self.http.get(url);
        if let Some(key) = &self.api_key {
            req = req.header("x-api-key", key);
        }
        let resp = req.send().await.map_err(|e| eyre!("request failed: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return Err(eyre!("HTTP 401 Unauthorized — set SR_API_KEY env var or register with: agentswap register"));
            }
            return Err(eyre!("HTTP {status}: {body}"));
        }
        resp.json().await.map_err(|e| eyre!("parse failed: {e}"))
    }

    pub(super) async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let url = format!("{}{path}", self.base_url);
        let mut req = self.http.post(&url).json(body);
        if let Some(key) = &self.api_key {
            req = req.header("x-api-key", key);
        }
        let resp = req.send().await.map_err(|e| eyre!("request failed: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return Err(eyre!("HTTP 401 Unauthorized — set SR_API_KEY env var or register with: agentswap register"));
            }
            return Err(eyre!("HTTP {status}: {body}"));
        }
        resp.json().await.map_err(|e| eyre!("parse failed: {e}"))
    }
}
