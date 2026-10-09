// DefiLlama contract-address prices parsed directly from JSON number text into fixed point.
// Failed requests or missing/invalid prices leave the corresponding token unpriced.
use super::amount;
use alloy::primitives::{Address, U256};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Response { coins: BTreeMap<String, Coin> }
#[derive(Deserialize)]
struct Coin { price: Option<Box<serde_json::value::RawValue>> }

pub async fn prices(chain: u64, tokens: &[Address], endpoint: &str) -> BTreeMap<Address, U256> {
    let key = match chain { 8453 => "base", 42161 => "arbitrum", 56 => "bsc", _ => return BTreeMap::new() };
    let Ok(client) = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build() else { return BTreeMap::new(); };
    let mut prices = BTreeMap::new();
    for chunk in tokens.chunks(50) {
        let keys = chunk.iter().map(|a| format!("{key}:{a:?}")).collect::<Vec<_>>().join(",");
        let Ok(response) = client.get(format!("{endpoint}/{keys}")).send().await else { continue; };
        let Ok(response) = response.error_for_status() else { continue; };
        let Ok(text) = response.text().await else { continue; };
        prices.extend(decode(&text, key));
    }
    prices
}

fn decode(text: &str, chain: &str) -> BTreeMap<Address, U256> {
    let Ok(response) = serde_json::from_str::<Response>(text) else { return BTreeMap::new(); };
    response.coins.into_iter().filter_map(|(key, coin)| {
        let (prefix, address) = key.split_once(':')?;
        if prefix != chain { return None; }
        let price = amount::fixed(coin.price.as_ref()?.get(), true).ok()?;
        if price == U256::ZERO { return None; }
        Some((address.parse().ok()?, price))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_prices_never_round_through_floats() {
        let address = Address::repeat_byte(1);
        let text = format!(r#"{{"coins":{{"base:{address:?}":{{"price":1.000000000000000001}}}}}}"#);
        assert_eq!(decode(&text, "base")[&address].to_string(), "1000000000000000001");
        assert!(decode(&text, "bsc").is_empty());
        assert!(decode("{}", "base").is_empty());
    }
    #[tokio::test]
    async fn robinhood_and_failed_http_are_unpriced() {
        assert!(prices(4663, &[Address::ZERO], "invalid").await.is_empty());
        assert!(prices(8453, &[Address::ZERO], "http://127.0.0.1:1").await.is_empty());
    }
}
