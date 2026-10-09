// Checked sweep quotes remain bound to their exact request during trade execution.
// An altered request is refused instead of fetching a replacement quote.
use super::*;
use serde_json::json;

#[tokio::test]
async fn pinned_sweep_quote_is_exact_and_does_not_change_other_clients() {
    let server = test_server::Server::start(vec![(200, String::new(), json!({"output":"1"}).to_string())]);
    let client = Client::new(&server.url, None);
    let request = json!({"chain_id":8453,"amount_in":"1000000","verify":true});
    let checked = json!({"output":"990000","execution":{"calldata":"0x"}});
    let pinned = client.clone().with_pinned_quote(request.clone(), checked.clone());
    assert_eq!(pinned.quote(&request).await.unwrap(), checked);
    for (field, value) in [("amount_in", json!("1000001")), ("chain_id", json!(56)), ("verify", json!(false))] {
        let mut changed = request.clone(); changed[field] = value;
        assert!(pinned.quote(&changed).await.unwrap_err().to_string().contains("differs"));
    }
    assert_eq!(server.requests.lock().unwrap().len(), 0);
    assert_eq!(client.quote(&request).await.unwrap()["output"], "1");
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}
