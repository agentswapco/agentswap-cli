// Fill-cost admission for intent sweeps, using the same app prices as the discounted value.
// Checked integer products compare USD values without rounding or floating point.
use super::{Context, Row};
use crate::{evm, service::portfolio::discovery};
use alloy::{primitives::{U256, U512}, providers::Provider};
use eyre::{Result, eyre};

// Single-fill receipts: BNB Smart Chain used 956,624 gas; Base used 549,012 gas.
const BNB_FILL_GAS: u64 = 956_624;
const BASE_FILL_GAS: u64 = 549_012;

pub(super) async fn skip(context: &Context<'_>, row: &mut Row, decay: U256) -> Result<bool> {
    let chain = discovery::config(&context.input.chain_id)?.id;
    let gas_price = context.provider.get_gas_price().await.ok();
    let native_price = evm::wrapped_native(chain).and_then(|address| context.prices.get(&address));
    if gas_price.is_none() { row.warnings.push("gas price unavailable; fill cost not checked".into()); }
    if native_price.is_none() { row.warnings.push("wrapped-native USD price unavailable; fill cost not checked".into()); }
    let (Some(gas_price), Some(native_price)) = (gas_price, native_price) else { return Ok(false); };
    let output = context.prices.get(&discovery::erc20(&context.receive.address)?)
        .ok_or_else(|| eyre!("receive price unavailable"))?;
    below(chain, decay, context.receive.decimals, output.value, gas_price, native_price.value)
}

pub(crate) fn below(chain: u64, decay: U256, decimals: u8, output_price: U256, gas_price: u128, native_price: U256) -> Result<bool> {
    let gas = if chain == 8453 { BASE_FILL_GAS } else { BNB_FILL_GAS.max(BASE_FILL_GAS) };
    let mut budget = U512::from(decay).checked_mul(U512::from(output_price))
        .ok_or_else(|| eyre!("discounted value overflow"))?;
    let mut cost = U512::from(native_price).checked_mul(U512::from(gas_price))
        .and_then(|value| value.checked_mul(U512::from(gas)))
        .ok_or_else(|| eyre!("fill cost overflow"))?;
    let scale = U512::from(10).checked_pow(U512::from(decimals.abs_diff(18)))
        .ok_or_else(|| eyre!("fill cost scale overflow"))?;
    if decimals <= 18 {
        budget = budget.checked_mul(scale).ok_or_else(|| eyre!("discounted value overflow"))?;
    } else {
        cost = cost.checked_mul(scale).ok_or_else(|| eyre!("fill cost overflow"))?;
    }
    Ok(budget < cost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_cost_boundaries_use_measured_gas_and_larger_fallback() {
        for chain in [56, 8453, 42161] { assert!(evm::wrapped_native(chain).is_some()); }
        assert!(evm::wrapped_native(4663).is_none());
        for (chain, gas) in [(56, 956_624), (8453, 549_012), (42161, 956_624), (4663, 956_624)] {
            for decimals in [6, 18, 24] {
                let scale = U256::from(10).pow(U256::from(decimals));
                let budget = U256::from(gas) * scale;
                let price = U256::from(1_000_000_000_000_000_000u64);
                let gas_price = 1_000_000_000_000_000_000u128;
                assert!(!below(chain, budget, decimals, price, gas_price, price).unwrap());
                assert!(below(chain, budget - U256::from(1), decimals, price, gas_price, price).unwrap());
                assert!(!below(chain, budget + U256::from(1), decimals, price, gas_price, price).unwrap());
            }
        }
        assert!(!below(56, U256::ZERO, 18, U256::MAX, 0, U256::MAX).unwrap());
        assert!(below(56, U256::MAX, 0, U256::MAX, 1, U256::MAX).is_err());
        assert!(below(56, U256::MAX, 255, U256::MAX, u128::MAX, U256::MAX).is_err());
    }
}
