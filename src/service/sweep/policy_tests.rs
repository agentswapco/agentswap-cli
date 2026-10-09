// Read-only sweep policy regressions using counted JSON-RPC methods.
// Covers membership, budgets, expiry, factory identity and the command's no-log path.
use super::*;
use crate::service::test_rpc::{TestRpc, ok};
use alloy::{primitives::U256, sol_types::{SolCall, SolValue}};
use serde_json::json;

fn input() -> Input {
    Input { chain_id: "8453".into(), proxy: Address::repeat_byte(6).to_string(), receive: "USDC".into(),
        tokens: vec![Address::repeat_byte(1).to_string(), Address::repeat_byte(1).to_string()],
        max_usd: "5".into(), max_loss_bps: 100, dry_run: true, self_submit: false, via: Via::Market, wait: None }
}

fn fixture(allowed: bool, receive_allowed: bool, cap: u64, expiry: u64, mask: u8, wrong_proxy: bool) -> TestRpc {
    TestRpc::start(move |body| {
        let result = match body["method"].as_str().unwrap() {
            "eth_getBlockByNumber" => {
                let mut block = serde_json::to_value(alloy::rpc::types::Block::<alloy::rpc::types::Transaction>::default()).unwrap();
                block["timestamp"] = json!("0x64"); block
            }
            "eth_call" => {
                let tx = &body["params"][0];
                let data = hex::decode(tx["input"].as_str().or(tx["data"].as_str()).unwrap().trim_start_matches("0x")).unwrap();
                let encoded = match &data[..4] {
                    s if s == UserProxyV6::ownerCall::SELECTOR => Address::repeat_byte(4).abi_encode(),
                    s if s == order_types::UserProxyFactoryV6::proxyOfCall::SELECTOR => Address::repeat_byte(if wrong_proxy { 7 } else { 6 }).abi_encode(),
                    s if s == UserProxyV6::policyOfCall::SELECTOR =>
                        (U256::from(expiry), U256::from(60), U256::from(mask), U256::from(7)).abi_encode(),
                    s if s == UserProxyV6::agentTokenInfoCall::SELECTOR => {
                        let call = UserProxyV6::agentTokenInfoCall::abi_decode(&data).unwrap();
                        let spend = call.token == Address::repeat_byte(1);
                        (if spend { allowed } else { receive_allowed }, U256::from(if spend { cap } else { 0 }), U256::from(3), U256::from(90)).abi_encode()
                    }
                    s if s == discovery::BalanceReader::balanceOfCall::SELECTOR => U256::ZERO.abi_encode(),
                    s if s == sale::AllowanceReader::allowanceCall::SELECTOR => U256::from(100).abi_encode(),
                    _ => panic!("unexpected selector"),
                };
                json!(format!("0x{}", hex::encode(encoded)))
            }
            other => panic!("unexpected RPC method {other}"),
        };
        Some(ok(body, result))
    })
}

#[tokio::test]
async fn explicit_policy_reads_membership_budgets_and_generation_without_logs() {
    let receive = token::from_registry("USDC", 8453).unwrap().address.parse().unwrap();
    for (allowed, cap) in [(true, 100), (false, 100), (true, 0)] {
        let rpc = fixture(allowed, true, cap, 200, 1, false);
        let provider = evm::read_provider(&rpc.url).unwrap();
        let policy = read_policy(&provider, &input(), Address::repeat_byte(4), Address::repeat_byte(5), receive).await.unwrap();
        assert_eq!((policy.expiry.as_str(), policy.epoch_len.as_str(), policy.action_mask.as_str(), policy.generation.as_str()), ("200", "60", "1", "7"));
        assert_eq!(policy.tokens.len(), 2, "duplicate explicit tokens read once");
        let spend = policy.tokens.iter().find(|t| t.token == format!("{:?}", Address::repeat_byte(1))).unwrap();
        assert_eq!(spend.allowed, allowed);
        assert_eq!(spend.cap, cap.to_string());
        assert_eq!((spend.used.as_str(), spend.epoch_start.as_str()), ("3", "90"));
        assert!(validate_policy(&policy, receive, 100, Via::Market).is_ok());
        assert_eq!(rpc.called("eth_call"), 4);
        assert_eq!(rpc.called("eth_getLogs"), 0);
    }
}

#[tokio::test]
async fn sweep_command_reads_only_and_refuses_invalid_policy() {
    let signer = Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
    let app = TestRpc::start(|_| Some(json!({"prices":{}})));
    for (allowed, receive_allowed, cap, expiry, mask, wrong_proxy, error) in [
        (true, true, 0, 200, 1, false, None),
        (true, true, 100, 200, 1, false, None),
        (false, true, 100, 200, 1, false, None),
        (true, false, 100, 200, 1, false, Some("receive token")),
        (true, true, 100, 100, 1, false, Some("expired")),
        (true, true, 100, 200, 4, false, Some("market action")),
        (true, true, 100, 200, 1, true, Some("factory proxy")),
    ] {
        let rpc = fixture(allowed, receive_allowed, cap, expiry, mask, wrong_proxy);
        let provider = evm::read_provider(&rpc.url).unwrap();
        let result = read(&Client::new(&app.url, None), signer.clone(), input(), provider, &app.url, None, Wait::MCP).await;
        if let Some(error) = error { assert!(result.unwrap_err().to_string().contains(error)); }
        else {
            let output = result.unwrap();
            assert_eq!(output.tokens.len(), if allowed { 2 } else { 1 });
            assert!(output.tokens.iter().all(|r| r.outcome == "skipped"));
            assert!(output.tokens.iter().all(|r| matches!(r.reason.as_deref(), Some("zero" | "receive_token"))));
        }
        assert_eq!(rpc.called("eth_getLogs"), 0);
        assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
    }
}

#[tokio::test]
async fn sweep_rejects_empty_tokens_before_rpc() {
    let signer = Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
    let mut request = input(); request.tokens.clear();
    let error = sweep(&Client::new("http://127.0.0.1:1", None), signer, request, false, None, Wait::MCP).await.unwrap_err();
    assert!(error.to_string().contains("at least one --token"));
}
