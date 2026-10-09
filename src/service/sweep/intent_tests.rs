// Hermetic sweep intent flows exercise signing, relay requests and mode-specific policy checks.
// JSON inputs keep these regressions executable against the market-only implementation.
use super::*;
use crate::{order_types::{IntentSettlerV3, UserProxyFactoryV6}, service::test_rpc::{TestRpc, ok}};
use alloy::{primitives::U256, sol_types::{SolCall, SolValue, SolEvent}};
use serde_json::{Value, json};
#[path = "../../client/test_server.rs"]
mod relay_server;

fn input(via: Option<&str>, dry: bool) -> Input {
    let mut value = json!({"chain_id":"8453", "proxy":Address::repeat_byte(6), "receive":"WETH",
        "tokens":[Address::repeat_byte(1), Address::repeat_byte(2)], "max_usd":"5", "max_loss_bps":100,
        "dry_run":dry, "wait":0});
    if let Some(via) = via { value["via"] = json!(via); }
    serde_json::from_value(value).unwrap()
}

fn rpc(mask: u8, status: &'static str) -> TestRpc {
    let mut orders = Vec::<order_types::Order>::new();
    TestRpc::start(move |body| {
        let result = match body["method"].as_str().unwrap() {
            "eth_getBlockByNumber" => {
                let mut block = serde_json::to_value(alloy::rpc::types::Block::<alloy::rpc::types::Transaction>::default()).unwrap();
                block["timestamp"] = json!("0x64"); block
            }
            "eth_blockNumber" => json!("0x1"),
            "eth_getLogs" => {
                let id = body["params"][0]["topics"][1].as_str().unwrap();
                let order = orders.iter().find(|o| format!("{:?}", order_types::order_id(o)) == id).unwrap();
                let event = IntentSettlerV3::IntentAnnounced { id: order_types::order_id(order), owner: order.owner,
                    appData: order.appData, order: order.abi_encode().into(), ownerSig: (U256::ZERO, alloy::primitives::Bytes::new()).abi_encode().into() };
                let log = event.encode_log_data();
                if status == "missing" { json!([]) } else { json!([{"address":evm::chain_config("8453").unwrap().settler,
                    "topics":log.topics(), "data":log.data, "blockNumber":"0x1", "logIndex":"0x0", "transactionIndex":"0x0"}]) }
            }
            "eth_call" => {
                let tx = &body["params"][0];
                let data = hex::decode(tx["input"].as_str().or(tx["data"].as_str()).unwrap().trim_start_matches("0x")).unwrap();
                if data.starts_with(&IntentSettlerV3::orderHashCall::SELECTOR) {
                    orders.push(IntentSettlerV3::orderHashCall::abi_decode(&data).unwrap().o);
                }
                if data.starts_with(&order_types::IntentLensV3::PREVIEW_LAYOUTCall::SELECTOR) { return Some(ok(body, json!(format!("0x{}", hex::encode(U256::from(3).abi_encode()))))); }
                if data.starts_with(&order_types::IntentLensV3::previewCall::SELECTOR) {
                    let mut words = [U256::ZERO; 19];
                    words[1] = U256::from(u8::from(status == "cancelled")); words[2] = U256::from(u8::from(status == "filled"));
                    words[5] = U256::from(1); words[6] = U256::from(u8::from(status != "expired"));
                    return Some(ok(body, json!(format!("0x{}", hex::encode(words.abi_encode())))));
                }
                json!(format!("0x{}", hex::encode(call(&data, mask, tx["to"].as_str().unwrap().parse().unwrap()))))
            }
            method => panic!("unexpected RPC {method}"),
        };
        Some(ok(body, result))
    })
}

fn call(data: &[u8], mask: u8, proxy: Address) -> Vec<u8> {
    let s = &data[..4];
    if s == UserProxyV6::ownerCall::SELECTOR { return Address::repeat_byte(4).abi_encode(); }
    if s == UserProxyFactoryV6::proxyOfCall::SELECTOR { return Address::repeat_byte(6).abi_encode(); }
    if s == UserProxyV6::policyOfCall::SELECTOR { return (U256::from(u64::MAX), U256::from(60), U256::from(mask), U256::from(1)).abi_encode(); }
    if s == UserProxyV6::agentTokenInfoCall::SELECTOR { return (true, U256::from(900_000), U256::from(200_000), U256::from(90)).abi_encode(); }
    if s == discovery::BalanceReader::balanceOfCall::SELECTOR { return U256::from(1_000_000).abi_encode(); }
    if s == sale::AllowanceReader::allowanceCall::SELECTOR { return U256::from(600_000).abi_encode(); }
    if s == order_types::Erc20Metadata::decimalsCall::SELECTOR { return U256::from(6).abi_encode(); }
    if s == order_types::Erc20Metadata::symbolCall::SELECTOR { return "TEST".to_string().abi_encode(); }
    if s == UserProxyV6::isAgentNonceUsedCall::SELECTOR { return false.abi_encode(); }
    if s == UserProxyV6::isIntentAuthorizedCall::SELECTOR { return true.abi_encode(); }
    if s == IntentSettlerV3::orderHashCall::SELECTOR {
        return order_types::order_id(&IntentSettlerV3::orderHashCall::abi_decode(data).unwrap().o).abi_encode();
    }
    if s == UserProxyV6::hashIntentAuthorizationCall::SELECTOR {
        let auth = UserProxyV6::hashIntentAuthorizationCall::abi_decode(data).unwrap().auth;
        return order_types::signing_hash(&auth, &order_types::proxy_domain(8453, proxy)).abi_encode();
    }
    panic!("unexpected selector {}", hex::encode(s));
}

