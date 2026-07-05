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
        let resp = if resp.status() == reqwest::StatusCode::PAYMENT_REQUIRED {
            self.retry_get_with_payment(url, resp).await?
        } else {
            resp
        };
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
        let resp = if resp.status() == reqwest::StatusCode::PAYMENT_REQUIRED {
            self.retry_post_with_payment(&url, body, resp).await?
        } else {
            resp
        };
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

    async fn retry_get_with_payment(
        &self,
        url: &str,
        resp: reqwest::Response,
    ) -> Result<reqwest::Response> {
        let payment = self.payment_header(resp).await?;
        self.http
            .get(url)
            .header("X-PAYMENT", payment)
            .send()
            .await
            .map_err(|e| eyre!("x402 retry failed: {e}"))
    }

    async fn retry_post_with_payment(
        &self,
        url: &str,
        body: &serde_json::Value,
        resp: reqwest::Response,
    ) -> Result<reqwest::Response> {
        let payment = self.payment_header(resp).await?;
        self.http
            .post(url)
            .header("X-PAYMENT", payment)
            .json(body)
            .send()
            .await
            .map_err(|e| eyre!("x402 retry failed: {e}"))
    }

    async fn payment_header(&self, resp: reqwest::Response) -> Result<String> {
        let body = resp.text().await.unwrap_or_default();
        if !self.x402.enabled || (self.api_key.is_some() && !self.x402.prefer_x402) {
            return Err(eyre!("HTTP 402 Payment Required: {body}"));
        }
        let signer = self
            .x402_signer
            .clone()
            .ok_or_else(|| eyre!("HTTP 402 Payment Required but no x402 signer configured"))?;
        crate::x402::payment_header(&body, &self.x402, signer).await
    }
}
