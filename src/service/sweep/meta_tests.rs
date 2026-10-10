// Both quote backends reach the actual sweep-to-trade dry-run digest boundary.
// Loopback RPC verifies the proxy domain; HTTP fixtures detect accidental re-quotes.
use super::*;
use crate::{client::meta_tests, service::test_rpc::{TestRpc, ok}};
use alloy::{primitives::U256, sol_types::{SolCall, SolValue}};
use serde_json::json;
#[path = "../../client/test_server.rs"]
mod http;

fn rpc(chain: u64) -> TestRpc {
    TestRpc::start(move |body| {
        let result = match body["method"].as_str() {
            Some("eth_getBlockByNumber") => {
                let mut block = serde_json::to_value(alloy::rpc::types::Block::<alloy::rpc::types::Transaction>::default()).unwrap();
                block["timestamp"] = json!("0x64"); block
            }
            Some("eth_call") => {
                let tx = &body["params"][0];
                let data = tx["input"].as_str().or(tx["data"].as_str()).unwrap();
                let data = hex::decode(data.trim_start_matches("0x")).unwrap();
                let encoded = match &data[..4] {
                    s if s == UserProxyV6::policyOfCall::SELECTOR => (U256::from(200), U256::from(60), U256::from(1), U256::from(1)).abi_encode(),
                    s if s == UserProxyV6::agentTokenInfoCall::SELECTOR => (true, U256::from(1_000_000), U256::ZERO, U256::ZERO).abi_encode(),
                    s if s == discovery::BalanceReader::balanceOfCall::SELECTOR || s == sale::AllowanceReader::allowanceCall::SELECTOR => U256::from(1_000_000).abi_encode(),
                    s if s == order_types::Erc20Metadata::decimalsCall::SELECTOR => U256::from(6).abi_encode(),
                    s if s == order_types::Erc20Metadata::symbolCall::SELECTOR => "TEST".to_string().abi_encode(),
                    s if s == UserProxyV6::hashAgentOrderCall::SELECTOR => {
                        let call = UserProxyV6::hashAgentOrderCall::abi_decode(&data).unwrap();
                        assert_eq!(call.o.generation, 1);
                        assert_eq!(call.o.minOut, U256::from(990000));
                        assert_eq!(call.o.router, Address::repeat_byte(8));
                        order_types::signing_hash(&call.o, &order_types::proxy_domain(chain, tx["to"].as_str().unwrap().parse().unwrap())).abi_encode()
                    }
                    _ => panic!("unexpected call {}", hex::encode(&data[..4])),
                };
                json!(format!("0x{}", hex::encode(encoded)))
            }
            other => panic!("unexpected method {other:?}"),
        };
        Some(ok(body, result))
    })
}

