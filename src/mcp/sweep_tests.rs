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
    for field in ["chain_id", "proxy", "receive", "max_usd", "max_loss_bps"] { assert!(required.contains(&json!(field))); }
    assert!(!required.contains(&json!("dry_run")));
    assert!(server.get_info().instructions.unwrap().contains("sweep"));
    let input: sweep::Input = serde_json::from_value(json!({"chain_id":"8453", "proxy":"proxy", "receive":"USDC", "max_usd":"5", "max_loss_bps":10})).unwrap();
    assert!(input.dry_run);
    assert!(server.sweep(Parameters(input.clone())).await.unwrap_err().contains("--key-file"));
    server.signer = Some(Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap()));
    server.trade_max_amount = Some("bad".into());
    assert!(server.sweep(Parameters(input)).await.unwrap_err().contains("sweep max-amount"));
}
