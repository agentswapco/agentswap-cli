// Per-token report rows and the batch summary from grant budgets, intent records and prices.
// Integer math only: USD values are 18-decimal fixed point and discounts are basis points.
use super::models::{FillRow, ReportRow, Summary};
use crate::service::{intentscan::{Fill, Intent}, portfolio::{amount, prices::Price}, sweep::math};
use alloy::primitives::{Address, U256, U512};
use std::collections::BTreeMap;

/// One spend token: `sold` is the grant budget spent on it.
pub(super) struct Token {
    pub address: Address,
    pub symbol: String,
    pub decimals: u8,
    pub cap: U256,
    pub sold: U256,
    pub balance: U256,
}

pub(super) struct Prices<'a> {
    pub map: &'a BTreeMap<Address, Price>,
    pub receive: Address,
    pub receive_decimals: u8,
}

impl Prices<'_> {
    fn get(&self, token: Address) -> Option<&Price> { self.map.get(&token).filter(|price| price.floor_eligible) }
}

/// Running totals across rows; market and received are in receive-token units.
#[derive(Default)]
pub(super) struct Tally {
    market: U256,
    received: U256,
    received_all: U256,
    sold_usd: U256,
    /// Value of sold amounts with and without fill records, and the tokens holding each.
    known_usd: U256,
    unknown_usd: U256,
    known_tokens: usize,
    unknown_tokens: usize,
    unsold_usd: U256,
    worst: Option<(i128, String)>,
    sold_tokens: usize,
    pub open: usize,
    pub open_until: u64,
    pub unsold: bool,
}

pub(super) fn usd(raw: U256, price: &Price, decimals: u8) -> U256 {
    let value = U512::from(raw) * U512::from(price.value) / U512::from(10).pow(U512::from(decimals));
    if value > U512::from(U256::MAX) { U256::MAX } else { U256::from_limbs_slice(&value.as_limbs()[..4]) }
}

/// (market − received) / market in basis points, negative when proceeds beat the market.
pub(super) fn discount_bps(market: U256, received: U256) -> Option<i128> {
    if market.is_zero() { return None; }
    let (difference, negative) = if market >= received { (market - received, false) } else { (received - market, true) };
    let bps = u128::try_from(U512::from(difference) * U512::from(10_000u64) / U512::from(market)).ok()?;
    let bps = i128::try_from(bps).ok()?;
    Some(if negative { -bps } else { bps })
}

pub(super) fn pct(bps: i128) -> String {
    format!("{}{}.{:02}", if bps < 0 { "-" } else { "" }, bps.unsigned_abs() / 100, bps.unsigned_abs() % 100)
}

