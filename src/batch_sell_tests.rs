// Hermetic batch-sell entrypoint proofs, also runnable on the pre-feature revision.
// Each case enters through clap and uses only loopback HTTP and RPC fixtures.
use super::*;
use alloy::primitives::Address;
use serde_json::{Value, json};
#[path = "client/test_server.rs"]
mod http;
#[path = "service/test_rpc.rs"]
mod test_rpc;

fn plan_args() -> Vec<String> {
    ["agentswap", "--url", "http://127.0.0.1:1", "--json", "batch-sell", "plan", "--chainid", "8453", "--owner", &Address::repeat_byte(4).to_string(),
        "--agent", &Address::repeat_byte(2).to_string(), "--receive", "WETH", "--max-loss-bps", "500"]
        .into_iter().map(String::from).collect()
}

fn child(name: &str, app: &http::Server, args: &[String]) -> std::process::Output {
    let rpc = test_rpc::TestRpc::start(|body| Some(test_rpc::ok(body, json!("0x1"))));
    std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("BATCH_APP", &app.url).env("BATCH_ARGS", serde_json::to_string(args).unwrap())
        .env("AGENTSWAP_RPC_URL", &rpc.url).env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap()
}

fn enter() -> bool {
    let Ok(url) = std::env::var("BATCH_APP") else { return false; };
    routes::TEST_APP_ORIGIN.set(url).unwrap();
    let args: Vec<String> = serde_json::from_str(&std::env::var("BATCH_ARGS").unwrap()).unwrap();
    let result = tokio::runtime::Runtime::new().unwrap().block_on(run_cli(Cli::try_parse_from(args).unwrap()));
    if std::env::var("BATCH_REFUSE").is_ok() {
        assert!(result.unwrap_err().to_string().contains("confirmed"));
    } else { result.unwrap(); }
    true
}

fn responses(n: u8) -> Vec<(u16, String, String)> {
    let tokens: Vec<_> = (1..=n).map(|i| json!({"address":Address::repeat_byte(i),"balanceRaw":"1234500","decimals":6,"symbol":"TEST"})).collect();
    let mut prices = serde_json::Map::new();
    for i in 1..=n { prices.insert(Address::repeat_byte(i).to_string(), json!({"priceUsd":2,"source":"defillama"})); }
    prices.insert(service::token::from_registry("WETH", 8453).unwrap().address, json!({"priceUsd":4,"source":"defillama"}));
    vec![(200, String::new(), json!({"chainId":8453,"owner":Address::repeat_byte(4),"indexed":true,"truncated":false,"tokens":tokens}).to_string()),
        (200, String::new(), json!({"prices":prices}).to_string()),
        (200, String::new(), json!({"prices":prices}).to_string()),
        (201, String::new(), json!({"id":"abcdefghijklmnopqrstuv","url":"https://app.agentswap.co/grant/r/abcdefghijklmnopqrstuv","expiresAt":"2026-10-10T12:00:00Z"}).to_string())]
}

