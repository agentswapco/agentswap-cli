// Confirmed request validation and execution without caller overrides of sale authority.
// The live policy generation must match the server-confirmed grant.
use super::{models::*, request};
use crate::{client::Client, order_types, service::{sweep, submit::Wait, portfolio::{amount, discovery}}, signer::Signer};
use eyre::{Result, ensure, eyre};
use std::sync::Arc;

pub async fn load(input: &RunInput) -> Result<Record> {
    sweep::check_start_premium(input.start_premium_bps)?;
    let record = request::get(&input.request).await?;
    ensure!(record.status == "confirmed" && record.confirmed.is_some(), "grant request must be confirmed before run");
    Ok(record)
}

pub async fn run(client: &Client, signer: Arc<dyn Signer>, input: RunInput, record: Record, allow: bool, cap: Option<&str>, wait: Wait) -> Result<sweep::Output> {
    let sale = execution(&input, &record, signer.address())?;
    let (chain, proxy, receive) = (sale.chain_id.clone(), sale.proxy.clone(), sale.receive.clone());
    let mut output = sweep::sweep(client, signer, sale, allow, cap, wait).await?;
    super::received::report(&chain, &proxy, &receive, &crate::service::intentscan::Origins::public(), &mut output).await?;
    Ok(output)
}

pub(super) fn execution(input: &RunInput, record: &Record, agent: alloy::primitives::Address) -> Result<sweep::Input> {
    ensure!(record.status == "confirmed", "grant request must be confirmed before run");
    let basket = basket(record)?;
    ensure!(discovery::erc20(&record.request.agent)? == agent, "request agent differs from signer");
    Ok(sweep::Input { chain_id: basket.chain_id.to_string(), proxy: basket.proxy, receive: basket.receive,
        max_usd: amount::render(&alloy::primitives::U256::MAX.to_string(), 18), max_loss_bps: basket.max_loss_bps,
        tokens: basket.tokens, dry_run: false, via: input.via, wait: input.wait, self_submit: input.via == Via::Market,
        start_premium_bps: input.start_premium_bps,
        confirmed_generation: Some(basket.generation), request_caps: basket.caps })
}

/// The owner-confirmed sale of a v1 batch-sell request: spend tokens in request order with their
/// decimal caps, the zero-cap receive token, and the confirmed proxy, generation and discount.
pub(super) struct Basket {
    pub chain_id: u64,
    pub proxy: String,
    pub generation: String,
    pub max_loss_bps: u16,
    pub receive: String,
    pub tokens: Vec<String>,
    pub caps: std::collections::BTreeMap<String, String>,
}

pub(super) fn basket(record: &Record) -> Result<Basket> {
    let confirmed = record.confirmed.as_ref().ok_or_else(|| eyre!("grant request is not confirmed"))?;
    let request = &record.request;
    ensure!(request.v == 1 && request.purpose == "batch-sell", "request is not a v1 batch-sell request");
    ensure!((1..=5000).contains(&request.max_loss_bps) && (1..=request.max_loss_bps).contains(&confirmed.max_loss_bps), "invalid confirmed maxLossBps");
    discovery::erc20(&request.agent)?;
    discovery::erc20(&confirmed.proxy)?;
    ensure!(order_types::parse_u256(&confirmed.generation)? != alloy::primitives::U256::ZERO, "invalid confirmed generation");
    ensure!((2..=100).contains(&request.tokens.len()), "request requires spend tokens and one receive token, at most 100 total");
    let mut receive = None;
    let mut tokens = Vec::new();
    let mut caps = std::collections::BTreeMap::new();
    for token in &request.tokens {
        let address = discovery::erc20(&token.address)?.to_string();
        ensure!(token.cap.len() <= 32 && caps.insert(address.clone(), token.cap.clone()).is_none(), "invalid or duplicate request token");
        ensure!(token.cap.bytes().all(|b| b.is_ascii_digit() || b == b'.') && token.cap.split('.').count() <= 2
            && !token.cap.starts_with('.') && !token.cap.ends_with('.') && !token.cap.is_empty(), "invalid decimal cap");
        if !token.cap.bytes().any(|b| matches!(b, b'1'..=b'9')) {
            ensure!(receive.replace(address).is_none(), "request has multiple receive tokens");
        } else { tokens.push(address); }
    }
    ensure!(!tokens.is_empty(), "request has no spend tokens");
    Ok(Basket { chain_id: request.chain_id, proxy: confirmed.proxy.clone(), generation: confirmed.generation.clone(),
        max_loss_bps: confirmed.max_loss_bps, receive: receive.ok_or_else(|| eyre!("request has no zero-cap receive token"))?, tokens, caps })
}

pub(crate) fn raw_cap(value: &str, decimals: u8) -> Result<alloy::primitives::U256> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    ensure!(!whole.is_empty() && whole.bytes().chain(fraction.bytes()).all(|b| b.is_ascii_digit())
        && fraction.len() <= usize::from(decimals), "cap does not fit token decimals");
    order_types::parse_raw_amount("request cap", &format!("{whole}{fraction}{}", "0".repeat(usize::from(decimals) - fraction.len())))
}
