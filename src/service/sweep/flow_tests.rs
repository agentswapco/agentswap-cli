// Sweep service flow tests with an isolated RPC environment and independent price fixtures.
// Exercises fresh reads, route checks, execute_trade previews and failures without broadcasting.
use super::*;
use crate::service::{test_rpc::{TestRpc, ok}};
use alloy::{primitives::U256, sol_types::{SolCall, SolValue}};
use serde_json::json;
#[path = "../../client/test_server.rs"]
mod quote_server;

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
    let client = Client::new(&std::env::var("SWEEP_QUOTE_URL").unwrap_or_else(|_| url.into()), None);
    let from = token::from_registry("USDC", 8453).unwrap();
    let mut receive = token::from_registry("WETH", 8453).unwrap();
    if scenario == "zero_floor" { receive.decimals = 0; }
    let mut policy = tests::policy();
    let mut spend = policy.tokens[0].clone();
    spend.token = from.address.clone(); spend.cap = "1000000".into();
    policy.tokens[0].token = receive.address.clone();
    policy.tokens.push(spend.clone());
    let signer = Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
    let input = Input { chain_id: "8453".into(), proxy: policy.proxy.clone(), receive: receive.address.clone(), max_usd: "1".into(),
        max_loss_bps: 100, tokens: vec![from.address.clone()], dry_run: !matches!(scenario.as_str(), "zero_floor" | "large_holding"), self_submit: true, via: Via::Market, wait: None, confirmed_generation: None, request_caps: Default::default(), start_premium_bps: DEFAULT_START_PREMIUM_BPS };
    let provider = evm::read_provider(url).unwrap();
    assert_eq!(now(&provider).await.unwrap(), 100);
    let mut prices = BTreeMap::from([(from.address.parse().unwrap(), tests::price("1", true)), (receive.address.parse().unwrap(), tests::price("1", true))]);
    if scenario == "oneinch_preview" {
        let app = quote_server::Server::start(vec![(200, String::new(), json!({"prices":{
            from.address.clone():{"priceUsd":1,"source":"1inch","change24h":null,"asOfSec":0},
            receive.address.clone():{"priceUsd":1,"source":"1inch","change24h":null,"asOfSec":0}
        }}).to_string())]);
        prices = prices::prices(8453, &[from.address.parse().unwrap(), receive.address.parse().unwrap()], &app.url).await;
    }
    let context = Context { client: &client, signer: signer.clone(), input: input.clone(), provider, owner: Address::repeat_byte(4),
        receive, prices, policy, max: amount::fixed("1", false).unwrap(), server_cap: (!input.dry_run).then_some(U256::ZERO), wait: Wait::MCP };
    if scenario == "pinned_preview" { pinned_sale(context, &spend).await; return; }
    let output = run(context).await.unwrap();
    assert_eq!(output.tokens.len(), 2);
    assert_eq!(output.tokens[0].reason.as_deref(), Some("receive_token"));
    let row = &output.tokens[1];
    match scenario.as_str() {
        "preview" | "quote_preview" | "oneinch_preview" => {
            assert_eq!(row.outcome, "skipped", "{row:?}");
            assert_eq!(row.reason.as_deref(), Some("dry_run"));
            let result = row.trade.as_ref().unwrap();
            assert!(result.signature.is_none() && result.self_submit.is_none());
            assert_eq!(result.digest.len(), 66);
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
        "no_route" | "quote_no_route" => {
            assert_eq!(row.reason.as_deref(), Some("no_route"));
            assert!(row.error.is_none());
        }
        "quote_error" => assert_quote_failure(row, &output),
        "changed" => { assert_eq!(row.outcome, "failed"); assert!(row.error.as_ref().unwrap().contains("changed")); }
        _ => panic!("bad scenario"),
    }
    let mut invalid = input.clone(); invalid.max_loss_bps = 10001;
    assert!(sweep(&client, signer.clone(), invalid.clone(), true, None, Wait::MCP).await.is_err());
    assert!(sweep(&client, signer, invalid, false, None, Wait::CLI).await.is_err());
}

#[test]
fn sweep_sale_reaches_digest_with_actual_pinned_request() {
    const NAME: &str = "service::sweep::flow_tests::sweep_sale_reaches_digest_with_actual_pinned_request";
    if std::env::var("SWEEP_FLOW_CASE").is_ok() {
        let url = std::env::var("AGENTSWAP_RPC_URL_8453").unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(flow_case(&url)); return;
    }
    let rpc = fixture(1_000_000_000_000_000_000, 1, 1_000_000);
    let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
        .env("SWEEP_FLOW_CASE", "pinned_preview").env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(rpc.called(""), 1, "one HTTP quote for the sale and replay");
    assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
}

