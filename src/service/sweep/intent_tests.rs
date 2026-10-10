// Hermetic sweep intent flows exercise signing, relay requests and mode-specific policy checks.
// JSON inputs keep these regressions executable against the market-only implementation.
use super::*;
use crate::{order_types::{IntentSettlerV3, UserProxyFactoryV6}, service::test_rpc::{TestRpc, ok}};
use alloy::{primitives::{B256, U256}, sol_types::{SolCall, SolValue, SolEvent}};
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

pub(super) fn rpc(mask: u8, status: &'static str) -> TestRpc {
    rpc_sharing(mask, status, Arc::default())
}

/// The sweep RPC fixture; every order whose hash the signer checks is appended to `orders`.
fn rpc_sharing(mask: u8, status: &'static str, orders: Arc<std::sync::Mutex<Vec<order_types::Order>>>) -> TestRpc {
    TestRpc::start(move |body| {
        let result = match body["method"].as_str().unwrap() {
            "eth_getBlockByNumber" => {
                let mut block = serde_json::to_value(alloy::rpc::types::Block::<alloy::rpc::types::Transaction>::default()).unwrap();
                block["timestamp"] = json!("0x64"); block
            }
            "eth_gasPrice" => match status {
                "gas_missing" => return Some(crate::service::test_rpc::failure(body, "unavailable")),
                "gas_high" => json!("0x3b9aca00"),
                "gas_border" => json!("0xe4e1c0"),
                _ => json!("0x1"),
            },
            "eth_blockNumber" => json!("0x30d40"),
            "eth_getLogs" => {
                if status == "bounded" && body["params"][0]["fromBlock"] != "0x30d40" {
                    return Some(crate::service::test_rpc::failure(body, "scan predates placement"));
                }
                assert_eq!(body["params"][0]["topics"][0], json!(IntentSettlerV3::IntentFilled::SIGNATURE_HASH), "only fill logs, never announce scans");
                let id = body["params"][0]["topics"][1].as_str().unwrap();
                let orders = orders.lock().unwrap();
                let order = orders.iter().find(|o| format!("{:?}", order_types::order_id(o)) == id).unwrap();
                let event = IntentSettlerV3::IntentFilled { id:order_types::order_id(order),owner:order.owner,solver:Address::repeat_byte(5),
                    recipient:order.owner,caller:Address::repeat_byte(5),amountIn:order.amountIn,requiredOut:order.endAmountOut + U256::from(10),
                    fee:U256::from(10),receivedOut:order.endAmountOut + U256::from(10),aboveFloor:U256::ZERO }.encode_log_data();
                json!([{"address":evm::chain_config("8453").unwrap().settler,"topics":event.topics(),"data":event.data,
                    "blockNumber":"0x30d40","logIndex":"0x0","transactionIndex":"0x0"}])
            }
            "eth_call" => {
                let tx = &body["params"][0];
                let data = hex::decode(tx["input"].as_str().or(tx["data"].as_str()).unwrap().trim_start_matches("0x")).unwrap();
                if data.starts_with(&IntentSettlerV3::orderHashCall::SELECTOR) {
                    orders.lock().unwrap().push(IntentSettlerV3::orderHashCall::abi_decode(&data).unwrap().o);
                }
                if let Some(reply) = lens(body, &data, status) { return Some(reply); }
                json!(format!("0x{}", hex::encode(call(&data, mask, tx["to"].as_str().unwrap().parse().unwrap()))))
            }
            method => panic!("unexpected RPC {method}"),
        };
        Some(ok(body, result))
    })
}

/// IntentLensV3 answers: layout 3 and one view per order for the batched status read.
fn lens(body: &Value, data: &[u8], status: &str) -> Option<Value> {
    if data.starts_with(&order_types::IntentLensV3::PREVIEW_LAYOUTCall::SELECTOR) { return Some(ok(body, json!(format!("0x{}", hex::encode(U256::from(3).abi_encode()))))); }
    if !data.starts_with(&order_types::IntentLensV3::previewManyCall::SELECTOR) { return None; }
    if status == "missing" { return Some(crate::service::test_rpc::failure(body, "lens unavailable")); }
    let count = order_types::IntentLensV3::previewManyCall::abi_decode(data).unwrap().o.len();
    let views = vec![crate::service::intentscan::fixture::view(status == "filled", status == "cancelled", status != "expired"); count];
    Some(ok(body, json!(format!("0x{}", hex::encode(order_types::IntentLensV3::previewManyCall::abi_encode_returns(&views))))))
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
        assert_eq!(row["start_out_raw"], if i == 0 { "303000000000000000" } else { "151500000000000000" });
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
                receive, prices, policy, max:amount::fixed("5", false).unwrap(), server_cap:None, wait:Wait::MCP, pacer:intent::Pacer::system() }).await.unwrap();
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