#[test]
fn gasless_sweep_relay_amounts_previews_skips_and_failures() {
    const NAME: &str = "service::sweep::intent_tests::gasless_sweep_relay_amounts_previews_skips_and_failures";
    if let Ok(url) = std::env::var("SWEEP_INTENT_RPC") {
        tokio::runtime::Runtime::new().unwrap().block_on(flows(&url)); return;
    }
    let rpc = rpc(5, "open");
    let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
        .env("SWEEP_INTENT_RPC", &rpc.url).env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
}

async fn flows(url: &str) {
    let receive = token::from_registry("WETH", 8453).unwrap();
    let price_body = json!({"prices":{
        Address::repeat_byte(1).to_string():{"priceUsd":2,"source":"defillama"},
        Address::repeat_byte(2).to_string():{"priceUsd":1,"source":"defillama"},
        receive.address.clone():{"priceUsd":4,"source":"defillama"}
    }}).to_string();
    for (dry, relay_status, max, cap, skip) in [(false, 200, "5", None, None), (true, 200, "5", None, None),
        (false, 500, "5", None, None), (false, 200, "0.1", None, Some("over_max_usd")),
        (false, 200, "5", Some(U256::from(1)), None), (false, 200, "5", None, Some("below_floor")),
        (false, 200, "5", None, Some("unpriced")), (false, 200, "5", None, Some("price_not_independent"))] {
        let mut body: Value = serde_json::from_str(&price_body).unwrap();
        let receive_price = &mut body["prices"][&receive.address];
        match skip {
            Some("below_floor") => receive_price["priceUsd"] = serde_json::from_str("100000000000000000000").unwrap(),
            Some("unpriced") => *receive_price = Value::Null,
            Some("price_not_independent") => receive_price["source"] = json!("oracle"),
            _ => {}
        }
        let app = relay_server::Server::start(vec![(200, String::new(), body.to_string())]);
        let relay = relay_server::Server::start(vec![(relay_status, String::new(), json!({"accepted":true}).to_string())]);
        let signer = Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
        let mut request = input(None, dry); request.max_usd = max.into();
        let output = read(&Client::new(&relay.url, None), signer, request, evm::read_provider(url).unwrap(), &app.url, cap, Wait::MCP).await.unwrap();
        assert_rows(&output, dry, relay_status == 500 || cap.is_some(), skip);
        let requests = relay.requests.lock().unwrap();
        assert_eq!(requests.len(), if dry || skip.is_some() || cap.is_some() { 0 } else { 2 });
        assert_requests(&requests, &output);
    }
}

fn assert_rows(output: &Output, dry: bool, failed: bool, skip: Option<&str>) {
    let value = serde_json::to_value(&output).unwrap();
    let rows: Vec<_> = value["tokens"].as_array().unwrap().iter().filter(|r| r["reason"] != "receive_token").collect();
    assert_eq!(rows.len(), 2);
    for (i, row) in rows.iter().enumerate() {
        assert!(row.get("intent_id").is_some(), "intent result fields missing: {row}");
        if let Some(skip) = skip {
            assert_eq!(row["reason"], skip);
            if skip == "below_floor" { assert_eq!(row["floor_raw"], "0"); }
            continue;
        }
        assert_eq!(row["amount_raw"], "600000");
        assert_eq!(row["start_out_raw"], if i == 0 { "300000000000000000" } else { "150000000000000000" });
        assert_eq!(row["floor_raw"], if i == 0 { "297000000000000000" } else { "148500000000000000" });
        if failed { assert_eq!(row["outcome"], "failed"); }
        else {
            assert_eq!(row["announce_status"], if dry { "dry_run" } else { "accepted" });
            assert_eq!(row["intent_id"].as_str().unwrap().len(), 66);
        }
    }
    assert_eq!(output.check().is_err(), failed);
}

