// Deterministic x402 payment-requirement selection.
// Exports: select_accept.
// Deps: crate::x402 types/config.

use crate::x402::types::Accept;
use crate::x402::Config;
use eyre::{eyre, Result};

pub fn select_accept<'a>(accepts: &'a [Accept], config: &Config) -> Result<&'a Accept> {
    let cap = amount_u128(&config.max_amount)?;
    for accept in accepts {
        if accept.scheme != "exact" {
            continue;
        }
        if !chain_matches(accept.network.as_deref(), config.chain_id) {
            continue;
        }
        if !asset_matches(accept.asset.as_deref(), &config.asset) {
            continue;
        }
        let amount = accept
            .max_amount_required
            .as_deref()
            .ok_or_else(|| eyre!("x402 accept missing maxAmountRequired"))?;
        if amount_u128(amount)? <= cap {
            return Ok(accept);
        }
    }
    Err(eyre!(
        "no acceptable x402 payment option for chain {}, asset {}, cap {}; options: {:?}",
        config.chain_id,
        config.asset,
        config.max_amount,
        accepts
    ))
}

fn chain_matches(network: Option<&str>, chain_id: u64) -> bool {
    match network {
        Some("base") => chain_id == 8453,
        Some("ethereum") | Some("mainnet") => chain_id == 1,
        Some("arbitrum") => chain_id == 42161,
        Some(value) => value == chain_id.to_string(),
        None => true,
    }
}

fn asset_matches(asset: Option<&str>, expected: &str) -> bool {
    match asset {
        Some(value) => value.eq_ignore_ascii_case(expected) || value.starts_with("0x"),
        None => true,
    }
}

fn amount_u128(value: &str) -> Result<u128> {
    value
        .parse::<u128>()
        .map_err(|e| eyre!("invalid x402 amount '{value}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x402::types::PaymentRequired;

    #[test]
    fn selects_exact_payment_under_cap() {
        let required = PaymentRequired::parse(
            r#"{"accepts":[
                {"scheme":"other","network":"base","asset":"0xasset","payTo":"0xpay","maxAmountRequired":"1"},
                {"scheme":"exact","network":"base","asset":"0xasset","payTo":"0xpay","maxAmountRequired":"20"}
            ]}"#,
        )
        .expect("parse payment required");
        let config = Config {
            enabled: true,
            prefer_x402: false,
            chain_id: 8453,
            max_amount: "25".to_string(),
            asset: "USDC".to_string(),
        };
        let accept = select_accept(&required.accepts, &config).expect("select accept");
        assert_eq!(accept.scheme, "exact");
        assert_eq!(accept.max_amount_required.as_deref(), Some("20"));
    }

    #[test]
    fn rejects_payment_over_cap() {
        let required = PaymentRequired::parse(
            r#"{"accepts":[{"scheme":"exact","network":"base","asset":"USDC","payTo":"0xpay","maxAmountRequired":"20"}]}"#,
        )
        .expect("parse payment required");
        let config = Config {
            enabled: true,
            prefer_x402: false,
            chain_id: 8453,
            max_amount: "10".to_string(),
            asset: "USDC".to_string(),
        };
        assert!(select_accept(&required.accepts, &config).is_err());
    }
}