#[test]
fn batch_sell_plan_posts_exact_caps_and_splits_at_twenty() {
    if enter() { return; }
    let app = http::Server::start(responses(21));
    let output = child("batch_sell_tests::batch_sell_plan_posts_exact_caps_and_splits_at_twenty", &app, &plan_args());
    assert!(output.status.success(), "{} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let requests = app.requests.lock().unwrap();
    let posts: Vec<_> = requests.iter().filter(|r| r.method == "POST").collect();
    assert_eq!(posts.len(), 2);
    for (post, count) in posts.iter().zip([20, 3]) {
        assert_eq!(post.target, "/api/grant-requests");
        let body: Value = serde_json::from_str(&post.body).unwrap();
        assert_eq!(body["tokens"].as_array().unwrap().len(), count);
        assert_eq!(body["tokens"][0]["cap"], "1.2345");
        assert_eq!(body["tokens"][count - 1]["cap"], "0");
        assert_eq!(body["maxLossBps"], 500); assert_eq!(body["purpose"], "batch-sell");
        assert_eq!(body["actions"], json!(["market","intent"]));
        assert_eq!(body["v"], 1); assert_eq!(body["chainId"], 8453);
        assert_eq!(body["epoch"], "1w"); assert!(body["signature"].is_null());
        assert!(!post.keyed && !post.paid);
    }
    assert!(String::from_utf8_lossy(&output.stdout).contains("51.849"));
}

#[test]
fn batch_sell_plan_selection_is_explicit_without_small_default() {
    if enter() { return; }
    let app = http::Server::start(responses(3));
    let mut args = plan_args();
    args.extend(["--min-usd".into(), "2".into(), "--max-usd".into(), "3".into(),
        "--token".into(), Address::repeat_byte(1).to_string(), "--token".into(), Address::repeat_byte(2).to_string(),
        "--exclude".into(), Address::repeat_byte(2).to_string()]);
    let output = child("batch_sell_tests::batch_sell_plan_selection_is_explicit_without_small_default", &app, &args);
    assert!(output.status.success(), "{} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let requests = app.requests.lock().unwrap();
    let post = requests.iter().find(|r| r.method == "POST").unwrap();
    let body: Value = serde_json::from_str(&post.body).unwrap();
    assert_eq!(body["tokens"].as_array().unwrap().len(), 2);
    assert_eq!(body["tokens"][0]["address"], Address::repeat_byte(1).to_string());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("excluded") && text.contains("not_selected"), "{text}");
}

#[test]
fn batch_sell_run_refuses_unconfirmed_before_signing_or_rpc() {
    if enter() { return; }
    for status in ["pending", "expired"] {
        let app = http::Server::start(vec![(200, String::new(), json!({"id":"abcdefghijklmnopqrstuv", "status":status,"confirmed":null,
            "request":{"v":1,"chainId":8453,"agent":Address::repeat_byte(2),"tokens":[],"maxLossBps":500,"purpose":"batch-sell"}}).to_string())]);
        let args = ["agentswap", "batch-sell", "run", "--request", "abcdefghijklmnopqrstuv"].map(String::from);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "batch_sell_tests::batch_sell_run_refuses_unconfirmed_before_signing_or_rpc", "--nocapture"])
            .env("BATCH_APP", &app.url).env("BATCH_ARGS", serde_json::to_string(&args).unwrap()).env("BATCH_REFUSE", "1").output().unwrap();
        assert!(output.status.success(), "{} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let requests = app.requests.lock().unwrap();
        assert_eq!(requests.len(), 1); assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].target, "/api/grant-requests/abcdefghijklmnopqrstuv");
    }
}

#[test]
fn batch_sell_cli_contract_and_grant_purpose() {
    assert!(Cli::try_parse_from(plan_args()).is_ok());
    let mut missing = plan_args(); missing.truncate(missing.len() - 2);
    assert!(Cli::try_parse_from(missing).is_err());
    assert!(Cli::try_parse_from(["agentswap", "sweep", "--help"]).err().unwrap().kind() == clap::error::ErrorKind::InvalidSubcommand);
    for loss in ["0", "5001"] { let mut args = plan_args(); *args.last_mut().unwrap() = loss.into(); assert!(Cli::try_parse_from(args).is_err()); }
    let args = ["agentswap", "grant-link", "--chainid", "8453", "--owner", "owner", "--agent", "agent", "--token", "token", "--receive", "WETH", "--one-shot", "--purpose", "batch-sell", "--max-loss-bps", "500"];
    assert!(Cli::try_parse_from(args).is_ok());
}

#[test]
fn batch_sell_plan_filters_and_economic_floor() {
    if enter() { return; }
    for (flag, value, reason) in [("--min-usd","3","under_min_usd"),("--max-usd","2","over_max_usd"),("gas","0","below_gas_floor"),("large","0",""),("fraction","0","over_max_usd")] {
        let mut replies = responses(1);
        if flag == "gas" || flag == "large" {
            let index = if flag == "gas" { 2 } else { 1 };
            let mut body: Value = serde_json::from_str(&replies[index].2).unwrap();
            let address = if flag == "gas" { service::token::from_registry("WETH",8453).unwrap().address } else { Address::repeat_byte(1).to_string() };
            body["prices"][address]["priceUsd"] = json!(1_000_000_000_000u64);
            replies[index].2 = body.to_string();
        }
        if flag == "fraction" {
            let mut body: Value = serde_json::from_str(&replies[1].2).unwrap();
            body["prices"][Address::repeat_byte(1).to_string()]["priceUsd"] = serde_json::from_str("0.000000000000000001").unwrap();
            replies[1].2 = body.to_string();
        }
        let app = http::Server::start(replies);
        let mut args = plan_args();
        if flag == "fraction" { args.extend(["--max-usd".into(), "0.000000000000000001".into()]); }
        if flag.starts_with("--") { args.extend([flag.into(), value.into()]); }
        let output = child("batch_sell_tests::batch_sell_plan_filters_and_economic_floor", &app, &args);
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{text} {}", String::from_utf8_lossy(&output.stderr));
        assert!(text.contains(reason), "{flag}: {text}");
        let requests = app.requests.lock().unwrap();
        assert_eq!(requests.iter().filter(|r| r.method == "POST").count(), usize::from(flag == "large"));
    }
}
