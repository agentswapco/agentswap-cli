// MCP sweep schema and early-refusal coverage.
// Verifies required risk inputs, advertised tool discovery and key/cap validation.
use super::*;
use serde_json::json;

#[tokio::test]
async fn sweep_schema_discovery_defaults_and_key_requirement() {
    let client = Client::new("http://127.0.0.1:1", None);
    let mut server = AgentSwapMcp::new(Config { client: client.clone(), intent_client: client,
        signer: None, allow_trade: false, trade_max_amount: None });
    let tools = server.tool_router.list_all();
    assert_eq!(tools.len(), 12);
    let sweep = tools.iter().find(|t| t.name == "sweep").unwrap();
    let schema = serde_json::to_value(&sweep.input_schema).unwrap();
    let required = schema["required"].as_array().unwrap();
    for field in ["chain_id", "proxy", "receive", "max_usd", "max_loss_bps", "tokens"] { assert!(required.contains(&json!(field))); }
    assert!(!required.contains(&json!("dry_run")));
    assert_eq!(schema["properties"]["tokens"]["minItems"], json!(1));
    assert!(schema["properties"].get("lookback_blocks").is_none());
    assert_eq!(schema["properties"]["max_loss_bps"]["maximum"], json!(9999));
    assert!(server.get_info().instructions.unwrap().contains("sweep"));
    let input: sweep::Input = serde_json::from_value(json!({"chain_id":"8453", "proxy":"proxy", "receive":"USDC", "max_usd":"5", "max_loss_bps":10,"tokens":["token"]})).unwrap();
    assert!(input.dry_run);
    assert!(server.sweep(Parameters(input.clone())).await.err().unwrap().contains("--key-file"));
    server.signer = Some(Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap()));
    server.trade_max_amount = Some("bad".into());
    assert!(server.sweep(Parameters(input)).await.err().unwrap().contains("sweep max-amount"));
}

#[tokio::test]
async fn sweep_mcp_refuses_10000_loss_bps() {
    let client = Client::new("http://127.0.0.1:1", None);
    let server = AgentSwapMcp::new(Config { client: client.clone(), intent_client: client,
        signer: Some(Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap())),
        allow_trade: true, trade_max_amount: Some("bad".into()) });
    let input = serde_json::from_value(json!({"chain_id":"8453", "proxy":"proxy", "receive":"USDC", "max_usd":"5", "max_loss_bps":10000,"tokens":["token"]})).unwrap();
    let error = server.sweep(Parameters(input)).await.err().unwrap();
    assert!(error.contains("max-loss-bps must be less than 10000"), "{error}");
}

#[test]
fn gasless_sweep_mcp_schema() {
    let client = Client::new("http://127.0.0.1:1", None);
    let server = AgentSwapMcp::new(Config { client: client.clone(), intent_client: client,
        signer: None, allow_trade: false, trade_max_amount: None });
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "sweep").unwrap();
    let schema = serde_json::to_value(&tool.input_schema).unwrap();
    assert_eq!(schema["properties"]["via"]["default"], "intent");
    assert!(schema["properties"].get("wait").is_some());
    assert!(schema.to_string().contains("market"));
    let input = json!({"chain_id":"8453", "proxy":"proxy", "receive":"USDC", "max_usd":"5", "max_loss_bps":10,"tokens":["token"], "via":"invalid"});
    assert!(serde_json::from_value::<sweep::Input>(input).is_err());
}
