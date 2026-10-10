// Net-proceeds reporting from actual decoded RPC logs.
// Fee-bearing intent fills and market receipts never substitute a quote for received value.
use super::*;
use alloy::{primitives::U256, sol_types::SolEvent};
use crate::service::test_rpc::{TestRpc, ok};
use serde_json::json;

#[tokio::test]
async fn batch_sell_received_decodes_net_fill_and_market_receipt() {
    let config = evm::chain_config("8453").unwrap();
    let proxy = Address::repeat_byte(6);
    let receive = Address::repeat_byte(3);
    let id = B256::repeat_byte(1);
    let fill = IntentSettlerV3::IntentFilled { id, owner:Address::repeat_byte(4), solver:Address::repeat_byte(5),
        recipient:Address::repeat_byte(4), caller:Address::repeat_byte(5), amountIn:U256::from(100), requiredOut:U256::from(90),
        fee:U256::from(3), receivedOut:U256::from(98), aboveFloor:U256::from(8) }.encode_log_data();
    let market = ExecutedAsAgent { agent:Address::repeat_byte(2), router:Address::repeat_byte(5), relayer:Address::repeat_byte(2),
        tokenIn:Address::repeat_byte(1), tokenOut:receive, amountIn:U256::from(100), amountOut:U256::from(97) }.encode_log_data();
    let rpc = TestRpc::start(move |body| Some(ok(body, match body["method"].as_str().unwrap() {
        "eth_blockNumber" => json!("0x2"),
        "eth_getLogs" => json!([{"address":config.settler,"topics":fill.topics(),"data":fill.data,"blockNumber":"0x1","logIndex":"0x0","transactionIndex":"0x0"}]),
        "eth_getTransactionReceipt" => json!({"type":"0x2","status":"0x1","cumulativeGasUsed":"0x5208",
            "logs":[{"address":proxy,"topics":market.topics(),"data":market.data,"blockNumber":"0x1","logIndex":"0x0","transactionIndex":"0x0"}],
            "logsBloom":format!("0x{}","0".repeat(512)),"transactionHash":id,"transactionIndex":"0x0", "blockHash":B256::ZERO,
            "blockNumber":"0x1","gasUsed":"0x5208","effectiveGasPrice":"0x1","from":Address::ZERO,"to":proxy,"contractAddress":null}),
        other => panic!("unexpected {other}"),
    })));
    let provider = evm::read_provider(&rpc.url).unwrap();
    let mut row = Row::new(Address::repeat_byte(1).to_string());
    row.amount_raw = "100".into(); row.intent_id = Some(id.to_string()); row.placement_block = Some(1);
    assert_eq!(intent_received(&provider, config, &row).await.unwrap(), "95");
    row.tx_hash = Some(id.to_string());
    assert_eq!(market_received(&provider, proxy, receive, &row).await.unwrap(), "97");
    assert!(market_received(&provider, Address::ZERO, receive, &row).await.is_err());
}

#[tokio::test]
async fn batch_sell_received_prefers_the_index_fill_record_over_logs() {
    let id = B256::repeat_byte(1);
    let fill = crate::service::intentscan::fixture::fill(id, "95", B256::repeat_byte(2), 7_000);
    let index = crate::service::test_http::TestHttp::start(move |target| {
        if target == format!("/v1/intent/{id:?}") { (200, fill.clone()) } else { (404, "{}".into()) }
    });
    let mut row = Row::new(Address::repeat_byte(1).to_string());
    row.intent_id = Some(format!("{id:?}")); row.intent_status = Some("filled".into());
    let mut output = Output { dry_run: false, owner: Address::repeat_byte(4).to_string(), note: None, tokens: vec![row] };
    let origins = Origins { stream: "http://127.0.0.1:1", data: &index.url };
    report("8453", &Address::repeat_byte(6).to_string(), &Address::repeat_byte(3).to_string(), &origins, &mut output).await.unwrap();
    assert_eq!((output.tokens[0].received_raw.as_deref(), output.tokens[0].outcome.as_str()), (Some("95"), "sold"));
    assert!(output.tokens[0].warnings.is_empty(), "no log fallback was needed");
}
