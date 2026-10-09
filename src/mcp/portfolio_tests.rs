// Configured MCP portfolio boundary against a payable HTTP 402 quote challenge.
// Uses isolated RPC environment, a counting inert signer and loopback HTTP fixtures.
use super::*;
use alloy::primitives::{Address, B256, Signature, U256};
use std::sync::atomic::{AtomicUsize, Ordering};
use serde_json::json;
#[path = "../client/test_server.rs"]
mod http;

struct CountingSigner(AtomicUsize);

#[async_trait::async_trait]
impl Signer for CountingSigner {
    fn address(&self) -> Address { Address::ZERO }
    async fn sign_message(&self, _: &[u8]) -> Result<Signature> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Signature::new(U256::ZERO, U256::ZERO, false))
    }
    async fn sign_hash(&self, _: B256) -> Result<Signature> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Signature::new(U256::ZERO, U256::ZERO, false))
    }
}

async fn configured_client_case(portfolio_only: bool) {
    let app = http::Server::start(vec![
        (200, String::new(), json!({"chainId":4663, "owner":Address::repeat_byte(4),
            "indexed":false, "truncated":false, "tokens":[]}).to_string()),
        (200, String::new(), json!({"prices":{}}).to_string()),
    ]);
    if portfolio_only { crate::routes::TEST_APP_ORIGIN.set(app.url.clone()).unwrap(); }
    let signer = Arc::new(CountingSigner(AtomicUsize::new(0)));
    let challenge = json!({"accepts":[{"scheme":"exact", "network":"8453",
        "asset":crate::tokens::resolve_token("USDC", 8453).unwrap().0,
        "payTo":Address::ZERO, "maxAmountRequired":"1"}]}).to_string();
    let http = http::Server::start(vec![(402, String::new(), challenge.clone()),
        (if portfolio_only { 402 } else { 200 }, String::new(), if portfolio_only { challenge } else { json!({"output":"123"}).to_string() }),
        (200, String::new(), json!({"output":"123"}).to_string())]);
    let client = Client::new(&http.url, None).with_x402(crate::x402::Config {
        enabled: true, prefer_x402: true, chain_id: 8453, max_amount: "1".into(), asset: "USDC".into(),
    }, Some(signer.clone()));
    if portfolio_only {
        let server = AgentSwapMcp::new(Config { client: client.clone(), intent_client: client,
            signer: Some(signer.clone()), allow_trade: false, trade_max_amount: None });
        let Json(output) = server.portfolio(Parameters(portfolio::tests::input("4663"))).await.unwrap();
        assert_eq!((signer.0.load(Ordering::SeqCst), http.requests.lock().unwrap().len()),
            (0, 2), "unindexed wallet, empty prices: one catalog and one quote challenge, no retry");
        assert_eq!(format!("{:?}", output.wallet_tokens), "Unindexed");
        let requests = app.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].target.starts_with("/api/wallet-tokens?"));
        assert!(requests[1].target.starts_with("/api/prices?"));
        assert!(requests.iter().all(|r| r.method == "GET" && !r.paid && !r.keyed));
        let requests = http.requests.lock().unwrap();
        assert!(requests[0].target.starts_with("/api/tokens"));
        assert_eq!(requests[1].target, "/quote");
        assert!(requests.iter().all(|r| !r.paid && !r.keyed));
        assert_eq!(output.tokens[0].status, "quote_failed");
        assert!(output.tokens[0].error.as_deref().unwrap().contains("HTTP 402 Payment Required"));
        assert!(output.tokens[0].quote_out_raw.is_none());
    } else {
        assert_eq!(client.quote(&json!({})).await.unwrap()["output"], "123");
        assert_eq!(signer.0.load(Ordering::SeqCst), 1);
        assert_eq!(http.requests.lock().unwrap().len(), 2);
        assert!(http.requests.lock().unwrap()[1].paid);
        signer.sign_message(b"counter coverage").await.unwrap();
        assert_eq!(signer.0.load(Ordering::SeqCst), 2);
    }
    let requests = http.requests.lock().unwrap();
    assert!(!requests[0].paid && !requests[0].keyed);
    assert_eq!(requests[0].method, if portfolio_only { "GET" } else { "POST" });
    assert!(serde_json::from_str::<serde_json::Value>(&requests[usize::from(portfolio_only)].body).is_ok());
}

#[test]
fn portfolio_configured_x402_never_signs_or_retries() {
    const CASE: &str = "mcp::portfolio_tests::portfolio_configured_x402_never_signs_or_retries";
    if std::env::var("AGENTSWAP_PORTFOLIO_TEST_CASE").as_deref() == Ok(CASE) {
        tokio::runtime::Runtime::new().unwrap().block_on(configured_client_case(true));
        return;
    }
    let rpc = portfolio::tests::fixture(false, false, false, 6);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CASE, "--nocapture"]).env("AGENTSWAP_PORTFOLIO_TEST_CASE", CASE)
        .env("AGENTSWAP_RPC_URL_4663", &rpc.url).output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(rpc.called("eth_getLogs"), 0);
}

#[tokio::test]
async fn configured_x402_control_can_sign_and_retry() {
    configured_client_case(false).await;
}
