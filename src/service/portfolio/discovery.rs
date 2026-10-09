// ERC-20 candidate discovery from registry, explicit addresses, incoming logs and optional indexer.
// Reuses V6 RPC configuration and event windows; balances always come from balanceOf.
use crate::{evm, order_types, tokens};
use alloy::{primitives::{Address, U256, keccak256}, providers::{DynProvider, Provider}, rpc::types::Filter};
use eyre::{Result, eyre};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

alloy::sol! {
    #[sol(rpc)]
    contract BalanceReader {
        function balanceOf(address owner) external view returns (uint256);
    }
}

pub type Candidates = BTreeMap<Address, BTreeSet<String>>;

pub fn config(chain: &str) -> Result<evm::ChainConfig> {
    let config = evm::chain_config(chain)?;
    if !matches!(config.id, 8453 | 42161 | 56 | 4663) { return Err(eyre!("{}", tokens::HOLDINGS_CHAINS_NOTE)); }
    Ok(config)
}

pub fn erc20(value: &str) -> Result<Address> {
    let address = order_types::parse_address(value)?;
    if address == Address::ZERO || address == Address::repeat_byte(0xee) {
        return Err(eyre!("native tokens are not ERC-20 tokens"));
    }
    Ok(address)
}

pub fn add(out: &mut Candidates, address: Address, source: &str) {
    if erc20(&format!("{address:?}")).is_ok() {
        out.entry(address).or_default().insert(source.into());
    }
}

pub async fn discover(provider: &DynProvider, config: evm::ChainConfig, owner: Address,
    explicit: &[String], lookback: Option<u64>) -> Result<(Candidates, u64, u64, Option<String>)> {
    let mut out = Candidates::new();
    for token in tokens::registry_addresses(config.id) { add(&mut out, erc20(token)?, "registry"); }
    for token in explicit { add(&mut out, erc20(token)?, "explicit"); }
    let from = evm::event_start_block(provider, evm::event_lookback_blocks(config, lookback)).await?;
    let latest = provider.get_block_number().await?;
    let mut block = from;
    while block <= latest {
        let end = block.saturating_add(evm::EVENT_CHUNK_SIZE - 1).min(latest);
        let filter = Filter::new().event_signature(keccak256("Transfer(address,address,uint256)"))
            .topic2(owner).from_block(block).to_block(end);
        for log in provider.get_logs(&filter).await.map_err(|e| evm::event_query_error(config, block, end, e))? {
            if log.topics().len() == 3 && log.data().data.len() == 32 {
                add(&mut out, log.address(), "transfer");
            }
        }
        if end == latest { break; }
        block = end.saturating_add(1);
    }
    let indexer = indexer(provider, owner).await;
    if let Some(tokens) = &indexer { for token in tokens { add(&mut out, *token, "indexer"); } }
    Ok((out, from, latest, indexer.map(|_| "alchemy_getTokenBalances".into())))
}

#[derive(Debug, Deserialize)]
struct IndexedBalances {
    #[serde(rename = "tokenBalances")]
    tokens: Vec<IndexedToken>,
    #[serde(rename = "pageKey")]
    page_key: Option<String>,
}
#[derive(Debug, Deserialize)]
struct IndexedToken {
    #[serde(rename = "contractAddress")]
    address: String,
}

async fn indexer(provider: &DynProvider, owner: Address) -> Option<Vec<Address>> {
    let mut params = serde_json::json!([owner, "erc20"]);
    let mut tokens = Vec::new();
    let mut cursors = BTreeSet::new();
    loop {
        let result: IndexedBalances = provider.raw_request("alchemy_getTokenBalances".into(), params).await.ok()?;
        for token in result.tokens { tokens.push(erc20(&token.address).ok()?); }
        let Some(cursor) = result.page_key.filter(|key| !key.is_empty()) else { return Some(tokens); };
        if !cursors.insert(cursor.clone()) { return None; }
        params = serde_json::json!([owner, "erc20", {"pageKey": cursor}]);
    }
}

pub async fn balance(provider: &DynProvider, token: Address, owner: Address) -> Result<U256> {
    Ok(BalanceReader::new(token, provider.clone()).balanceOf(owner).call().await?)
}