/// Stream-only intents: the relay answers `{id, mode: "broadcast"}` and no IntentAnnounced log
/// exists; the intent index settles status and proceeds (an expiry once the lens agrees), and no log is scanned.
#[test]
fn stream_only_intents_settle_from_the_index_without_announce_logs() {
    const NAME: &str = "service::sweep::intent_tests::stream_only_intents_settle_from_the_index_without_announce_logs";
    let signer = || Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
    if let Ok(app) = std::env::var("STREAM_APP") {
        crate::routes::TEST_APP_ORIGIN.set(app).unwrap();
        crate::routes::TEST_INTENTSCAN_ORIGIN.set(std::env::var("STREAM_INDEX").unwrap()).unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(stream_only_run(signer())); return;
    }
    let weth = token::from_registry("WETH", 8453).unwrap().address;
    for status in ["filled", "expired"] {
        let orders = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (rpc, index) = (rpc_sharing(5, if status == "expired" { "expired" } else { "missing" }, orders.clone()), stream_index(orders, status, signer().address()));
        let relay = relay_server::Server::start(vec![(200, String::new(), json!({"id": format!("{:?}", B256::repeat_byte(1)), "mode": "broadcast"}).to_string())]);
        let record = json!({"id": "abcdefghijklmnopqrstuv", "status": "confirmed", "confirmed": {"proxy": Address::repeat_byte(6), "generation": "1", "maxLossBps": 100},
            "request": {"v": 1, "chainId": 8453, "agent": signer().address(), "owner": Address::repeat_byte(4), "purpose": "batch-sell", "maxLossBps": 500,
            "tokens": [{"address": Address::repeat_byte(1), "cap": "1"}, {"address": weth, "cap": "0"}]}});
        let prices = json!({"prices": {Address::repeat_byte(1).to_string(): {"priceUsd": 2, "source": "defillama"}, weth.clone(): {"priceUsd": 4, "source": "defillama"}}});
        let app = relay_server::Server::start(vec![(200, String::new(), record.to_string()), (200, String::new(), prices.to_string())]);
        let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
            .env("STREAM_APP", &app.url).env("STREAM_INDEX", &index.url).env("STREAM_RELAY", &relay.url).env("STREAM_STATUS", status)
            .env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
        assert!(output.status.success(), "{status}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert_eq!(rpc.called("eth_getLogs"), 0, "{status}: no IntentAnnounced or IntentFilled scan");
        assert_eq!((relay.requests.lock().unwrap().len(), rpc.called("eth_sendRawTransaction")), (1, 0));
    }
}

async fn stream_only_run(signer: Arc<crate::signer::local::LocalKey>) {
    use crate::service::batch_sell;
    let input: batch_sell::RunInput = serde_json::from_value(json!({"request": "abcdefghijklmnopqrstuv", "wait": 20})).unwrap();
    let record = batch_sell::load(&input).await.unwrap();
    let relay = Client::new(&std::env::var("STREAM_RELAY").unwrap(), None);
    let output = batch_sell::run(&relay, signer, input, record, true, None, Wait::MCP).await.unwrap();
    let row = output.tokens.iter().find(|r| r.intent_id.is_some()).expect("a placed intent");
    let status = std::env::var("STREAM_STATUS").unwrap();
    assert_eq!((row.intent_status.as_deref(), row.wait_timed_out, row.relay.as_ref().map(|r| r["mode"].clone())), (Some(status.as_str()), false, Some(json!("broadcast"))));
    let expected = if status == "filled" { (Some("777"), "sold", None) } else { (None, "skipped", Some("expired")) };
    assert_eq!((row.received_raw.as_deref(), row.outcome.as_str(), row.reason.as_deref()), expected);
}

/// Intent index fixture: history rows for every signed order in `status`, and a fill record
/// for each when filled.
fn stream_index(orders: Arc<std::sync::Mutex<Vec<order_types::Order>>>, status: &'static str, agent: Address) -> crate::service::test_http::TestHttp {
    use crate::service::intentscan::fixture;
    crate::service::test_http::TestHttp::start(move |target| {
        let orders = orders.lock().unwrap().clone();
        if target.starts_with("/v1/intents?") {
            let items = orders.iter().map(|o| fixture::item(8453, o, agent, 1, status, super::now_ms(), u64::MAX / 2)).collect();
            return (200, fixture::page(items, None));
        }
        match orders.iter().map(order_types::order_id).find(|id| target == format!("/v1/intent/{id:?}")) {
            Some(id) if status == "filled" => (200, fixture::fill(id, "777", B256::repeat_byte(7), 1_000)),
            _ => (404, "{}".into()),
        }
    })
}

#[path = "gas_floor/flow_tests.rs"]
mod fix_tests;
#[path = "premium_tests.rs"]
mod premium_tests;
