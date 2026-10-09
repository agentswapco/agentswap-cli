// MCP quote schema and batch forwarding regressions.
// Exercises JSON inputs against a hermetic quote service.
use super::*;
#[path = "../client/test_server.rs"]
mod http;
use serde_json::json;

#[tokio::test]
async fn fix_mcp_batch_requires_taker_and_exposes_both_inputs() {
    let client = Client::new("http://127.0.0.1:1", None);
    let server = AgentSwapMcp::new(Config { client:client.clone(), intent_client:client,
        signer:None, allow_trade:false, trade_max_amount:None });
    for name in ["quote", "batch_quote"] {
        let tools = server.tool_router.list_all();
        let tool = tools.iter().find(|t| t.name == name).unwrap();
        assert!(tool.input_schema["properties"].get("taker").is_some(), "{name}");
    }
    let input = serde_json::from_value(json!({"chain_id":"56", "pairs":["UNKNOWN/UNKNOWN"], "amount":"1"})).unwrap();
    let error = server.batch_quote(Parameters(input)).await.err().expect("missing taker must fail");
    assert!(error.contains("taker") && error.contains("V6 proxy"));
}

#[test]
fn fix_mcp_quotes_forward_owner_proxy_to_meta() {
    const NAME: &str = "mcp::fix_tests::fix_mcp_quotes_forward_owner_proxy_to_meta";
    if let Ok(url) = std::env::var("FIX_META_URL") {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let client = crate::client::meta_tests::client("http://127.0.0.1:1", &url);
            let server = AgentSwapMcp::new(Config { client:client.clone(), intent_client:client,
                signer:None, allow_trade:false, trade_max_amount:None });
            let from = alloy::primitives::Address::repeat_byte(1).to_string();
            let to = alloy::primitives::Address::repeat_byte(2).to_string();
            let taker = alloy::primitives::Address::repeat_byte(6).to_string();
            let input = json!({"chain_id":"56", "amount":"1", "taker":taker, "from":from,"to":to});
            server.quote(Parameters(serde_json::from_value(input).unwrap())).await.ok().expect("quote");
            let input = json!({"chain_id":"56", "amount":"1", "taker":taker, "pairs":[format!("{from}/{to}")]});
            let Json(result) = server.batch_quote(Parameters(serde_json::from_value(input).unwrap())).await.ok().expect("batch");
            assert!(result.results[0].error.is_none());
        }); return;
    }
    let rpc = crate::fix_tests::metadata_rpc();
    let meta = http::Server::start(vec![(200, String::new(), crate::client::meta_tests::best().to_string())]);
    let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
        .env("FIX_META_URL", &meta.url).env("AGENTSWAP_RPC_URL_56", &rpc.url).output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let seen = meta.requests.lock().unwrap();
    assert_eq!(seen.len(), 2);
    for request in seen.iter() {
        assert_eq!(request.method, "POST");
        assert_eq!(request.target, "/quote");
        let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["taker"], alloy::primitives::Address::repeat_byte(6).to_string());
        assert!(!request.keyed && !request.paid);
    }
}