async fn pinned_sale(context: Context<'_>, budget: &crate::service::intent::TokenPolicy) {
    let input = sale::trade_input(&context, budget, U256::from(1_000_000), U256::from(990_000_000_000_000_000u64));
    let request = crate::service::quote::QuoteInput { chain_id: input.chain_id.clone(), from: input.from.clone(),
        to: input.to.clone(), amount: input.amount.clone(), slippage: input.slippage, verify: false, taker: Some(input.proxy.clone()) };
    let from = token::from_registry("USDC", 8453).unwrap();
    let (body, _) = crate::service::quote::build_quote_body(&request, 8453, &from, &context.receive);
    let checked = crate::service::quote::quote(context.client, request).await.unwrap();
    let pinned = context.client.clone().with_pinned_quote(body, checked.response);
    let signer = context.signer.clone();
    let context = Context { client: &pinned, ..context };
    let output = run(context).await.unwrap();
    output.check().unwrap();
    let sale = output.tokens[1].trade.as_ref().expect("sale reaches the dry-run signing boundary");
    assert!(sale.dry_run && sale.signature.is_none());
    assert_eq!(sale.digest.len(), 66);
    let replay = crate::service::trade::execute_trade(&pinned, signer, input, false, Wait::MCP).await
        .expect("sale input must match the actual checked request through execute_trade");
    assert_eq!(replay.order.amount_in, sale.order.amount_in);
    assert_eq!(replay.order.min_out, sale.order.min_out);
    assert!(replay.dry_run && replay.signature.is_none());
    assert_eq!(replay.digest.len(), 66);
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

#[test]
fn sweep_quote_request_omits_verify() {
    quote_flow("quote_preview", "service::sweep::flow_tests::sweep_quote_request_omits_verify", 200,
        &json!({"router": Address::repeat_byte(8), "output":"1000000000000000000", "calldata":"0x"}).to_string(), "");
}

#[test]
fn sweep_quote_pinned_binding_rejects_differing_requote() {
    quote_flow("quote_preview", "service::sweep::flow_tests::sweep_quote_pinned_binding_rejects_differing_requote", 200,
        &json!({"router": Address::repeat_byte(8), "output":"1000000000000000000", "calldata":"0x"}).to_string(), "");
}

#[test]
fn sweep_quote_404_no_executable_route_is_no_route() {
    quote_flow("quote_no_route", "service::sweep::flow_tests::sweep_quote_404_no_executable_route_is_no_route",
        404, "no executable route found", "");
}

#[test]
fn sweep_quote_other_errors_keep_redacted_text() {
    for (status, body, expected) in [
        (404, "unknown endpoint", "HTTP 404 Not Found: unknown endpoint"),
        (401, "invalid key", "HTTP 401 Unauthorized"),
        (429, "rate limited", "HTTP 429 Too Many Requests: rate limited"),
        (500, "upstream https://user:pass@rpc.example/SECRET?key=SECRET failed", "HTTP 500 Internal Server Error: upstream https://rpc.example/[redacted] failed"),
    ] {
        quote_flow("quote_error", "service::sweep::flow_tests::sweep_quote_other_errors_keep_redacted_text", status, body, expected);
        if std::env::var("SWEEP_FLOW_CASE").is_ok() { break; }
    }
}

fn quote_flow(scenario: &str, name: &str, status: u16, body: &str, expected: &str) {
    if std::env::var("SWEEP_FLOW_CASE").is_ok() {
        let url = std::env::var("AGENTSWAP_RPC_URL_8453").unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(flow_case(&url)); return;
    }
    let rpc = fixture(1_000_000_000_000_000_000, 1, 1_000_000);
    let server = quote_server::Server::start(vec![(status, String::new(), body.into())]);
    let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", name, "--nocapture"])
        .env("SWEEP_FLOW_CASE", scenario).env("AGENTSWAP_RPC_URL_8453", &rpc.url)
        .env("SWEEP_QUOTE_URL", &server.url).env("SWEEP_QUOTE_ERROR", expected).output().unwrap();
    assert!(output.status.success(), "{scenario}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1, "trade must reuse the checked sweep response");
    let request: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert!(request.get("verify").is_none(), "{request}");
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].target, crate::routes::QUOTE);
    assert!(!requests[0].keyed && !requests[0].paid);
    drop(requests);
    if scenario == "quote_preview" {
        let checked: serde_json::Value = serde_json::from_str(body).unwrap();
        let bound = request.clone();
        let pinned = Client::new(&server.url, None).with_pinned_quote(bound.clone(), checked.clone());
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            assert_eq!(pinned.quote(&bound).await.unwrap(), checked);
            for (field, value) in [("amount_in", json!("1000001")), ("chain_id", json!(56)),
                ("token_in", json!(Address::repeat_byte(9))), ("token_out", json!(Address::repeat_byte(9))),
                ("verify", json!(true)), ("slippage_bps", json!(1))] {
                let mut changed = bound.clone(); changed[field] = value;
                assert!(pinned.quote(&changed).await.unwrap_err().to_string().contains("differs"));
            }
        });
        assert_eq!(server.requests.lock().unwrap().len(), 1, "differing re-quotes must not reach HTTP");
    }
    assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
}

fn assert_quote_failure(row: &Row, output: &Output) {
    assert_eq!(row.reason.as_deref(), Some("quote_failed"), "{row:?}");
    assert_eq!(row.outcome, "failed");
    assert_eq!(row.error.as_deref(), Some(std::env::var("SWEEP_QUOTE_ERROR").unwrap().as_str()));
    assert!(row.trade.is_none());
    assert!(output.check().is_err());
}

#[test]
fn sweep_oneinch_only_prices_reach_dry_run_signing() {
    const NAME: &str = "service::sweep::flow_tests::sweep_oneinch_only_prices_reach_dry_run_signing";
    if std::env::var("SWEEP_FLOW_CASE").is_ok() {
        let url = std::env::var("AGENTSWAP_RPC_URL_8453").unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(flow_case(&url)); return;
    }
    let rpc = fixture(1_000_000_000_000_000_000, 1, 1_000_000);
    let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
        .env("SWEEP_FLOW_CASE", "oneinch_preview").env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(rpc.called(""), 1);
    assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
}
