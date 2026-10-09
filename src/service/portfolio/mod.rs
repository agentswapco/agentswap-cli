// Read-only portfolio orchestration shared by CLI and MCP.
// Discovers ERC-20 holdings, reads bounded concurrent balances, values and optionally quotes them.
pub(crate) mod amount;
pub(crate) mod discovery;
mod prices;
#[cfg(test)]
pub(crate) mod tests;

use alloy::{primitives::{Address, U256}, providers::DynProvider};
use crate::{client::Client, evm, order_types, service::{quote, token}};
use eyre::Result;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(Debug, Clone, Deserialize, JsonSchema, clap::Args)]
pub struct Input {
    #[arg(long = "chainid", help = crate::tokens::CHAIN_ID_HELP)]
    #[schemars(description = crate::tokens::CHAIN_ID_HELP)]
    pub chain_id: String,
    /// Owner wallet whose ERC-20 balances are read.
    #[arg(long)]
    pub owner: String,
    /// Additional ERC-20 address; repeatable.
    #[arg(long = "token")]
    #[serde(default)]
    pub tokens: Vec<String>,
    /// USD decimal threshold with up to 18 fractional places; eligible balances and unpriced tokens are quoted in full.
    #[arg(long)]
    pub max_usd: Option<String>,
    /// Quote output symbol or address; defaults to USDC.
    #[arg(long)]
    pub quote_token: Option<String>,
    /// Incoming Transfer event lookback in blocks.
    #[arg(long)]
    pub lookback_blocks: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Row {
    pub address: String,
    pub symbol: String,
    pub decimals: Option<u8>,
    pub balance_raw: Option<String>,
    pub price_usd: Option<String>,
    pub value_usd: Option<String>,
    pub sources: Vec<String>,
    pub status: String,
    pub quote_out_raw: Option<String>,
    pub dust: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Output {
    pub chain_id: u64,
    pub owner: String,
    pub from_block: u64,
    pub to_block: u64,
    pub indexer: Option<String>,
    pub warning: String,
    pub tokens: Vec<Row>,
}

pub async fn portfolio(client: &Client, input: Input) -> Result<Output> {
    let config = discovery::config(&input.chain_id)?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    read(client, input, &provider, "https://coins.llama.fi/prices/current").await
}

pub(crate) async fn read(client: &Client, input: Input, provider: &DynProvider, price_url: &str) -> Result<Output> {
    let config = discovery::config(&input.chain_id)?;
    let owner = order_types::parse_address(&input.owner)?;
    let max = input.max_usd.as_deref().map(|s| amount::fixed(s, false)).transpose()?;
    let (candidates, from_block, to_block, indexer) = discovery::discover(provider, config, owner, &input.tokens, input.lookback_blocks).await?;
    let mut rows = balances(provider, config.id, owner, candidates).await?;
    let addresses = rows.iter().filter_map(|r| r.address.parse().ok()).collect::<Vec<_>>();
    let prices = prices::prices(config.id, &addresses, price_url).await;
    let client = client.clone().with_x402(crate::x402::Config::disabled(), None);
    for row in &mut rows {
        let Some(raw) = row.balance_raw.as_deref() else { continue; };
        let Some(decimals) = row.decimals else { continue; };
        let price = prices.get(&row.address.parse::<Address>()?).copied();
        let (value, below) = price.map(|p| amount::valuation(order_types::parse_u256(raw).unwrap(), p, decimals, max))
            .map_or((None, false), |(v, b)| (Some(v), b));
        row.price_usd = price.map(|p| amount::render(&p.to_string(), 18));
        row.value_usd = value;
        row.status = if price.is_some() { "priced" } else { "unpriced" }.into();
        if max.is_some() && (below || price.is_none()) {
            row.quote_out_raw = quote_row(&client, &input, row, provider, config.id).await.ok();
            row.dust = below && row.quote_out_raw.is_some();
            if row.quote_out_raw.is_none() { row.status = "no_route".into(); }
        }
    }
    Ok(Output { chain_id: config.id, owner: format!("{owner:?}"), from_block, to_block, indexer,
        warning: "Tokens received before the scanned block range may be missing; include their addresses explicitly. USD values are indicative; quotes are not sale floors.".into(), tokens: rows })
}

async fn balances(provider: &DynProvider, chain: u64, owner: Address, candidates: discovery::Candidates) -> Result<Vec<Row>> {
    let mut out = Vec::new();
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    for chunk in candidates.chunks(8) {
        let mut jobs = tokio::task::JoinSet::new();
        for (address, sources) in chunk {
            let (provider, address, sources) = (provider.clone(), *address, sources.clone());
            jobs.spawn(async move { read_row(&provider, chain, owner, address, sources.into_iter().collect()).await });
        }
        while let Some(result) = jobs.join_next().await { if let Some(row) = result? { out.push(row); } }
    }
    out.sort_by(|a, b| a.address.cmp(&b.address));
    Ok(out)
}

async fn read_row(provider: &DynProvider, chain: u64, owner: Address, address: Address, sources: Vec<String>) -> Option<Row> {
    let balance = discovery::balance(provider, address, owner).await;
    if matches!(balance, Ok(b) if b == U256::ZERO) { return None; }
    let metadata = token::read_metadata(provider, address, chain).await;
    let status = if balance.is_err() { "balance_error" } else if metadata.is_err() { "metadata_error" } else { "unpriced" };
    Some(Row { address: format!("{address:?}"), symbol: metadata.as_ref().map(|m| m.symbol.clone()).unwrap_or_default(),
        decimals: metadata.ok().map(|m| m.decimals), balance_raw: balance.ok().map(|b| b.to_string()),
        price_usd: None, value_usd: None, sources, status: status.into(), quote_out_raw: None, dust: false })
}

async fn quote_row(client: &Client, input: &Input, row: &Row, provider: &DynProvider, chain: u64) -> Result<String> {
    let target = input.quote_token.as_deref().unwrap_or("USDC");
    let to = match token::from_registry(target, chain) {
        Some(token) => token,
        None => token::read_metadata(provider, discovery::erc20(target)?, chain).await?,
    };
    let from = token::Token { address: row.address.clone(), symbol: row.symbol.clone(), decimals: row.decimals.unwrap() };
    let input = quote::QuoteInput { chain_id: chain.to_string(), from: from.address.clone(), to: to.address.clone(),
        amount: row.balance_raw.clone().unwrap(), slippage: None, verify: false };
    let (body, _) = quote::build_quote_body(&input, chain, &from, &to);
    let response = client.quote(&body).await?;
    let raw = response["output"].as_str().ok_or_else(|| eyre::eyre!("no route output"))?;
    if order_types::parse_u256(raw)? == U256::ZERO { return Err(eyre::eyre!("zero route output")); }
    Ok(raw.into())
}
