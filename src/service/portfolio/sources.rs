// Optional app holdings and live service catalog discovery.
// Validates indexed balances and metadata; failures contribute no holdings.
use super::discovery::{Candidates, add, erc20};
use crate::{client::Client, order_types, service::market};
use alloy::primitives::{Address, U256};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(Debug, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WalletTokens { Indexed, Unindexed, Unavailable }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Holdings {
    chain_id: u64,
    owner: Address,
    indexed: bool,
    truncated: bool,
    tokens: Vec<Holding>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Holding {
    address: String,
    balance_raw: String,
    decimals: Option<u8>,
    symbol: Option<String>,
    name: Option<String>,
    price_usd: Option<String>,
}

pub async fn wallet_tokens(origin: &str, chain: u64, owner: Address) -> (WalletTokens, bool, Vec<super::Row>) {
    match holdings(origin, chain, owner).await {
        Ok(Some((tokens, truncated))) => {
            (WalletTokens::Indexed, truncated, tokens)
        }
        Ok(None) => (WalletTokens::Unindexed, false, Vec::new()),
        Err(_) => (WalletTokens::Unavailable, false, Vec::new()),
    }
}

async fn holdings(origin: &str, chain: u64, owner: Address) -> eyre::Result<Option<(Vec<super::Row>, bool)>> {
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(15)).build()?;
    let response = client.get(format!("{origin}/api/wallet-tokens"))
        .query(&[("chain", chain.to_string()), ("owner", format!("{owner:?}"))])
        .send().await?;
    eyre::ensure!(response.status().is_success(), "wallet-tokens HTTP failure");
    let response = response.json::<Holdings>().await?;
    eyre::ensure!(response.chain_id == chain && response.owner == owner, "wallet-tokens identity mismatch");
    if !response.indexed { return Ok(None); }
    let mut tokens = std::collections::BTreeMap::new();
    for token in response.tokens {
        let address = erc20(&token.address)?;
        if order_types::parse_u256(&token.balance_raw)? != U256::ZERO { tokens.insert(address, holding_row(address, token)); }
    }
    Ok(Some((tokens.into_values().collect(), response.truncated)))
}

fn holding_row(address: Address, token: Holding) -> super::Row {
    let short = crate::display::short_addr(&address.to_string());
    let symbol = token.symbol.filter(|s| !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_graphic()))
        .map(|s| format!("{s} ({short})")).unwrap_or(short);
    let name = token.name.filter(|s| s.len() <= 128 && s.chars().all(|c| c.is_ascii_graphic() || c == ' '));
    let price = token.price_usd.and_then(|s| super::amount::fixed(&s, false).ok()).filter(|p| *p != U256::ZERO);
    super::Row { address: format!("{address:?}"), symbol, name, decimals: token.decimals,
        balance_raw: Some(token.balance_raw), price_usd: price.map(|p| super::amount::render(&p.to_string(), 18)),
        value_usd: None, source: price.map(|_| "alchemy".into()), confidence: None, basis: None, observed: None,
        source_count: None, floor_eligible: false, sources: vec!["wallet-tokens".into()],
        status: if token.decimals.is_some() { "unpriced" } else { "metadata_error" }.into(), quote_out_raw: None, error: None, dust: false }
}

pub async fn catalog(client: &Client, chain: u64, out: &mut Candidates) -> eyre::Result<()> {
    #[derive(Deserialize)]
    struct Entry { chain_id: u64 }
    let response = market::tokens(client).await?;
    let entries: std::collections::BTreeMap<String, Entry> = serde_json::from_value(response)?;
    let mut addresses = Vec::new();
    for (address, entry) in entries {
        if entry.chain_id == chain { addresses.push(erc20(&address)?); }
    }
    for address in addresses { add(out, address, "catalog"); }
    Ok(())
}
