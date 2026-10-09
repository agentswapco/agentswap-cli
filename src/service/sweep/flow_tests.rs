// Sweep service flow tests with an isolated RPC environment and independent price fixtures.
// Exercises fresh reads, route checks, execute_trade previews and failures without broadcasting.
use super::*;
use crate::service::{test_rpc::{TestRpc, ok}};
use alloy::{primitives::U256, sol_types::{SolCall, SolValue}};
use serde_json::json;

fn fixture(output: u64, generation: u64, balance: u64) -> TestRpc {
    TestRpc::start(move |body| {
        let result = match body["method"].as_str() {
            None => return Some(json!({"router": Address::repeat_byte(8), "output":output.to_string(), "calldata":"0x"})),
            Some("eth_getBlockByNumber") => {
                let mut block = serde_json::to_value(alloy::rpc::types::Block::<alloy::rpc::types::Transaction>::default()).unwrap();
                block["timestamp"] = json!("0x64"); block
            }
            Some("eth_call") => {
                let tx = &body["params"][0];
                let data = tx["input"].as_str().or(tx["data"].as_str()).unwrap();
                let data = hex::decode(data.trim_start_matches("0x")).unwrap();
                let selector = &data[..4];
                let encoded = if selector == UserProxyV6::policyOfCall::SELECTOR {
                    (U256::from(200), U256::from(60), U256::from(1), U256::from(generation)).abi_encode()
                } else if selector == UserProxyV6::agentTokenInfoCall::SELECTOR {
                    (true, U256::from(1_000_000), U256::ZERO, U256::ZERO).abi_encode()
                } else if selector == discovery::BalanceReader::balanceOfCall::SELECTOR {
                    U256::from(balance).abi_encode()
                } else if selector == sale::AllowanceReader::allowanceCall::SELECTOR { U256::from(1_000_000).abi_encode() }
                else if selector == order_types::Erc20Metadata::decimalsCall::SELECTOR { U256::from(6).abi_encode() }
                else if selector == order_types::Erc20Metadata::symbolCall::SELECTOR { "TEST".to_string().abi_encode() }
                else if selector == UserProxyV6::hashAgentOrderCall::SELECTOR {
                    let call = UserProxyV6::hashAgentOrderCall::abi_decode(&data).unwrap();
                    order_types::signing_hash(&call.o, &order_types::proxy_domain(8453, tx["to"].as_str().unwrap().parse().unwrap())).abi_encode()
                } else { panic!("unexpected call {}", hex::encode(selector)); };
                json!(format!("0x{}", hex::encode(encoded)))
            }
            other => panic!("unexpected method {other:?}"),
        };
        Some(ok(body, result))
    })
}

