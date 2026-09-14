// Actual HTTP redirect and x402 retry regressions; run only on remote test hosts.
// Exports: tests for credential containment, negative controls, and API behavior.
// Deps: Client, loopback Server, and an inert signer (no usable payment signature).

use super::{Client, test_server::Server};
use alloy::primitives::{Address, B256, Signature, U256};
use serde_json::{Value, json};
use std::sync::Arc;

struct InertSigner;

#[async_trait::async_trait]
impl crate::signer::Signer for InertSigner {
    fn address(&self) -> Address { Address::ZERO }
    async fn sign_message(&self, _: &[u8]) -> eyre::Result<Signature> {
        eyre::bail!("fixture does not sign messages")
    }
    async fn sign_hash(&self, _: B256) -> eyre::Result<Signature> {
        Ok(Signature::new(U256::ZERO, U256::ZERO, false))
    }
}

fn challenge() -> String {
    json!({"accepts": [{"scheme": "exact", "network": "8453",
        "asset": crate::tokens::resolve_token("USDC", 8453).unwrap().0,
        "payTo": Address::ZERO.to_string(), "maxAmountRequired": "1"}]}).to_string()
}

fn paid_client(url: &str) -> Client {
    Client::new(url, None).with_x402(crate::x402::Config {
        enabled: true, prefer_x402: false, chain_id: 8453,
        max_amount: "1".into(), asset: "USDC".into(),
    }, Some(Arc::new(InertSigner)))
}

async fn request(client: &Client, post: bool) -> eyre::Result<Value> {
    if post { client.quote(&json!({"fixture": true})).await }
    else { client.health().await }
}

#[tokio::test]
async fn default_redirect_policy_negative_control_exposes_custom_headers() {
    let destination = Server::start(vec![(200, String::new(), "{}".into())]);
    let origin = Server::start(vec![(307, format!("Location: {}/sink\r\n", destination.url), "{}".into())]);
    reqwest::Client::new().post(&origin.url)
        .header("x-api-key", "inert-api-canary").header("X-PAYMENT", "inert-payment-canary")
        .json(&json!({"fixture": true})).send().await.unwrap();
    let seen = destination.requests.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].keyed && seen[0].paid);
    assert_eq!(seen[0].method, "POST");
}

#[tokio::test]
async fn api_key_redirects_never_reach_another_destination() {
    for status in [301, 302, 303, 307, 308] {
        for post in [false, true] {
            let destination = Server::start(vec![(200, String::new(), "{}".into())]);
            let origin = Server::start(vec![(status, format!("Location: {}/sink\r\n", destination.url), "{}".into())]);
            let result = request(&Client::new(&origin.url, Some("inert-api-canary".into())), post).await;
            assert!(destination.requests.lock().unwrap().is_empty(), "credential escaped on {status}, post={post}");
            assert!(result.is_err(), "redirect must fail");
            assert!(origin.requests.lock().unwrap()[0].keyed);
        }
    }
}

#[tokio::test]
async fn payment_retry_redirects_never_reach_another_destination() {
    for status in [301, 302, 303, 307, 308] {
        for post in [false, true] {
            let destination = Server::start(vec![(200, String::new(), "{}".into())]);
            let origin = Server::start(vec![(402, String::new(), challenge()),
                (status, format!("Location: {}/sink\r\n", destination.url), "{}".into())]);
            let result = request(&paid_client(&origin.url), post).await;
            assert!(destination.requests.lock().unwrap().is_empty(), "payment escaped on {status}, post={post}");
            assert!(result.is_err());
            let seen = origin.requests.lock().unwrap();
            assert_eq!(seen.len(), 2);
            assert!(!seen[0].paid && seen[1].paid);
        }
    }
}

#[tokio::test]
async fn direct_api_and_single_payment_retry_preserve_method_body_and_errors() {
    for post in [false, true] {
        for status in [200, 401, 402, 500] {
            let server = Server::start(vec![(status, String::new(), "{}".into())]);
            let result = request(&Client::new(&server.url, Some("inert-api-canary".into())), post).await;
            assert_eq!(result.is_ok(), status == 200);
            let seen = server.requests.lock().unwrap();
            assert_eq!(seen.len(), 1);
            assert!(seen[0].keyed);
        }
        for status in [200, 402] {
            let server = Server::start(vec![(402, String::new(), challenge()), (status, String::new(), "{}".into())]);
            let result = request(&paid_client(&server.url), post).await;
            assert_eq!(result.is_ok(), status == 200);
            let seen = server.requests.lock().unwrap();
            assert_eq!(seen.len(), 2);
            assert!(seen[1].paid && !seen[1].keyed);
            assert_eq!(seen[1].method, if post { "POST" } else { "GET" });
            assert_eq!(seen[0].body, seen[1].body);
        }
    }
    use crate::signer::Signer;
    assert!(InertSigner.sign_message(b"fixture").await.is_err());
}