pub(super) fn time(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(i64::try_from(ms).unwrap_or(i64::MAX))
        .map_or_else(|| ms.to_string(), |t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn status(sold: U256, unsold: U256, intents: &[&(Intent, Option<Fill>)]) -> &'static str {
    if intents.iter().any(|(i, _)| i.status == "open") { return "open"; }
    if sold.is_zero() {
        if intents.is_empty() { return "not_placed"; }
        return if intents.iter().any(|(i, _)| i.status == "expired") { "expired" } else { "unsold" };
    }
    if unsold.is_zero() { "sold" } else { "partly_sold" }
}

fn fills(intents: &[&(Intent, Option<Fill>)]) -> Vec<FillRow> {
    let mut rows: Vec<_> = intents.iter().filter(|(i, _)| i.status == "filled").map(|(intent, fill)| FillRow {
        intent_id: format!("{:?}", intent.id), amount_in_raw: intent.order.amountIn.to_string(),
        received_raw: fill.as_ref().map(|f| f.received.to_string()), tx_hash: fill.as_ref().map(|f| f.tx_hash.clone()),
        filled_at: fill.as_ref().map(|f| time(f.filled_ms)) }).collect();
    rows.sort_by(|a, b| a.filled_at.cmp(&b.filled_at));
    rows
}

pub(super) fn row(token: &Token, intents: &[&(Intent, Option<Fill>)], prices: &Prices, tally: &mut Tally, warnings: &mut Vec<String>) -> ReportRow {
    let filled = intents.iter().filter(|(i, _)| i.status == "filled");
    let filled_in = filled.clone().fold(U256::ZERO, |sum, (i, _)| sum + i.order.amountIn);
    let (known_in, received) = filled.filter_map(|(i, f)| f.as_ref().map(|f| (i.order.amountIn, f.received)))
        .fold((U256::ZERO, U256::ZERO), |(a, r), (i, f)| (a + i, r + f));
    let sold = token.sold.max(filled_in);
    if sold > known_in {
        warnings.push(format!("{}: {} sold without a fill record; its proceeds are not counted", token.symbol, amount::render(&(sold - known_in).to_string(), token.decimals.into())));
    }
    let unsold = token.cap.saturating_sub(sold).min(token.balance);
    if unsold.is_zero() && token.cap > sold {
        warnings.push(format!("{}: {} of the cap was not sold; the owner holds none of this token, so none is left to sell", token.symbol,
            amount::render(&(token.cap - sold).to_string(), token.decimals.into())));
    }
    let open: Vec<u64> = intents.iter().filter(|(i, _)| i.status == "open").map(|(i, _)| i.deadline_ms).collect();
    let price = prices.get(token.address);
    let discount = discount(token, known_in, received, prices, tally);
    tally.received_all += received;
    tally.sold_tokens += usize::from(!sold.is_zero());
    tally.open += open.len();
    tally.open_until = tally.open_until.max(open.iter().copied().max().unwrap_or(0));
    tally.unsold |= !unsold.is_zero();
    let value = price.map(|p| usd(sold, p, token.decimals));
    let unsold_value = price.map(|p| usd(unsold, p, token.decimals));
    tally.sold_usd += value.unwrap_or_default();
    split(tally, price.map(|p| (usd(known_in, p, token.decimals), usd(sold - known_in, p, token.decimals))), known_in, sold);
    tally.unsold_usd += unsold_value.unwrap_or_default();
    if price.is_none() && !(sold.is_zero() && unsold.is_zero()) { warnings.push(format!("{}: no independent price; its USD values are not counted", token.symbol)); }
    let render = |raw: U256| amount::render(&raw.to_string(), token.decimals.into());
    let received_known = !known_in.is_zero();
    ReportRow { token: token.address.to_string(), symbol: token.symbol.clone(), status: status(sold, unsold, intents).into(),
        cap_raw: token.cap.to_string(), cap: render(token.cap), sold_raw: sold.to_string(), sold: render(sold),
        received_raw: received_known.then(|| received.to_string()),
        received: received_known.then(|| amount::render(&received.to_string(), prices.receive_decimals.into())),
        value_usd: value.map(|v| amount::render(&v.to_string(), 18)), discount_pct: discount.map(pct),
        unsold_raw: unsold.to_string(), unsold_value_usd: unsold_value.map(|v| amount::render(&v.to_string(), 18)),
        open_until: open.iter().copied().max().map(time), fills: fills(intents) }
}

/// Sold value with known proceeds versus sold value whose proceeds have no fill record.
fn split(tally: &mut Tally, values: Option<(U256, U256)>, known_in: U256, sold: U256) {
    let (known, unknown) = values.unwrap_or_default();
    tally.known_usd += known;
    tally.unknown_usd += unknown;
    tally.known_tokens += usize::from(!known_in.is_zero());
    tally.unknown_tokens += usize::from(sold > known_in);
}

fn discount(token: &Token, known_in: U256, received: U256, prices: &Prices, tally: &mut Tally) -> Option<i128> {
    let (input, output) = (prices.get(token.address)?, prices.get(prices.receive)?);
    if known_in.is_zero() { return None; }
    let market = math::floor(known_in, input, output, token.decimals, prices.receive_decimals, 0).ok()?;
    let bps = discount_bps(market, received)?;
    tally.market += market;
    tally.received += received;
    if tally.worst.as_ref().is_none_or(|(worst, _)| bps > *worst) { tally.worst = Some((bps, token.symbol.clone())); }
    Some(bps)
}

pub(super) fn summary(tally: Tally, total: usize, prices: &Prices) -> Summary {
    let received_usd = prices.get(prices.receive).map(|p| amount::render(&usd(tally.received_all, p, prices.receive_decimals).to_string(), 18));
    Summary { tokens_sold: tally.sold_tokens, tokens_total: total, received_raw: tally.received_all.to_string(),
        received: amount::render(&tally.received_all.to_string(), prices.receive_decimals.into()), received_usd,
        sold_value_usd: amount::render(&tally.sold_usd.to_string(), 18),
        known_proceeds_tokens: tally.known_tokens, known_proceeds_value_usd: amount::render(&tally.known_usd.to_string(), 18),
        unknown_proceeds_tokens: tally.unknown_tokens, unknown_proceeds_value_usd: amount::render(&tally.unknown_usd.to_string(), 18),
        average_discount_pct: discount_bps(tally.market, tally.received).map(pct),
        worst_discount_pct: tally.worst.as_ref().map(|(bps, _)| pct(*bps)), worst_discount_token: tally.worst.map(|(_, s)| s),
        unsold_value_usd: amount::render(&tally.unsold_usd.to_string(), 18), open_intents: tally.open }
}
