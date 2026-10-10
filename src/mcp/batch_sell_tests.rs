// Batch-sell MCP validation uses the same request DTOs as the CLI.
// Confirmed discount cannot be supplied as an execution override; start premium bounds hold pre-network.
use super::*;
use serde_json::json;
#[allow(dead_code)]
#[path = "../client/test_server.rs"]
mod http;

#[test]
fn batch_sell_mcp_inputs_require_discount_and_default_intent() {
    let value = json!({"chain_id":"8453", "owner":"owner", "agent":"agent", "receive":"USDC"});
    assert!(serde_json::from_value::<batch_sell::PlanInput>(value).is_err());
    let input: batch_sell::RunInput = serde_json::from_value(json!({"request":"abcdefghijklmnopqrstuv"})).unwrap();
    assert_eq!(input.via, batch_sell::Via::Intent);
    assert!(input.wait.is_none());
    assert!(serde_json::from_value::<batch_sell::RunInput>(json!({"request":"id", "via":"invalid"})).is_err());
}

#[tokio::test]
async fn batch_sell_mcp_routes_validate_before_network_or_signing() {
    let client = Client::new("http://127.0.0.1:1", None);
    let server = AgentSwapMcp::new(Config {client:client.clone(),intent_client:client,signer:None,allow_trade:false,trade_max_amount:None});
    let input = serde_json::from_value(json!({"chain_id":"8453","owner":"owner","agent":"agent","receive":"USDC","max_loss_bps":0})).unwrap();
    assert!(server.batch_sell_plan(Parameters(input)).await.err().unwrap().contains("max-loss-bps"));
    let input = serde_json::from_value(json!({"request":"short"})).unwrap();
    assert!(server.batch_sell_run(Parameters(input)).await.err().unwrap().contains("request id"));
}

#[tokio::test]
async fn batch_sell_mcp_plan_refuses_long_name_before_network() {
    let client = Client::new("http://127.0.0.1:1", None);
    let server = AgentSwapMcp::new(Config {client:client.clone(),intent_client:client,signer:None,allow_trade:false,trade_max_amount:None});
    let input = serde_json::from_value(json!({"chain_id":"8453","owner":"owner","agent":"agent","receive":"USDC","max_loss_bps":500,"name":"a".repeat(33)})).unwrap();
    let error = server.batch_sell_plan(Parameters(input)).await.err().unwrap();
    assert!(error.contains("name must be at most 32 characters"), "{error}");
}

#[test]
fn batch_sell_plan_mcp_schema_offers_optional_name_and_note() {
    let client = Client::new("http://127.0.0.1:1", None);
    let server = AgentSwapMcp::new(Config {client:client.clone(),intent_client:client,signer:None,allow_trade:false,trade_max_amount:None});
    let tool = server.tool_router.list_all().into_iter().find(|t| t.name == "batch_sell_plan").unwrap();
    let schema = serde_json::to_value(&tool.input_schema).unwrap();
    for field in ["name", "note"] {
        assert!(schema["properties"][field]["description"].as_str().is_some_and(|d| d.contains("review page")), "{schema}");
        assert!(!schema["required"].as_array().unwrap().contains(&json!(field)));
    }
    assert!(tool.description.as_deref().unwrap_or_default().contains("Always pass name"));
}

#[test]
fn batch_sell_run_mcp_schema_offers_bounded_start_premium() {
    let client = Client::new("http://127.0.0.1:1", None);
    let server = AgentSwapMcp::new(Config {client:client.clone(),intent_client:client,signer:None,allow_trade:false,trade_max_amount:None});
    let tool = server.tool_router.list_all().into_iter().find(|t| t.name == "batch_sell_run").unwrap();
    let schema = serde_json::to_value(&tool.input_schema).unwrap();
    let field = &schema["properties"]["start_premium_bps"];
    assert_eq!((&field["default"], &field["minimum"], &field["maximum"]), (&json!(100), &json!(0), &json!(1000)), "{schema}");
    assert!(field["description"].as_str().is_some_and(|d| d.contains("above independent market value")), "{schema}");
    assert!(!schema["required"].as_array().unwrap().contains(&json!("start_premium_bps")));
    assert!(tool.description.as_deref().unwrap_or_default().contains("start_premium_bps"));
}

#[test]
fn batch_sell_run_mcp_refuses_start_premium_out_of_range_before_any_request() {
    const NAME: &str = "mcp::batch_sell_tests::batch_sell_run_mcp_refuses_start_premium_out_of_range_before_any_request";
    if let Ok(origin) = std::env::var("PREMIUM_APP") {
        crate::routes::TEST_APP_ORIGIN.set(origin).unwrap();
        let premium: u16 = std::env::var("PREMIUM_BPS").unwrap().parse().unwrap();
        let client = Client::new("http://127.0.0.1:1", None);
        let server = AgentSwapMcp::new(Config {client:client.clone(),intent_client:client,signer:None,allow_trade:false,trade_max_amount:None});
        let input = serde_json::from_value(json!({"request":"abcdefghijklmnopqrstuv","start_premium_bps":premium})).unwrap();
        let error = tokio::runtime::Runtime::new().unwrap().block_on(server.batch_sell_run(Parameters(input))).err().unwrap();
        assert!(error.contains(if premium > 1000 { "start-premium-bps must be 0..1000" } else { "must be confirmed" }), "{error}");
        return;
    }
    for (premium, requests) in [(1001, 0), (1000, 1)] {
        let app = http::Server::start(vec![(200, String::new(), json!({"id":"abcdefghijklmnopqrstuv","status":"pending","confirmed":null,
            "request":{"v":1,"chainId":8453,"agent":alloy::primitives::Address::repeat_byte(2),"tokens":[],"maxLossBps":500,"purpose":"batch-sell"}}).to_string())]);
        let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
            .env("PREMIUM_APP", &app.url).env("PREMIUM_BPS", premium.to_string()).output().unwrap();
        assert!(output.status.success(), "{premium}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let seen = app.requests.lock().unwrap();
        assert_eq!(seen.len(), requests, "{premium}");
        assert!(seen.iter().all(|r| r.method == "GET" && r.target == "/api/grant-requests/abcdefghijklmnopqrstuv"));
    }
}