async fn preview(chain: u64, url: &str) {
    let checked = json!({"router":Address::repeat_byte(8),"output":"1000000000000000000",
        "execution":{"target":Address::repeat_byte(8),"spender":Address::repeat_byte(9),"calldata":"0x12345678"}});
    let legacy = http::Server::start(vec![(200, String::new(), checked.to_string()), (500, String::new(), "must not requote".into())]);
    let meta = http::Server::start(vec![(200, String::new(), meta_tests::best().to_string()), (500, String::new(), "must not requote".into())]);
    let client = meta_tests::client(&legacy.url, &meta.url);
    let mut policy = tests::policy();
    let spend = intent::TokenPolicy { token: Address::repeat_byte(1).to_string(), allowed: true, cap: "1000000".into(), used: "0".into(), epoch_start: "0".into() };
    policy.tokens.push(spend.clone());
    let input = Input { chain_id: chain.to_string(), proxy: policy.proxy.clone(), receive: policy.tokens[0].token.clone(),
        max_usd: "1".into(), max_loss_bps: 100, tokens: vec![spend.token.clone()], dry_run: true, self_submit: true, via: Via::Market, wait: None, confirmed_generation: None, request_caps: Default::default(), start_premium_bps: DEFAULT_START_PREMIUM_BPS };
    let context = Context { client: &client, signer: Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap()),
        input, provider: evm::read_provider(url).unwrap(), owner: Address::repeat_byte(4),
        receive: token::Token { address: Address::repeat_byte(3).to_string(), symbol: "OUT".into(), decimals: 6 },
        prices: BTreeMap::from([(Address::repeat_byte(1), tests::price("1", true)), (Address::repeat_byte(3), tests::price("1", true))]),
        policy, max: amount::fixed("1", false).unwrap(), server_cap: None, wait: Wait::MCP, pacer: intent::Pacer::system() };
    let trade_input = sale::trade_input(&context, &spend, U256::from(1_000_000), U256::from(990000));
    let signer = context.signer.clone();
    let out = run(context).await.unwrap();
    out.check().unwrap();
    let row = &out.tokens[1];
    let trade = row.trade.as_ref().expect("sale reaches signing boundary");
    assert!(trade.dry_run && trade.signature.is_none() && trade.self_submit.is_none());
    assert_eq!(trade.order.min_out, "990000");
    assert_eq!(trade.order.amount_in, "1000000");
    assert_eq!(trade.order.generation, "1");
    assert_eq!(trade.digest.len(), 66);
    assert_eq!(row.floor_raw.as_deref(), Some("990000"));
    assert_eq!(trade.quote["execution"]["calldata"], "0x12345678");
    assert_eq!(trade.quote["execution"]["spender"], json!(Address::repeat_byte(9)));
    verify_binding(&client, signer, trade_input, trade.quote.clone()).await;
    assert_eq!(legacy.requests.lock().unwrap().len(), usize::from(chain != 56));
    let seen = meta.requests.lock().unwrap();
    assert_eq!(seen.len(), usize::from(chain == 56));
    if chain == 56 {
        let request: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(request["taker"], json!(Address::repeat_byte(6)));
    }
}

async fn verify_binding(client: &Client, signer: Arc<dyn Signer>, input: crate::service::trade::TradeInput, checked: serde_json::Value) {
    let body = json!({"chain_id":input.chain_id.parse::<u64>().unwrap(),"token_in":input.from,
        "token_out":input.to,"amount_in":input.amount,"slippage_bps":100,"taker":input.proxy});
    let pinned = client.clone().with_pinned_quote(body.clone(), checked);
    let result = crate::service::trade::execute_trade(&pinned, signer.clone(), input.clone(), false, Wait::MCP).await.unwrap();
    assert_eq!(result.order.min_out, "990000");
    for (field, value) in [("amount_in", json!("1000001")), ("chain_id", json!(1)),
        ("token_in", json!(Address::repeat_byte(10))), ("token_out", json!(Address::repeat_byte(10))),
        ("slippage_bps", json!(1)), ("taker", json!(Address::repeat_byte(10))), ("verify", json!(true))] {
        let mut changed = body.clone(); changed[field] = value;
        assert!(pinned.quote(&changed).await.unwrap_err().to_string().contains("differs"));
    }
    let mut capped = input.clone(); capped.max_amount = Some("999999".into());
    let error = crate::service::trade::execute_trade(&pinned, signer.clone(), capped, false, Wait::MCP).await.unwrap_err();
    assert!(error.to_string().contains("max-amount"));
    let mut changed = input; changed.proxy = Address::repeat_byte(10).to_string();
    let error = crate::service::trade::execute_trade(&pinned, signer, changed, false, Wait::MCP).await.unwrap_err();
    assert!(error.to_string().contains("differs"));
}

#[test]
fn bsc_and_legacy_sweep_execute_exact_checked_quote_to_digest() {
    const NAME: &str = "service::sweep::meta_tests::bsc_and_legacy_sweep_execute_exact_checked_quote_to_digest";
    if let Ok(chain) = std::env::var("META_FLOW_CHAIN") {
        let chain: u64 = chain.parse().unwrap();
        let url = std::env::var(format!("AGENTSWAP_RPC_URL_{chain}")).unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(preview(chain, &url)); return;
    }
    for chain in [56, 8453] {
        let rpc = rpc(chain);
        let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
            .env("META_FLOW_CHAIN", chain.to_string()).env(format!("AGENTSWAP_RPC_URL_{chain}"), &rpc.url).output().unwrap();
        assert!(output.status.success(), "{chain}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
    }
}
