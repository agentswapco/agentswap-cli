// Owner-facing batch-sale report: confirmed request, one batched grant read, independent prices,
// and the intent index's history and fill records. Needs no key and scans no logs.
mod budget;
mod models;
mod render;
mod rows;
#[cfg(test)]
mod tests;

pub use models::{ReportInput, ReportOutput};
pub use render::text;
use super::{models::Record, request, run};
use crate::{evm, order_types::UserProxyV6, service::{intentscan::{self, Fill, Intent, Origins}, portfolio::{discovery, prices}, token}};
use alloy::{primitives::{Address, U256}, providers::DynProvider};
use eyre::{Result, ensure};
use models::{GrantState, ReportToken};

/// An open intent this long past its deadline counts as expired; a fill lands before the deadline.
const EXPIRY_GRACE_MS: u64 = 60_000;
/// History reaches this far before the confirmation time, for clock differences.
const WINDOW_SLACK_MS: u64 = 600_000;

pub async fn report(input: &ReportInput) -> Result<ReportOutput> {
    let record = request::get(&input.request).await?;
    let config = evm::chain_config(&record.request.chain_id.to_string())?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    build(record, &provider, crate::routes::app_origin(), &Origins::public()).await
}

pub(super) async fn build(record: Record, provider: &DynProvider, app: &str, index: &Origins<'_>) -> Result<ReportOutput> {
    ensure!(matches!(record.status.as_str(), "confirmed" | "expired") && record.confirmed.is_some(), "grant request is not confirmed; nothing was sold under it");
    let basket = run::basket(&record)?;
    let (agent, proxy, receive) = (discovery::erc20(&record.request.agent)?, discovery::erc20(&basket.proxy)?, discovery::erc20(&basket.receive)?);
    let owner = match &record.request.owner {
        Some(owner) => discovery::erc20(owner)?,
        None => UserProxyV6::new(proxy, provider.clone()).owner().call().await?,
    };
    let spend = basket.tokens.iter().map(|t| discovery::erc20(t)).collect::<Result<Vec<_>>>()?;
    let all: Vec<Address> = spend.iter().copied().chain([receive]).collect();
    let budget = budget::read(provider, proxy, agent, owner, &all).await?;
    let generation_ok = budget.generation.to_string() == basket.generation;
    let price_map = prices::prices(basket.chain_id, &all, app).await;
    let mut warnings = Vec::new();
    if !generation_ok { warnings.push("the grant changed after confirmation; sold amounts come from intent records only".into()); }
    let generation: u64 = basket.generation.parse()?;
    let since = window_start(&record).saturating_sub(WINDOW_SLACK_MS);
    let intents = matched(index, (owner, agent, generation), basket.chain_id, since, budget.now * 1000, (receive, &spend), &mut warnings).await;
    let receive_state = budget.tokens.last().ok_or_else(|| eyre::eyre!("receive token missing from the budget read"))?;
    let receive_decimals = decimals(receive, basket.chain_id, receive_state.decimals)?;
    let prices = rows::Prices { map: &price_map, receive, receive_decimals };
    let mut tally = rows::Tally::default();
    let mut tokens = Vec::new();
    for (index, address) in spend.iter().enumerate() {
        let state = &budget.tokens[index];
        let token = spend_token(*address, basket.chain_id, state, basket.caps.get(&address.to_string()), generation_ok)?;
        let mine: Vec<_> = intents.iter().filter(|(i, _)| i.order.tokenIn == *address).collect();
        tokens.push(rows::row(&token, &mine, &prices, &mut tally, &mut warnings));
    }
    let grant = GrantState { expiry: budget.expiry, expires_at: rows::time(budget.expiry.saturating_mul(1000)),
        active: generation_ok && budget.expiry > budget.now, generation_matches: generation_ok };
    let next = next(&record.id, &grant, &tally);
    Ok(ReportOutput { request: record.id, chain_id: basket.chain_id, owner: format!("{owner:?}"), agent: format!("{agent:?}"),
        receive: ReportToken { address: receive.to_string(), symbol: token::label(receive, basket.chain_id, receive_state.symbol.as_deref()), decimals: receive_decimals },
        max_loss_bps: basket.max_loss_bps, grant, as_of: rows::time(budget.now * 1000), summary: rows::summary(tally, spend.len(), &prices),
        tokens, next, warnings })
}

