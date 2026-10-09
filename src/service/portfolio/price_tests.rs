// Local HTTP price-contract regressions and exhaustive floor-eligibility combinations.
// Covers exact number text, batching, provenance, absent prices and optional-source failures.
use super::*;
use crate::service::portfolio::http::Server;
use serde_json::json;

#[test]
fn json_prices_never_round_through_floats() {
    let address = Address::repeat_byte(1);
    let text = format!(r#"{{"prices":{{"{address:?}":{{"priceUsd":1.000000000000000001,"source":"defillama"}}}}}}"#);
    assert_eq!(decode(&text)[&address].value.to_string(), "1000000000000000001");
    for price in ["0", "-1", "null", "\"1\""] {
        assert!(decode(&text.replace("1.000000000000000001", price)).is_empty());
    }
    assert!(decode("{}").is_empty());
}

#[test]
fn floor_eligible_truth_table() {
    let address = Address::repeat_byte(1);
    for source in ["defillama", "1inch", "oracle", "quote", "alchemy"] {
        for observed in [None, Some(false), Some(true)] {
            for count in [None, Some(0), Some(1), Some(2), Some(3)] {
                for basis in [None, Some("manual_pin"), Some("stablecoin_par"), Some("onchain_pool"), Some("aggregate"), Some("quote")] {
                    let text = json!({"prices":{format!("{address:?}"):{"priceUsd":1,"source":source,
                        "observed":observed,"sourceCount":count,"basis":basis,"confidence":0.9}}}).to_string();
                    let prices = decode(&text);
                    if matches!(source, "quote" | "alchemy") { assert!(prices.is_empty()); continue; }
                    let trusted = [Some("manual_pin"), Some("stablecoin_par"), Some("onchain_pool")].contains(&basis);
                    assert_eq!(prices[&address].floor_eligible, matches!(source, "defillama" | "1inch")
                        || (observed == Some(true) && (count.unwrap_or(0) >= 2 || trusted)), "{text}");
                }
            }
        }
    }
}

#[tokio::test]
async fn app_prices_batch_lowercase_addresses_on_any_chain() {
    let tokens = (1..=241).map(|n| Address::from_word(U256::from(n).into())).collect::<Vec<_>>();
    let body = format!(r#"{{"prices":{{"{:?}":{{"priceUsd":1e-18,"source":"defillama"}}}}}}"#, tokens[0]);
    let server = Server::start(vec![(200, String::new(), body), (200, String::new(), "{\"prices\":{}}".into())]);
    let result = prices(4663, &tokens, &server.url).await;
    assert_eq!(result.len(), 1);
    assert_eq!(result[&tokens[0]].value, U256::from(1));
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    for (request, count) in requests.iter().zip([240, 1]) {
        assert_eq!(request.method, "GET");
        let url = reqwest::Url::parse(&format!("{}{}", server.url, request.target)).unwrap();
        assert_eq!(url.path(), "/api/prices");
        let query: BTreeMap<_, _> = url.query_pairs().collect();
        assert_eq!(query["chainId"], "4663");
        assert_eq!(query["addresses"].split(',').count(), count);
        assert_eq!(query["addresses"], query["addresses"].to_lowercase());
    }
}

#[tokio::test]
async fn price_source_failure_modes_are_unpriced() {
    for (status, body) in [(500, "{}"), (200, "<html>SPA</html>"), (200, "{}"), (200, "{\"prices\":[]}")] {
        let server = Server::start(vec![(status, String::new(), body.into())]);
        assert!(prices(8453, &[Address::repeat_byte(1)], &server.url).await.is_empty());
    }
    assert!(prices(8453, &[Address::repeat_byte(1)], "http://127.0.0.1:1").await.is_empty());
}

#[test]
fn oneinch_prices_preserve_source_and_drop_unknowns() {
    let address = Address::repeat_byte(1);
    let unknown = Address::repeat_byte(2);
    let text = json!({"prices":{
        format!("{address:?}"):{"priceUsd":2,"source":"1inch","change24h":null,"asOfSec":0},
        format!("{unknown:?}"):{"priceUsd":2,"source":"alchemy","observed":true,"sourceCount":3}
    }}).to_string();
    let prices = decode(&text);
    assert!(!prices.contains_key(&unknown));
    assert_eq!(prices.len(), 1);
    let price = &prices[&address];
    assert_eq!(price.value, amount::fixed("2", true).unwrap());
    assert_eq!(price.source, "1inch");
    assert!(price.confidence.is_none());
    assert!(price.floor_eligible);
}