async fn flow_case(url: &str) {
    let scenario = std::env::var("SWEEP_FLOW_CASE").unwrap();
    let client = Client::new(url, None);
    let from = token::from_registry("USDC", 8453).unwrap();
    let mut receive = token::from_registry("WETH", 8453).unwrap();
    if scenario == "zero_floor" { receive.decimals = 0; }
    let mut policy = tests::policy();
    let mut spend = policy.tokens[0].clone();
    spend.token = from.address.clone(); spend.cap = "1000000".into();
    policy.tokens[0].token = receive.address.clone();
    policy.tokens.push(spend);
    let signer = Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
    let input = Input { chain_id: "8453".into(), proxy: policy.proxy.clone(), receive: receive.address.clone(), max_usd: "1".into(),
        max_loss_bps: 100, tokens: vec![from.address.clone()], dry_run: !matches!(scenario.as_str(), "zero_floor" | "large_holding"), self_submit: true };
    let provider = evm::read_provider(url).unwrap();
    assert_eq!(now(&provider).await.unwrap(), 100);
    let prices = BTreeMap::from([(from.address.parse().unwrap(), tests::price("1", true)), (receive.address.parse().unwrap(), tests::price("1", true))]);
    let output = run(Context { client: &client, signer: signer.clone(), input: input.clone(), provider, owner: Address::repeat_byte(4),
        receive, prices, policy, max: amount::fixed("1", false).unwrap(), server_cap: (!input.dry_run).then_some(U256::ZERO), wait: Wait::MCP }).await.unwrap();
    assert_eq!(output.tokens.len(), 2);
    assert_eq!(output.tokens[0].reason.as_deref(), Some("receive_token"));
    let row = &output.tokens[1];
    match scenario.as_str() {
        "preview" => {
            assert_eq!(row.outcome, "skipped", "{row:?}");
            assert_eq!(row.reason.as_deref(), Some("dry_run"));
            let result = row.trade.as_ref().unwrap();
            assert!(result.signature.is_none() && result.self_submit.is_none());
            assert_eq!(result.order.amount_in, "1000000");
            assert_eq!(result.order.min_out, "990000000000000000");
            assert!(output.check().is_ok());
        }
        "zero_floor" | "large_holding" => {
            assert_eq!(row.reason.as_deref(), Some(if scenario == "zero_floor" { "below_floor" } else { "over_max_usd" }), "{row:?}");
            assert!(row.trade.is_none(), "must skip before execute_trade");
            assert!(row.quote_out_raw.is_none(), "must skip before quoting");
            if scenario == "zero_floor" { assert_eq!(row.floor_raw.as_deref(), Some("0")); }
        }
        "below" => assert_eq!(row.reason.as_deref(), Some("below_floor")),
        "no_route" => assert_eq!(row.reason.as_deref(), Some("no_route")),
        "changed" => { assert_eq!(row.outcome, "failed"); assert!(row.error.as_ref().unwrap().contains("changed")); }
        _ => panic!("bad scenario"),
    }
    let mut invalid = input.clone(); invalid.max_loss_bps = 10001;
    assert!(sweep(&client, signer.clone(), invalid.clone(), true, None, Wait::MCP).await.is_err());
    assert!(crate::commands::sweep::run(&client, signer, invalid, false, None, true).await.is_err());
}

#[test]
fn sweep_flow_preview_routes_and_policy_change() {
    const NAME: &str = "service::sweep::flow_tests::sweep_flow_preview_routes_and_policy_change";
    if std::env::var("SWEEP_FLOW_CASE").is_ok() {
        let url = std::env::var("AGENTSWAP_RPC_URL_8453").unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(flow_case(&url)); return;
    }
    for (name, output, generation) in [("preview", 1_000_000_000_000_000_000, 1), ("below", 1, 1), ("no_route", 0, 1), ("changed", 1, 2)] {
        let rpc = fixture(output, generation, 1_000_000);
        let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
            .env("SWEEP_FLOW_CASE", name).env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
        assert!(output.status.success(), "{name}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
        assert_eq!(rpc.called("eth_getLogs"), 0);
        assert_eq!(rpc.called(""), usize::from(name != "changed"), "one checked quote per attempted sale");
    }
}

#[test]
fn sweep_zero_floor_skips_before_execute_trade() {
    guarded_flow("zero_floor", "service::sweep::flow_tests::sweep_zero_floor_skips_before_execute_trade", 1);
}

#[test]
fn sweep_max_usd_checks_whole_holding_under_recurring_cap() {
    guarded_flow("large_holding", "service::sweep::flow_tests::sweep_max_usd_checks_whole_holding_under_recurring_cap", 2_000_000);
}

fn guarded_flow(scenario: &str, name: &str, balance: u64) {
    if std::env::var("SWEEP_FLOW_CASE").is_ok() {
        let url = std::env::var("AGENTSWAP_RPC_URL_8453").unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(flow_case(&url)); return;
    }
    let rpc = fixture(1_000_000_000_000_000_000, 1, balance);
    let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", name, "--nocapture"])
        .env("SWEEP_FLOW_CASE", scenario).env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
    assert!(output.status.success(), "{scenario}: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(rpc.called(""), 0, "no quote binding or execute_trade");
    assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
    assert_eq!(rpc.called("eth_getLogs"), 0);
}
