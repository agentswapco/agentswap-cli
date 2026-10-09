// App contract-address prices parsed directly from JSON number text into fixed point.
// Missing or invalid prices remain unpriced; provenance determines sale-floor eligibility.
use super::amount;
use alloy::primitives::{Address, U256};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Response { prices: BTreeMap<String, Coin> }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Coin {
    price_usd: serde_json::Number,
    source: String,
    confidence: Option<serde_json::Number>,
    basis: Option<String>,
    observed: Option<bool>,
    source_count: Option<u64>,
}

pub struct Price {
    pub value: U256,
    pub source: String,
    pub confidence: Option<serde_json::Number>,
    pub basis: Option<String>,
    pub observed: Option<bool>,
    pub source_count: Option<u64>,
    pub floor_eligible: bool,
}

pub async fn prices(chain: u64, tokens: &[Address], origin: &str) -> BTreeMap<Address, Price> {
    let Ok(client) = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(15)).build() else { return BTreeMap::new(); };
    let mut prices = BTreeMap::new();
    for chunk in tokens.chunks(240) {
        let addresses = chunk.iter().map(|a| format!("{a:?}")).collect::<Vec<_>>().join(",");
        let Ok(response) = client.get(format!("{origin}/api/prices"))
            .query(&[("chainId", chain.to_string()), ("addresses", addresses)]).send().await else { continue; };
        if !response.status().is_success() { continue; }
        let Ok(text) = response.text().await else { continue; };
        prices.extend(decode(&text).into_iter().filter(|(address, _)| chunk.contains(address)));
    }
    prices
}

fn decode(text: &str) -> BTreeMap<Address, Price> {
    let Ok(response) = serde_json::from_str::<Response>(text) else { return BTreeMap::new(); };
    response.prices.into_iter().filter_map(|(address, coin)| {
        let value = amount::fixed(&coin.price_usd.to_string(), true).ok()?;
        if value == U256::ZERO || !matches!(coin.source.as_str(), "oracle" | "defillama") { return None; }
        let floor_eligible = coin.source == "defillama" || (coin.observed == Some(true)
            && (coin.source_count.is_some_and(|n| n >= 2)
                || matches!(coin.basis.as_deref(), Some("manual_pin" | "stablecoin_par" | "onchain_pool"))));
        Some((address.parse().ok()?, Price { value, source: coin.source, confidence: coin.confidence,
            basis: coin.basis, observed: coin.observed, source_count: coin.source_count, floor_eligible }))
    }).collect()
}

#[cfg(test)]
#[path = "price_tests.rs"]
mod tests;
