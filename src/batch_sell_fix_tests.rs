// Batch planning regressions for the single-grant boundary and unusable receive prices.
// Child processes isolate the app origin and RPC environment from parallel tests.
use crate::batch_sell_tests::{plan_args, responses};
use alloy::primitives::Address;
use serde_json::{Value, json};
#[path = "client/test_server.rs"]
mod http;
#[path = "service/test_rpc.rs"]
mod rpc;

#[test]
fn batch_sell_plan_rejects_overflow_and_bad_receive_and_omits_long_caps() {
    const NAME: &str = "batch_sell_fix_tests::batch_sell_plan_rejects_overflow_and_bad_receive_and_omits_long_caps";
    if let Ok(origin) = std::env::var("BATCH_FIX_APP") {
        crate::routes::TEST_APP_ORIGIN.set(origin).unwrap();
        use clap::Parser;
        let result = tokio::runtime::Runtime::new().unwrap().block_on(crate::run_cli(crate::cli::Cli::try_parse_from(plan_args()).unwrap()));
        match std::env::var("BATCH_FIX_CASE").unwrap().as_str() {
            "overflow" => { let error = result.unwrap_err().to_string(); assert!(error.contains("101") && error.contains("narrow"), "{error}"); }
            "missing" | "ineligible" => assert!(result.unwrap_err().to_string().contains("receive token price")),
            "long" => result.unwrap(),
            _ => unreachable!(),
        }
        return;
    }
    let mut failures = Vec::new();
    for case in ["overflow", "missing", "ineligible", "long"] {
        let replies = replies(case);
        let app = http::Server::start(replies);
        let rpc = rpc::TestRpc::start(|body| Some(rpc::ok(body, json!("0x1"))));
        let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
            .env("BATCH_FIX_APP", &app.url).env("BATCH_FIX_CASE", case)
            .env("AGENTSWAP_RPC_URL", &rpc.url).env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() { failures.push(format!("{case}: {text} {}", String::from_utf8_lossy(&output.stderr))); continue; }
        let requests = app.requests.lock().unwrap();
        let posts: Vec<_> = requests.iter().filter(|r| r.method == "POST").collect();
        assert_eq!(posts.len(), usize::from(case == "long"));
        if case == "long" {
            assert!(text.contains("cap_too_long"), "{text}");
            let body: Value = serde_json::from_str(&posts[0].body).unwrap();
            assert_eq!(body["tokens"].as_array().unwrap().len(), 2);
            assert_eq!(body["tokens"][0]["address"], Address::repeat_byte(2).to_string());
        }
        assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn replies(case: &str) -> Vec<(u16, String, String)> {
    let mut replies = responses(if case == "overflow" { 100 } else { 2 });
    if case == "missing" || case == "ineligible" {
        let mut body: Value = serde_json::from_str(&replies[2].2).unwrap();
        let receive = crate::service::token::from_registry("WETH", 8453).unwrap().address;
        if case == "missing" { body["prices"].as_object_mut().unwrap().remove(&receive); }
        else { body["prices"][receive]["source"] = json!("oracle"); }
        replies[2].2 = body.to_string();
    }
    if case == "long" {
        let mut body: Value = serde_json::from_str(&replies[0].2).unwrap();
        body["tokens"][0]["balanceRaw"] = json!("1000000000000000000000000000000001");
        body["tokens"][0]["decimals"] = json!(18);
        replies[0].2 = body.to_string();
    }
    replies
}
