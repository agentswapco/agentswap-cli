// MCP boundary tests for early validation and exact JSON string preservation.
// Exports: module-local tests.
// Deps: super MCP server and service DTOs.

use super::*;
use alloy::primitives::{Address, B256, Signature};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingSigner {
    calls: AtomicUsize,
}

#[async_trait]
impl Signer for CountingSigner {
    fn address(&self) -> Address { Address::ZERO }

    async fn sign_message(&self, _: &[u8]) -> Result<Signature> {
        Err(eyre::eyre!("unexpected signature"))
    }

    async fn sign_hash(&self, _: B256) -> Result<Signature> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(eyre::eyre!("unexpected signature"))
    }
}

#[tokio::test]
async fn malformed_mcp_trade_cap_rejects_before_signing() {
    for cap in ["not-an-amount", "12abc", "", "  ", "1_000"] {
        let signer = Arc::new(CountingSigner { calls: AtomicUsize::new(0) });
        let server = AgentSwapMcp::new(Config {
            client: Client::new("http://127.0.0.1:1", None),
            intent_client: Client::new("http://127.0.0.1:1", None),
            signer: Some(signer.clone()),
            allow_trade: true,
            trade_max_amount: Some(cap.to_string()),
        });
        let result = server.trade(Parameters(trade::TradeInput {
            chain_id: "base".to_string(), from: "USDC".to_string(), to: "WETH".to_string(),
            amount: "1".to_string(), slippage: None, min_out: Some("1".to_string()),
            max_amount: None, mode: "agent-order".to_string(),
            proxy: "0x2222222222222222222222222222222222222222".to_string(),
            nonce: Some("1".to_string()), deadline_secs: Some(120), dry_run: true,
            self_submit: false,
        })).await;
        let error = match result {
            Ok(_) => panic!("malformed operator cap {cap:?} must fail"),
            Err(error) => error,
        };
        assert!(error.contains("invalid MCP trade max-amount"), "cap {cap:?}: {error}");
        assert_eq!(signer.calls.load(Ordering::SeqCst), 0, "cap {cap:?} signed");
    }
}

#[test]
fn mcp_json_keeps_large_raw_amount_as_a_string() {
    let amount = "900719925474099300000000000000000000";
    let output = quote::QuoteOutput {
        request: quote::QuoteContext {
            chain_id: 8453,
            token_in: "0x1".to_string(),
            token_in_symbol: "USDC".to_string(),
            token_in_decimals: 6,
            token_out: "0x2".to_string(),
            token_out_symbol: "WETH".to_string(),
            token_out_decimals: 18,
            amount_in: amount.to_string(),
        },
        response: serde_json::json!({}),
    };
    let json = serde_json::to_value(output).expect("MCP JSON");
    assert_eq!(json["request"]["amount_in"], amount);
}
