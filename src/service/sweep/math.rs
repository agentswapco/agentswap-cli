// Budget, independent-price floor and Dutch-curve start arithmetic for sweeps.
// Checked wide products refuse overflow; division rounds down only once at the end.
use alloy::primitives::{U256, U512};
use eyre::{Result, eyre};
use crate::service::portfolio::{amount, prices::Price};

pub(super) fn remaining(cap: U256, used: U256, start: u64, epoch: u64, now: u64) -> U256 {
    if start == 0 || u128::from(now) >= u128::from(start) + u128::from(epoch) { cap }
    else { cap.saturating_sub(used) }
}

pub(super) fn spendable(balance: U256, budget: U256, allowance: U256) -> U256 {
    balance.min(budget).min(allowance)
}

pub(crate) fn floor(raw: U256, input: &Price, output: &Price, dec_in: u8, dec_out: u8, loss: u16) -> Result<U256> {
    eyre::ensure!(loss <= 10_000, "invalid floor parameters");
    scaled(raw, input, output, (dec_in, dec_out), 10_000 - u32::from(loss), "batch-sell floor")
}

/// Dutch-curve start: independent market value raised by `premium` bps, never below market value.
pub(super) fn start(raw: U256, input: &Price, output: &Price, dec_in: u8, dec_out: u8, premium: u16) -> Result<U256> {
    super::check_start_premium(premium)?;
    scaled(raw, input, output, (dec_in, dec_out), 10_000 + u32::from(premium), "batch-sell start")
}

fn scaled(raw: U256, input: &Price, output: &Price, (dec_in, dec_out): (u8, u8), factor_bps: u32, label: &str) -> Result<U256> {
    eyre::ensure!(output.value != U256::ZERO, "invalid floor parameters");
    eyre::ensure!(input.floor_eligible && output.floor_eligible, "price_not_independent");
    let ten = U512::from(10);
    let scale_in = ten.checked_pow(U512::from(dec_in)).ok_or_else(|| eyre!("floor scale overflow"))?;
    let scale_out = ten.checked_pow(U512::from(dec_out)).ok_or_else(|| eyre!("floor scale overflow"))?;
    let numerator = U512::from(raw).checked_mul(U512::from(input.value))
        .and_then(|n| n.checked_mul(scale_out)).and_then(|n| n.checked_mul(U512::from(factor_bps)))
        .ok_or_else(|| eyre!("floor numerator overflow"))?;
    let denominator = scale_in.checked_mul(U512::from(output.value)).and_then(|n| n.checked_mul(U512::from(10_000)))
        .ok_or_else(|| eyre!("floor denominator overflow"))?;
    crate::order_types::parse_raw_amount(label, &(numerator / denominator).to_string())
}

pub(super) fn price_skip(raw: U256, decimals: u8, input: Option<&Price>, output: Option<&Price>, max: U256) -> Option<&'static str> {
    if raw == U256::ZERO { return Some("zero"); }
    let (Some(input), Some(output)) = (input, output) else { return Some("unpriced"); };
    if !input.floor_eligible || !output.floor_eligible { return Some("price_not_independent"); }
    if !amount::valuation(raw, input.value, decimals, Some(max)).1 { return Some("over_max_usd"); }
    None
}

pub(super) fn quote_skip(output: Option<U256>, floor: U256) -> Option<&'static str> {
    match output {
        None | Some(U256::ZERO) => Some("no_route"),
        Some(output) if output < floor => Some("below_floor"),
        _ => None,
    }
}
