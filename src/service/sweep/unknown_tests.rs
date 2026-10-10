// A relay HTTP 504 (stream publish timed out) keeps the signed intent as placement_unknown; the
// status wait polls it by its order id, and an index fill settles it as sold, never re-signed.
use super::*;
use super::intent_tests::{rpc_sharing, stream_index};
use crate::service::batch_sell;
use serde_json::json;
#[path = "../../client/test_server.rs"]
mod relay_server;

fn signer() -> Arc<crate::signer::local::LocalKey> {
    Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap())
}

#[test]
fn timed_out_publish_is_polled_and_settles_from_an_index_fill() {
    const NAME: &str = "service::sweep::unknown_tests::timed_out_publish_is_polled_and_settles_from_an_index_fill";
    if let Ok(app) = std::env::var("UNKNOWN_APP") {
        crate::routes::TEST_APP_ORIGIN.set(app).unwrap();
        crate::routes::TEST_INTENTSCAN_ORIGIN.set(std::env::var("UNKNOWN_INDEX").unwrap()).unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(child()); return;
    }
    let weth = token::from_registry("WETH", 8453).unwrap().address;
    let orders = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (rpc, index) = (rpc_sharing(5, "missing", orders.clone()), stream_index(orders.clone(), "filled", signer().address()));
    let relay = relay_server::Server::start(vec![(504, String::new(), json!({"error": "stream_publish_unknown", "retry": "poll the intent id"}).to_string())]);
    let record = json!({"id": "abcdefghijklmnopqrstuv", "status": "confirmed", "confirmed": {"proxy": Address::repeat_byte(6), "generation": "1", "maxLossBps": 100},
        "request": {"v": 1, "chainId": 8453, "agent": signer().address(), "owner": Address::repeat_byte(4), "purpose": "batch-sell", "maxLossBps": 500,
        "tokens": [{"address": Address::repeat_byte(1), "cap": "1"}, {"address": weth, "cap": "0"}]}});
    let prices = json!({"prices": {Address::repeat_byte(1).to_string(): {"priceUsd": 2, "source": "defillama"}, weth.clone(): {"priceUsd": 4, "source": "defillama"}}});
    let app = relay_server::Server::start(vec![(200, String::new(), record.to_string()), (200, String::new(), prices.to_string())]);
    let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
        .env("UNKNOWN_APP", &app.url).env("UNKNOWN_INDEX", &index.url).env("UNKNOWN_RELAY", &relay.url)
        .env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let requests = relay.requests.lock().unwrap();
    assert_eq!(requests.len(), 1, "a 504 is not resent");
    let sent = &requests[0];
    assert_eq!((sent.method.as_str(), sent.target.as_str(), sent.keyed, sent.paid, sent.body.is_empty()), ("POST", crate::routes::INTENT_ANNOUNCE, false, false, false));
    assert_eq!(orders.lock().unwrap().len(), 1, "one signed order, never re-signed");
    assert_eq!((rpc.called("eth_getLogs"), rpc.called("eth_sendRawTransaction")), (0, 0));
}

async fn child() {
    let input: batch_sell::RunInput = serde_json::from_value(json!({"request": "abcdefghijklmnopqrstuv", "wait": 20})).unwrap();
    let record = batch_sell::load(&input).await.unwrap();
    let relay = Client::new(&std::env::var("UNKNOWN_RELAY").unwrap(), None);
    let output = batch_sell::run(&relay, signer(), input, record, true, None, Wait::MCP).await.unwrap();
    let row = output.tokens.iter().find(|r| r.intent_id.is_some()).expect("the timed-out intent keeps its id");
    let order = row.order.as_ref().expect("the signed order is kept");
    assert_eq!(row.intent_id.as_deref(), Some(format!("{:?}", order_types::order_id(order)).as_str()));
    assert_eq!((row.announce_status.as_deref(), row.intent_status.as_deref(), row.wait_timed_out), (Some("accepted"), Some("filled"), false));
    assert_eq!((row.outcome.as_str(), row.received_raw.as_deref()), ("sold", Some("777")));
    assert!(row.error.as_deref().unwrap().starts_with("HTTP 504"), "{row:?}");
    output.check().unwrap();
}
