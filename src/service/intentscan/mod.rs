// Read-only intent index client: an owner's intent history from the stream and fill records
// from the data API. Orders and authorizations are decoded and hash-checked locally.
// Deps: reqwest without redirects, crate::{order_types, routes, service::intent}.
mod decode;
#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;

use crate::order_types::Order;
use alloy::primitives::{Address, B256, U256};
use eyre::{Result, ensure};
use serde::Deserialize;

/// Most history rows one request returns, and most pages one read follows.
const PAGE_LIMIT: u64 = 500;
const MAX_PAGES: usize = 20;

#[derive(Debug, Clone, Copy)]
pub struct Origins<'a> {
    pub stream: &'a str,
    pub data: &'a str,
}

impl Origins<'static> {
    pub fn public() -> Self {
        let (stream, data) = crate::routes::intentscan_origins();
        Self { stream, data }
    }
}

/// One decoded history entry. `status` is the index's word: open, filled, cancelled or expired.
#[derive(Debug, Clone)]
pub struct Intent {
    pub id: B256,
    pub order: Order,
    /// Agent and policy generation from the authorization; None for an owner-signed intent.
    pub agent: Option<(Address, u64)>,
    pub status: String,
    pub deadline_ms: u64,
}

/// A settled fill: `received` is the owner's net output after the protocol fee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fill {
    pub received: U256,
    pub tx_hash: String,
    pub filled_ms: u64,
}

#[derive(Deserialize)]
struct Page {
    intents: Vec<decode::Item>,
    next_before_ms: Option<u64>,
}

#[derive(Deserialize)]
struct FillRecord {
    intent_hash: Option<String>,
    tx_hash: Option<String>,
    output_amount: Option<String>,
    filled_ms: Option<u64>,
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15)).build()?)
}

/// The owner's intents on `chain` created at or after `since_ms`, newest first. Entries that do
/// not decode as this protocol's orders, or whose order hash does not match, are left out.
pub async fn history(origins: &Origins<'_>, owner: Address, chain: u64, since_ms: u64) -> Result<Vec<Intent>> {
    let client = client()?;
    let mut out = Vec::new();
    let mut before: Option<u64> = None;
    for _ in 0..MAX_PAGES {
        let mut query = vec![("owner", format!("{owner:?}")), ("chain_ids", chain.to_string()), ("limit", PAGE_LIMIT.to_string())];
        if let Some(before) = before { query.push(("before_ms", before.to_string())); }
        let response = client.get(format!("{}/v1/intents", origins.stream)).query(&query).send().await?;
        ensure!(response.status().is_success(), "intent history HTTP {}", response.status());
        let page: Page = response.json().await?;
        let oldest = page.intents.last().map(|item| item.created_ms);
        out.extend(page.intents.into_iter().filter(|item| item.created_ms >= since_ms).filter_map(|item| decode::intent(item, chain)));
        match (page.next_before_ms, oldest) {
            (Some(next), Some(oldest)) if oldest >= since_ms => before = Some(next),
            _ => return Ok(out),
        }
    }
    Ok(out)
}

/// The fill record of intent `id`, or None while the index holds no fill for it.
pub async fn fill(origins: &Origins<'_>, id: B256) -> Result<Option<Fill>> {
    let response = client()?.get(format!("{}/v1/intent/{id:?}", origins.data)).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND { return Ok(None); }
    ensure!(response.status().is_success(), "fill record HTTP {}", response.status());
    let record: FillRecord = response.json().await?;
    let same = record.intent_hash.as_deref().is_some_and(|hash| hash.eq_ignore_ascii_case(&format!("{id:?}")));
    let (Some(amount), Some(tx_hash), Some(filled_ms), true) = (record.output_amount, record.tx_hash, record.filled_ms, same) else {
        return Ok(None);
    };
    Ok(Some(Fill { received: crate::order_types::parse_raw_amount("fill output", &amount)?, tx_hash, filled_ms }))
}