fn window_start(record: &Record) -> u64 {
    let stamp = record.confirmed.as_ref().and_then(|c| c.confirmed_at.as_deref()).or(record.created_at.as_deref());
    stamp.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).and_then(|t| u64::try_from(t.timestamp_millis()).ok()).unwrap_or(0)
}

fn decimals(address: Address, chain: u64, read: Option<u8>) -> Result<u8> {
    read.or_else(|| token::from_registry(&address.to_string(), chain).map(|t| t.decimals))
        .ok_or_else(|| eyre::eyre!("token {address} does not answer decimals()"))
}

fn spend_token(address: Address, chain: u64, state: &budget::TokenState, cap: Option<&String>, generation_ok: bool) -> Result<rows::Token> {
    let decimals = decimals(address, chain, state.decimals)?;
    let cap = match (generation_ok && state.allowed, cap) {
        (true, _) => state.cap,
        (false, Some(cap)) => super::raw_cap(cap, decimals)?,
        (false, None) => U256::ZERO,
    };
    Ok(rows::Token { address, symbol: token::label(address, chain, state.symbol.as_deref()), decimals, cap,
        sold: if generation_ok { state.used } else { U256::ZERO }, balance: state.balance.unwrap_or_default() })
}

/// This sale's intents: placed by the agent under the confirmed generation, selling a request
/// token for the receive token; filled ones carry their fill record when the index has it.
async fn matched(index: &Origins<'_>, (owner, agent, generation): (Address, Address, u64), chain: u64, since: u64, now_ms: u64,
    (receive, spend): (Address, &[Address]), warnings: &mut Vec<String>) -> Vec<(Intent, Option<Fill>)> {
    let history = match intentscan::history(index, owner, chain, since).await {
        Ok(history) => history,
        Err(error) => {
            warnings.push(format!("intent records unavailable ({}); sold amounts come from the grant budget and proceeds are unknown", crate::redact::urls(&error.to_string())));
            return Vec::new();
        }
    };
    let (mut out, mut overdue) = (Vec::new(), 0);
    for mut intent in history.into_iter().filter(|i| i.agent == Some((agent, generation)) && i.order.tokenOut == receive && spend.contains(&i.order.tokenIn)) {
        if intent.status == "open" && now_ms > intent.deadline_ms.saturating_add(EXPIRY_GRACE_MS) { intent.status = "expired".into(); overdue += 1; }
        let fill = if intent.status == "filled" { intentscan::fill(index, intent.id).await.unwrap_or_else(|error| {
            warnings.push(format!("fill record for {:?} unavailable: {}", intent.id, crate::redact::urls(&error.to_string()))); None
        }) } else { None };
        out.push((intent, fill));
    }
    if overdue > 0 {
        warnings.push(format!("{overdue} intent{} the index still lists as open {} reported as expired: {} deadline passed with no fill recorded",
            if overdue == 1 { "" } else { "s" }, if overdue == 1 { "is" } else { "are" }, if overdue == 1 { "its" } else { "their" }));
    }
    out
}

fn next(id: &str, grant: &GrantState, tally: &rows::Tally) -> Vec<String> {
    let mut next = Vec::new();
    if tally.open > 0 {
        next.push(format!("{} intent{} open until {}; send the owner `batch-sell report --request {id}` again after that.",
            tally.open, if tally.open == 1 { " is" } else { "s are" }, rows::time(tally.open_until)));
    }
    if !tally.unsold {
        if tally.open == 0 { next.push("Nothing in this request is left to sell.".into()); }
    } else if grant.active {
        let lead = if tally.open > 0 { "After the open intents close, run" } else { "Run" };
        next.push(format!("{lead} `batch-sell run --request {id}` (MCP `batch_sell_run`) before the grant expires at {} to offer the unsold amount again.", grant.expires_at));
    } else if !grant.generation_matches {
        next.push("The owner changed the grant after confirming this request; selling the rest needs a new `batch-sell plan` and owner confirmation.".into());
    } else {
        next.push(format!("The grant expired at {}; selling the rest needs a new `batch-sell plan` and owner confirmation.", grant.expires_at));
    }
    next
}
