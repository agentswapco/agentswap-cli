// Batch-sell MCP validation uses the same request DTOs as the CLI.
// Confirmed discount cannot be supplied as an execution override.
use super::*;
use serde_json::json;

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
