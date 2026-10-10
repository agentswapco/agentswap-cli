// Intent-index client: owner history paging, decoding and filtering, and fill-record parsing.
// Every response comes from a loopback fixture; no test reaches the public index.
use super::{fixture, *};
use crate::service::test_http::TestHttp;
use alloy::primitives::Address;

fn owner() -> Address { Address::repeat_byte(4) }

#[tokio::test]
async fn history_pages_by_owner_and_keeps_only_matching_orders_in_the_window() {
    let a = fixture::order(owner(), Address::repeat_byte(1), 10, Address::repeat_byte(3), 5, 1);
    let b = fixture::order(owner(), Address::repeat_byte(2), 20, Address::repeat_byte(3), 6, 2);
    let old = fixture::order(owner(), Address::repeat_byte(2), 30, Address::repeat_byte(3), 7, 3);
    let mut forged = fixture::item(8453, &b, Address::repeat_byte(2), 1, "open", 4_000, 9_000);
    forged["order_hash"] = serde_json::json!(format!("{:?}", B256::repeat_byte(9)));
    let first = fixture::page(vec![fixture::item(8453, &a, Address::repeat_byte(2), 1, "filled", 5_000, 9_000),
        fixture::item(56, &a, Address::repeat_byte(2), 1, "open", 4_500, 9_000), forged], Some(4_000));
    let second = fixture::page(vec![fixture::item(8453, &b, Address::repeat_byte(2), 7, "open", 3_000, 9_000),
        fixture::item(8453, &old, Address::repeat_byte(2), 1, "expired", 1_000, 2_000)], Some(1_000));
    let server = TestHttp::start(move |target| (200, if target.contains("before_ms=4000") { second.clone() } else { first.clone() }));
    let origins = Origins { stream: &server.url, data: "http://127.0.0.1:1" };
    let intents = history(&origins, owner(), 8453, 2_000).await.unwrap();
    assert_eq!(intents.iter().map(|i| (i.id, i.status.as_str(), i.agent)).collect::<Vec<_>>(), vec![
        (crate::order_types::order_id(&a), "filled", Some((Address::repeat_byte(2), 1))),
        (crate::order_types::order_id(&b), "open", Some((Address::repeat_byte(2), 7)))]);
    assert_eq!(intents[1].order.amountIn, U256::from(20));
    let targets = server.targets();
    assert_eq!(targets.len(), 2, "paging stops once a page reaches past the window");
    assert_eq!(targets[0], format!("/v1/intents?owner={:?}&chain_ids=8453&limit=500", owner()));
    assert!(targets[1].ends_with("&before_ms=4000"));
}

#[tokio::test]
async fn history_fails_on_an_error_answer() {
    let server = TestHttp::start(|_| (503, "{}".into()));
    let origins = Origins { stream: &server.url, data: "http://127.0.0.1:1" };
    assert!(history(&origins, owner(), 8453, 0).await.unwrap_err().to_string().contains("HTTP 503"));
}

#[tokio::test]
async fn fill_reads_net_output_and_ignores_records_without_a_fill() {
    let (id, other, tx) = (B256::repeat_byte(1), B256::repeat_byte(2), B256::repeat_byte(3));
    let server = TestHttp::start(move |target| match target {
        t if t.ends_with(&format!("{id:?}")) => (200, fixture::fill(id, "95", tx, 7_000)),
        t if t.ends_with(&format!("{other:?}")) => (200, serde_json::json!({"intent_hash": format!("{other:?}"), "record_type": "intent", "status": "open"}).to_string()),
        t if t.ends_with(&format!("{tx:?}")) => (200, fixture::fill(id, "95", tx, 7_000)),
        t if t.contains("0x0404") => (500, "{}".into()),
        _ => (404, r#"{"error":"not_found"}"#.into()),
    });
    let origins = Origins { stream: "http://127.0.0.1:1", data: &server.url };
    assert_eq!(fill(&origins, id).await.unwrap(), Some(Fill { received: U256::from(95), tx_hash: format!("{tx:?}"), filled_ms: 7_000 }));
    assert_eq!(fill(&origins, other).await.unwrap(), None, "an intent record without a fill");
    assert_eq!(fill(&origins, tx).await.unwrap(), None, "a record for another intent");
    assert_eq!(fill(&origins, B256::repeat_byte(5)).await.unwrap(), None);
    assert!(fill(&origins, B256::repeat_byte(4)).await.is_err());
    assert_eq!(server.targets()[0], format!("/v1/intent/{id:?}"));
}
