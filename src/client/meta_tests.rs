// Hermetic BSC adapter and portfolio regressions using independent loopback backends.
// Exercises wire format, fail-closed responses and actionable portfolio errors.
use super::{Client, test_server::Server};
use alloy::primitives::Address;
use serde_json::{Value, json};

pub(crate) fn best() -> Value {
    json!({"best":{"provider":"OKX","amountOut":"1000000000000000000",
        "target":Address::repeat_byte(8),"approveTarget":Address::repeat_byte(9),
        "calldata":"0x12345678","value":"0","gasEstimate":123456,
        "taxInBps":0,"taxOutBps":0,"simulation":{"status":"success","actualOutput":"999"}}})
}

fn request(chain: u64) -> Value {
    json!({"chain_id":chain,"token_in":Address::repeat_byte(1),"token_out":Address::repeat_byte(3),
        "amount_in":"1000000","taker":Address::repeat_byte(2),"slippage_bps":100,"verify":true})
}

pub(crate) fn client(legacy: &str, meta: &str) -> Client {
    let mut client = Client::new(legacy, Some("synthetic-key".into()));
    client.meta_quote_url = Some(format!("{meta}/quote"));
    client
}

#[tokio::test]
async fn bsc_backend_request_and_normalization_leave_other_chains_unchanged() {
    let meta = Server::start(vec![(200, String::new(), best().to_string())]);
    let legacy = Server::start(vec![(200, String::new(), json!({"output":"42"}).to_string())]);
    let client = client(&legacy.url, &meta.url);
    let response = client.quote(&request(56)).await.unwrap();
    assert_eq!(response["output"], "1000000000000000000");
    assert_eq!(response["router"], best()["best"]["target"]);
    assert_eq!(response["execution"]["spender"], best()["best"]["approveTarget"]);
    assert_eq!(response["execution"]["calldata"], "0x12345678");
    let seen = meta.requests.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].target, "/quote");
    assert!(!seen[0].keyed && !seen[0].paid);
    let body: Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body, json!({"chainId":56,"tokenIn":Address::repeat_byte(1),
        "tokenOut":Address::repeat_byte(3),"amountIn":"1000000","taker":Address::repeat_byte(2),"slippageBps":100}));
    drop(seen);
    for chain in [1, 8453, 42161, 4663, 5042, 5042002] {
        assert_eq!(client.quote(&request(chain)).await.unwrap()["output"], "42");
        let seen = legacy.requests.lock().unwrap();
        let sent: Value = serde_json::from_str(&seen.last().unwrap().body).unwrap();
        let mut expected = request(chain); expected.as_object_mut().unwrap().remove("taker");
        assert_eq!(sent, expected);
        assert!(seen.last().unwrap().keyed);
    }
    assert_eq!(meta.requests.lock().unwrap().len(), 1);
}

async fn refused(response: Value) {
    let meta = Server::start(vec![(200, String::new(), response.to_string())]);
    let legacy = Server::start(vec![(200, String::new(), "{}".into())]);
    let error = client(&legacy.url, &meta.url).quote(&request(56)).await.unwrap_err();
    assert!(error.to_string().contains("no executable route"), "{error}");
    assert!(legacy.requests.lock().unwrap().is_empty());
    assert_eq!(meta.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn bsc_refuses_absent_best() {
    for response in [json!({}), json!({"best":null})] { refused(response).await; }
}

#[tokio::test]
async fn bsc_refuses_unsuccessful_simulation() {
    for status in [json!("failed"), json!("skipped"), json!("pending"), Value::Null] {
        let mut response = best(); response["best"]["simulation"]["status"] = status;
        refused(response).await;
    }
}

#[tokio::test]
async fn bsc_refuses_nonzero_or_missing_value() {
    for value in [json!("1"), json!("0x0"), json!(0), Value::Null] {
        let mut response = best(); response["best"]["value"] = value;
        refused(response).await;
    }
}

#[tokio::test]
async fn bsc_portfolio_uses_owner_proxy_and_preserves_quote_failures() {
    use crate::service::portfolio::{self, tests};
    let rpc = tests::fixture(false, false, true, 6);
    let provider = crate::evm::read_provider(&rpc.url).unwrap();
    for (chain, status, response, expected) in [
        (56, 200, best().to_string(), "priced"),
        (56, 200, json!({"best":null}).to_string(), "no_route"),
        (56, 500, "upstream https://user:pass@rpc.example/SECRET?key=SECRET failed".into(), "quote_failed"),
        (8453, 400, "unsupported chain_id".into(), "quote_failed"),
        (8453, 404, "unknown endpoint".into(), "quote_failed"),
        (8453, 404, "no executable route found".into(), "no_route"),
        (8453, 200, "{\"output\":\"0\"}".into(), "no_route"),
        (8453, 200, "{}".into(), "quote_failed"),
    ] {
        let app = Server::start(vec![(200, String::new(), json!({"chainId":chain,"owner":Address::repeat_byte(4),
            "indexed":true,"truncated":false,"tokens":[{"address":Address::repeat_byte(1),
            "balanceRaw":"1000000","decimals":6,"symbol":"TEST","priceUsd":"1"}]}).to_string()),
            (200, String::new(), json!({"prices":{}}).to_string())]);
        let server = Server::start(vec![(status, String::new(), response)]);
        let out = portfolio::read(&client(&server.url, &server.url), tests::input(&chain.to_string()), &provider, &app.url).await.unwrap();
        let row = serde_json::to_value(&out.tokens[0]).unwrap();
        assert_eq!(row["status"], expected, "{row}");
        if expected == "quote_failed" {
            let error = row["error"].as_str().unwrap();
            assert!(!error.contains("SECRET") && !error.contains("user:pass"));
            if status != 200 { assert!(error.contains(&format!("HTTP {status}"))); }
        } else { assert!(row["error"].is_null()); }
        if expected == "priced" {
            assert_eq!(row["quote_out_raw"], "1000000000000000000");
            assert_eq!(row["dust"], true);
            let seen = server.requests.lock().unwrap();
            let request: Value = serde_json::from_str(&seen[0].body).unwrap();
            assert_eq!(request["taker"], json!(Address::repeat_byte(2)));
            assert_ne!(request["taker"], json!(Address::repeat_byte(4)));
            assert_eq!(request["slippageBps"], 50);
        }
    }
}
