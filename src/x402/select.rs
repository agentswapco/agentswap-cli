// Deterministic x402 payment-requirement selection.
// Exports: select_accept.
// Deps: crate::x402 types/config.

use crate::x402::types::Accept;
use crate::x402::Config;
use eyre::{eyre, Result};

pub fn select_accept<'a>(accepts: &'a [Accept], config: &Config) -> Result<&'a Accept> {
    let cap = amount_u128(&config.max_amount)?;
    let expected_asset = expected_asset_address(&config.asset, config.chain_id)?;
    for accept in accepts {
        if accept.scheme != "exact" {
            continue;
        }
        if !chain_matches(accept.network.as_deref(), config.chain_id) {
            continue;
        }
        if !asset_matches(accept.asset.as_deref(), &expected_asset) {
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

/// Enforce the configured asset as an on-chain address allowlist. The accept's
/// asset must equal the expected token address for the chain (case-insensitive).
/// Fail closed: an accept without an asset is rejected.
fn asset_matches(asset: Option<&str>, expected_addr: &str) -> bool {
    match asset {
        Some(value) => value.eq_ignore_ascii_case(expected_addr),
        None => false,
    }
}

/// Resolve the configured asset (symbol or address) to the canonical on-chain
/// address for `chain_id`, reusing the built-in token registry. Symbols like
/// "USDC" resolve to that chain's USDC address; raw addresses pass through.
fn expected_asset_address(configured: &str, chain_id: u64) -> Result<String> {
    if configured.starts_with("0x") || configured.starts_with("0X") {
        return Ok(configured.to_string());
    }
    crate::tokens::resolve_token(configured, chain_id)
        .map(|(addr, _, _)| addr.to_string())
        .ok_or_else(|| {
            eyre!("unknown x402 asset '{configured}' for chain {chain_id}; cannot resolve to an address")
        })
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

    const USDC_BASE: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";

    #[test]
    fn selects_exact_payment_under_cap() {
        let body = format!(
            r#"{{"accepts":[
                {{"scheme":"other","network":"base","asset":"{USDC_BASE}","payTo":"0xpay","maxAmountRequired":"1"}},
                {{"scheme":"exact","network":"base","asset":"{USDC_BASE}","payTo":"0xpay","maxAmountRequired":"20"}}
            ]}}"#
        );
        let required = PaymentRequired::parse(&body).expect("parse payment required");
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
        let body = format!(
            r#"{{"accepts":[{{"scheme":"exact","network":"base","asset":"{USDC_BASE}","payTo":"0xpay","maxAmountRequired":"20"}}]}}"#
        );
        let required = PaymentRequired::parse(&body).expect("parse payment required");
        let config = Config {
            enabled: true,
            prefer_x402: false,
            chain_id: 8453,
            max_amount: "10".to_string(),
            asset: "USDC".to_string(),
        };
        assert!(select_accept(&required.accepts, &config).is_err());
    }

    #[test]
    fn rejects_wrong_asset_address() {
        // An accept advertising a non-USDC address must be rejected even under cap.
        let required = PaymentRequired::parse(
            r#"{"accepts":[{"scheme":"exact","network":"base","asset":"0xdeadBEEFdeadBEEFdeadBEEFdeadBEEFdeadBEEF","payTo":"0xpay","maxAmountRequired":"5"}]}"#,
        )
        .expect("parse payment required");
        let config = Config {
            enabled: true,
            prefer_x402: false,
            chain_id: 8453,
            max_amount: "25".to_string(),
            asset: "USDC".to_string(),
        };
        assert!(select_accept(&required.accepts, &config).is_err());
    }

    #[test]
    fn rejects_missing_asset() {
        // Fail closed: an accept without an asset field cannot be verified.
        let required = PaymentRequired::parse(
            r#"{"accepts":[{"scheme":"exact","network":"base","payTo":"0xpay","maxAmountRequired":"5"}]}"#,
        )
        .expect("parse payment required");
        let config = Config {
            enabled: true,
            prefer_x402: false,
            chain_id: 8453,
            max_amount: "25".to_string(),
            asset: "USDC".to_string(),
        };
        assert!(select_accept(&required.accepts, &config).is_err());
    }
}
