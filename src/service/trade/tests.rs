// Unit tests for V6 market-order validation and protection-floor rules.
// Exports: no production symbols.
// Deps: parent trade module and quote models.

use super::*;
use crate::service::quote::QuoteContext;

fn quote_output() -> QuoteOutput {
    QuoteOutput {
        request: QuoteContext {
            chain_id: 8453,
            token_in: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
            token_in_symbol: "USDC".to_string(),
            token_in_decimals: 6,
            token_out: "0x4200000000000000000000000000000000000006".to_string(),
            token_out_symbol: "WETH".to_string(),
            token_out_decimals: 18,
            amount_in: "1000000".to_string(),
        },
        response: serde_json::json!({
            "router": "0x1111111111111111111111111111111111111111",
            "output": "500000000000000000",
        }),
    }
}

fn trade_input(min_out: Option<String>, dry_run: bool) -> TradeInput {
    TradeInput {
        chain_id: "base".to_string(), from: "USDC".to_string(), to: "WETH".to_string(),
        amount: "1".to_string(), slippage: Some(50), min_out, max_amount: None,
        mode: "agent-order".to_string(),
        proxy: "0x2222222222222222222222222222222222222222".to_string(), nonce: Some("1".to_string()),
        deadline_secs: Some(120), dry_run, self_submit: false,
    }
}

#[test]
fn refuses_fund_moving_trade_without_min_out() {
    let err = build_order(&trade_input(None, false), Address::ZERO, 1, &quote_output())
        .expect_err("must refuse missing min_out on fund-moving trade");
    assert!(format!("{err}").contains("explicit --min-out floor"), "{err}");
}

#[test]
fn allows_dry_run_without_min_out() {
    build_order(&trade_input(None, true), Address::ZERO, 1, &quote_output())
        .expect("dry-run may derive min_out");
}

#[test]
fn rejects_slippage_over_10000_bps() {
    let response = serde_json::json!({ "output": "1000000" });
    assert!(slippage_min_out(&response, 10_001).is_err());
    assert!(slippage_min_out(&response, 10_000).is_ok());
}

#[test]
fn enforces_notional_cap() {
    let order = build_order(&trade_input(Some("1".to_string()), false), Address::ZERO, 1, &quote_output())
        .expect("build order");
    assert!(enforce_notional_cap(&order, Some("500000")).is_err());
    assert!(enforce_notional_cap(&order, Some("1000000")).is_ok());
    assert!(enforce_notional_cap(&order, None).is_ok());
}

#[test]
fn trade_amount_validation_rejects_before_quote_or_signing() {
    for value in ["", " ", "1.5", "1e6", "raw:1000000", "-1", "0x10", "1_000"] {
        let mut input = trade_input(Some("1".to_string()), true);
        input.amount = value.to_string();
        let error = validate_input_amounts(&input).expect_err("invalid amount");
        assert!(format!("{error}").contains("trade amount"));
    }
}
