// Optional app holdings and live service catalog discovery.
// Validates remote shapes; failures contribute no candidates and never supply authoritative balances.
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
struct Holding { address: String, balance_raw: String }

pub async fn wallet_tokens(origin: &str, chain: u64, owner: Address, out: &mut Candidates) -> (WalletTokens, bool) {
    match holdings(origin, chain, owner).await {
        Ok(Some((tokens, truncated))) => {
            for token in tokens { add(out, token, "wallet-tokens"); }
            (WalletTokens::Indexed, truncated)
        }
        Ok(None) => (WalletTokens::Unindexed, false),
        Err(_) => (WalletTokens::Unavailable, false),
    }
}

async fn holdings(origin: &str, chain: u64, owner: Address) -> eyre::Result<Option<(Vec<Address>, bool)>> {
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(15)).build()?;
    let response = client.get(format!("{origin}/api/wallet-tokens"))
        .query(&[("chain", chain.to_string()), ("owner", format!("{owner:?}"))])
        .send().await?;
    eyre::ensure!(response.status().is_success(), "wallet-tokens HTTP failure");
    let response = response.json::<Holdings>().await?;
    eyre::ensure!(response.chain_id == chain && response.owner == owner, "wallet-tokens identity mismatch");
    if !response.indexed { return Ok(None); }
    let mut tokens = Vec::new();
    for token in response.tokens {
        let address = erc20(&token.address)?;
        if order_types::parse_u256(&token.balance_raw)? != U256::ZERO { tokens.push(address); }
    }
    Ok(Some((tokens, response.truncated)))
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