fn assert_requests(requests: &[relay_server::Request], output: &Output) {
    let value = serde_json::to_value(output).unwrap();
    let rows: Vec<_> = value["tokens"].as_array().unwrap().iter().filter(|r| r["reason"] != "receive_token").collect();
    for (i, request) in requests.iter().enumerate() {
        assert_eq!(request.target, crate::routes::INTENT_ANNOUNCE, "never quote in intent mode");
        assert!(!request.paid);
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["chainId"], 8453); assert_eq!(body["generation"], "v6");
        let order = &body["announce"]["order"];
        assert_eq!(order["amountIn"], "600000");
        assert_eq!(order["startAmountOut"], rows[i]["start_out_raw"]);
        assert_eq!(order["endAmountOut"], rows[i]["floor_raw"]);
        assert_eq!(order["owner"], format!("{:?}", Address::repeat_byte(4)));
        assert_eq!(order["recipient"], order["owner"]);
        assert_eq!(order["endTime"].as_str().unwrap().parse::<u64>().unwrap() - order["startTime"].as_str().unwrap().parse::<u64>().unwrap(), 600);
        assert_eq!(order["endTime"], order["decayEndTime"]);
        assert_ne!(body["announce"]["auth"], "0x");
    }
}

#[tokio::test]
async fn gasless_sweep_action_bits_refused_before_prices() {
    for (via, mask, reason) in [(None, 1, Some("intent action")), (Some("intent"), 1, Some("intent action")),
        (Some("market"), 4, Some("market action")), (None, 4, None), (Some("market"), 1, None)] {
        let rpc = rpc(mask, "open");
        let app = relay_server::Server::start(vec![(200, String::new(), json!({"prices":{}}).to_string())]);
        let signer = Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
        let result = read(&Client::new(&app.url, None), signer, input(via, true), evm::read_provider(&rpc.url).unwrap(), &app.url, None, Wait::MCP).await;
        if let Some(reason) = reason { assert!(result.unwrap_err().to_string().contains(reason)); assert_eq!(app.requests.lock().unwrap().len(), 0); }
        else { assert!(result.unwrap().check().is_ok()); }
    }
}

#[test]
fn gasless_sweep_wait_reports_terminal_and_timeout_states() {
    const NAME: &str = "service::sweep::intent_tests::gasless_sweep_wait_reports_terminal_and_timeout_states";
    if let Ok(url) = std::env::var("SWEEP_INTENT_RPC") {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let relay = relay_server::Server::start(vec![(200, String::new(), json!({"accepted":true}).to_string())]);
            let client = Client::new(&relay.url, None);
            let value = json!({"chain_id":"8453", "proxy":Address::repeat_byte(6), "receive":"WETH",
                "tokens":[Address::repeat_byte(1)], "max_usd":"5", "max_loss_bps":100, "dry_run":false,"wait":1});
            let input = serde_json::from_value(value).unwrap();
            let receive = token::from_registry("WETH", 8453).unwrap();
            let mut policy = tests::policy(); policy.tokens.push(intent::TokenPolicy {
                token: Address::repeat_byte(1).to_string(), allowed:true, cap:"900000".into(), used:"200000".into(), epoch_start:"90".into() });
            policy.tokens[0].token = receive.address.clone(); policy.action_mask = "5".into();
            let prices = BTreeMap::from([(Address::repeat_byte(1), tests::price("1", true)), (receive.address.parse().unwrap(), tests::price("1", true))]);
            let signer = Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
            let output = run(Context { client:&client, signer, input, provider:evm::read_provider(&url).unwrap(), owner:Address::repeat_byte(4),
                receive, prices, policy, max:amount::fixed("5", false).unwrap(), server_cap:None, wait:Wait::MCP }).await.unwrap();
            output.check().unwrap();
            let rows = serde_json::to_value(output).unwrap();
            let row = &rows["tokens"][1];
            let status = std::env::var("SWEEP_WAIT_STATUS").unwrap();
            assert_eq!(row["intent_status"], if status == "missing" { "unknown" } else { &status });
            assert_eq!(row["wait_timed_out"], status == "open" || status == "missing");
            assert_eq!(relay.requests.lock().unwrap().len(), 1);
        }); return;
    }
    for status in ["filled", "expired", "cancelled", "open", "missing"] {
        let rpc = rpc(5, status);
        let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
            .env("SWEEP_INTENT_RPC", &rpc.url).env("AGENTSWAP_RPC_URL_8453", &rpc.url).env("SWEEP_WAIT_STATUS", status).output().unwrap();
        assert!(output.status.success(), "{status}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
    }
}
