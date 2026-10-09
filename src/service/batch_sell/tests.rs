// Exact cap and request identity validation at the sale boundary.
// Invalid URLs and malformed confirmations fail before chain reads or signing.
use super::{models::*, run::execution, request, raw_cap};
use alloy::primitives::{Address, U256};
use serde_json::json;

#[test]
fn batch_sell_request_caps_and_identity_validation() {
    for (cap, decimals, raw) in [("1.2345",6,"1234500"),("0.000000000000000000000001",24,"1"),("0",6,"0")] {
        assert_eq!(raw_cap(cap,decimals).unwrap().to_string(),raw);
    }
    for cap in ["-1", "1e2", "1.0001", ".1"] { assert!(raw_cap(cap,3).is_err()); }
    assert_eq!(request::id("https://app.agentswap.co/grant/r/abcdefghijklmnopqrstuv").unwrap(),"abcdefghijklmnopqrstuv");
    for url in ["https://evil.test/grant/r/abcdefghijklmnopqrstuv", "../abcdefghijklmnopqrstuv", "https://app.agentswap.co/grant/r/abcdefghijklmnopqrstuv?x=1", "short"] {
        assert!(request::id(url).is_err());
    }
    let mut record: Record = serde_json::from_value(json!({"id":"abcdefghijklmnopqrstuv","status":"confirmed",
        "confirmed":{"proxy":Address::repeat_byte(6),"generation":"1","maxLossBps":100},
        "request":{"v":1,"chainId":8453,"agent":Address::repeat_byte(2),"purpose":"batch-sell","maxLossBps":500,
        "tokens":[{"address":Address::repeat_byte(1),"cap":"1"},{"address":Address::repeat_byte(3),"cap":"0"}]}})).unwrap();
    let input = RunInput {request:record.id.clone(),via:Via::Market,wait:None};
    let sale = execution(&input,&record,Address::repeat_byte(2)).unwrap();
    assert_eq!(sale.max_loss_bps,100); assert!(sale.self_submit); assert_eq!(sale.tokens.len(),1);
    assert_eq!(raw_cap(&sale.request_caps[&sale.tokens[0]],6).unwrap(),U256::from(1_000_000));
    assert!(execution(&input,&record,Address::repeat_byte(9)).is_err());
    record.confirmed.as_mut().unwrap().max_loss_bps = 501;
    assert!(execution(&input,&record,Address::repeat_byte(2)).is_err());
    record.confirmed.as_mut().unwrap().max_loss_bps = 100;
    record.request.tokens[0].cap = "0".into();
    assert!(execution(&input,&record,Address::repeat_byte(2)).is_err());
}
