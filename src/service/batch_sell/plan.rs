// Exact portfolio selection and one-shot request construction.
// Caller filters are optional; no default holding-size threshold is imposed.
use super::{models::*, request};
use crate::{client::Client, evm, order_types, service::{portfolio::{self, amount, discovery, prices}, token, sweep}};
use alloy::{primitives::{Address, U256, U512}, providers::Provider};
use eyre::{Result, ensure, eyre};
use std::collections::BTreeSet;

pub async fn plan(client: &Client, input: PlanInput) -> Result<PlanOutput> {
    ensure!((1..=5000).contains(&input.max_loss_bps), "max-loss-bps must be 1..5000");
    let config = discovery::config(&input.chain_id)?;
    discovery::erc20(&input.owner)?; discovery::erc20(&input.agent)?;
    let filters = Filters::new(&input)?;
    let receive = token::resolve(&input.receive, config.id).await?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    let holdings = portfolio::portfolio(client, portfolio::Input { chain_id: input.chain_id.clone(), owner: input.owner.clone(),
        tokens: input.tokens.clone(), max_usd: None, quote_token: None }).await?;
    let native = evm::wrapped_native(config.id);
    let price_addresses: Vec<_> = native.into_iter().chain([discovery::erc20(&receive.address)?]).collect();
    let prices = prices::prices(config.id, &price_addresses, crate::routes::app_origin()).await;
    let gas = provider.get_gas_price().await.ok();
    let native_price = native.and_then(|a| prices.get(&a)).filter(|p| p.floor_eligible);
    let mut output = PlanOutput { requests: Vec::new(), count: 0, total_value_usd: "0".into(), max_loss_bps: input.max_loss_bps,
        left_out: Vec::new(), warnings: vec![holdings.warning] };
    if gas.is_none() || native_price.is_none() { output.warnings.push("Fill cost unavailable; economic eligibility could not be checked".into()); }
    let seen: BTreeSet<_> = holdings.tokens.iter().map(|r| discovery::erc20(&r.address)).collect::<Result<_>>()?;
    let mut tokens = Vec::new();
    let mut total = U512::ZERO;
    for row in holdings.tokens {
        if let Some(reason) = filters.reason(&row, &receive.address)? {
            output.left_out.push(LeftOut { token: row.address, reason: reason.into() }); continue;
        }
        let value = amount::fixed(row.value_usd.as_deref().ok_or_else(|| eyre!("unpriced"))?, false)?;
        let discounted = order_types::parse_u256(&(U512::from(value) * U512::from(10_000 - input.max_loss_bps) / U512::from(10_000)).to_string())?;
        if let (Some(gas), Some(price)) = (gas, native_price) {
            if sweep::gas_floor::below(config.id, discounted, 18, amount::fixed("1", false)?, gas, price.value)? {
                output.left_out.push(LeftOut { token: row.address, reason: "below_gas_floor".into() }); continue;
            }
        }
        let cap = amount::render(row.balance_raw.as_deref().ok_or_else(|| eyre!("balance unavailable"))?, usize::from(row.decimals.ok_or_else(|| eyre!("metadata unavailable"))?));
        ensure!(cap.len() <= 32, "rendered cap exceeds 32 characters for {}", row.address);
        tokens.push(GrantToken { address: row.address, cap }); total += U512::from(value);
    }
    for missing in filters.tokens.difference(&seen) { output.left_out.push(LeftOut { token: missing.to_string(), reason: "no_holding".into() }); }
    output.count = tokens.len(); output.total_value_usd = amount::render(&total.to_string(), 18);
    for body in requests(&input, config.id, &receive.address, &tokens)? { output.requests.push(request::post(&body).await?); }
    if output.requests.len() > 1 { output.warnings.push("Selection split across requests. Confirm and run each URL before confirming the next: grants replace the token basket.".into()); }
    Ok(output)
}

struct Filters { tokens: BTreeSet<Address>, exclude: BTreeSet<Address>, min: Option<U256>, max: Option<U256> }

impl Filters {
    fn new(input: &PlanInput) -> Result<Self> {
        let min = input.min_usd.as_deref().map(|s| amount::fixed(s, false)).transpose()?;
        let max = input.max_usd.as_deref().map(|s| amount::fixed(s, false)).transpose()?;
        ensure!(!matches!((min, max), (Some(a), Some(b)) if a > b), "min-usd exceeds max-usd");
        Ok(Self { tokens: input.tokens.iter().map(|s| discovery::erc20(s)).collect::<Result<_>>()?,
            exclude: input.exclude.iter().map(|s| discovery::erc20(s)).collect::<Result<_>>()?, min, max })
    }

    fn reason(&self, row: &portfolio::Row, receive: &str) -> Result<Option<&'static str>> {
        let address = discovery::erc20(&row.address)?;
        if address == discovery::erc20(receive)? { return Ok(Some("receive_token")); }
        if self.exclude.contains(&address) { return Ok(Some("excluded")); }
        if !self.tokens.is_empty() && !self.tokens.contains(&address) { return Ok(Some("not_selected")); }
        if row.balance_raw.is_none() { return Ok(Some("balance_error")); }
        if row.decimals.is_none() { return Ok(Some("metadata_error")); }
        if order_types::parse_u256(row.balance_raw.as_deref().unwrap_or("0"))? == U256::ZERO { return Ok(Some("zero")); }
        let Some(value) = row.value_usd.as_deref() else { return Ok(Some("unpriced")); };
        if !row.floor_eligible { return Ok(Some("price_not_independent")); }
        let value = amount::fixed(value, false)?;
        if self.min.is_some_and(|min| value < min) { return Ok(Some("under_min_usd")); }
        if self.max.is_some_and(|max| value > max) { return Ok(Some("over_max_usd")); }
        Ok(None)
    }
}

fn requests(input: &PlanInput, chain: u64, receive: &str, tokens: &[GrantToken]) -> Result<Vec<GrantRequest>> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let expiry = chrono::DateTime::from_timestamp(i64::try_from(now + 86400)?, 0).ok_or_else(|| eyre!("invalid expiry"))?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    Ok(tokens.chunks(19).map(|chunk| {
        let mut tokens = chunk.to_vec(); tokens.push(GrantToken { address: receive.into(), cap: "0".into() });
        GrantRequest { v: 1, chain_id: chain, agent: input.agent.clone(), owner: Some(input.owner.clone()), label: None, note: None,
            tokens, epoch: "1w".into(), expiry: expiry.clone(), actions: vec!["market".into(), "intent".into()],
            purpose: "batch-sell".into(), max_loss_bps: input.max_loss_bps, signature: None }
    }).collect())
}
