// Sweep arithmetic, validation and outcome regression tests.
// Fixtures use generated addresses and exact raw integers, never monetary floats.
use super::*;
use alloy::primitives::U256;
use crate::service::{submit::TxStatus, trade::{TradeOutcome, SelfSubmitPreview}};
use serde_json::json;

pub(super) fn price(value: &str, eligible: bool) -> prices::Price {
    prices::Price { value: amount::fixed(value, false).unwrap(), source: "oracle".into(), confidence: None,
        basis: None, observed: Some(true), source_count: Some(2), floor_eligible: eligible }
}

pub(super) fn policy() -> intent::PolicyOutput {
    intent::PolicyOutput { owner: Address::repeat_byte(4).to_string(), agent: Address::repeat_byte(5).to_string(), proxy: Address::repeat_byte(6).to_string(),
        expiry: "200".into(), epoch_len: "60".into(), action_mask: "1".into(), generation: "1".into(), note: None,
        tokens: vec![intent::TokenPolicy { token: Address::repeat_byte(3).to_string(), allowed: true, cap: "0".into(), used: "0".into(), epoch_start: "0".into() }] }
}

#[test]
fn budget_windows_and_three_way_minimum() {
    let cap = U256::from(100);
    for (start, now, expected) in [(0, 100, 100), (100, 159, 70), (100, 160, 100), (100, 200, 100)] {
        assert_eq!(math::remaining(cap, U256::from(30), start, 60, now), U256::from(expected));
    }
    assert_eq!(math::remaining(cap, U256::from(101), 100, 60, 120), U256::ZERO);
    assert_eq!(math::remaining(cap, U256::from(30), u64::MAX, 60, u64::MAX), U256::from(70));
    for (a, b, c) in [(2, 3, 4), (4, 2, 3), (3, 4, 2)] {
        assert_eq!(math::spendable(U256::from(a), U256::from(b), U256::from(c)), U256::from(2));
    }
}

#[test]
fn independent_floor_six_eighteen_decimals_and_rounding() {
    let one = price("1", true);
    let three = price("3", true);
    assert_eq!(math::floor(U256::from(1_000_000), &one, &one, 6, 18, 100).unwrap().to_string(), "990000000000000000");
    assert_eq!(math::floor(U256::from(1_000_000_000_000_000_000u64), &one, &one, 18, 6, 100).unwrap(), U256::from(990000));
    assert_eq!(math::floor(U256::from(10), &one, &three, 0, 0, 100).unwrap(), U256::from(3));
    assert_eq!(math::floor(U256::from(1), &one, &three, 6, 18, 0).unwrap().to_string(), "333333333333");
    assert!(math::floor(U256::MAX, &one, &one, 0, 255, 0).is_err());
    assert!(math::floor(U256::from(1), &one, &price("0", true), 6, 18, 0).is_err());
    assert!(math::floor(U256::from(1), &one, &one, 6, 18, 10001).is_err());
    assert!(math::floor(U256::from(1), &price("1", false), &one, 6, 18, 0).is_err());
}

#[test]
fn every_price_and_route_skip_reason() {
    let one = price("1", true);
    let bad = price("1", false);
    let raw = U256::from(1_000_000);
    let max = amount::fixed("1", false).unwrap();
    for (raw, input, output, max, reason) in [
        (U256::ZERO, None, None, max, Some("zero")),
        (raw, None, Some(&one), max, Some("unpriced")),
        (raw, Some(&one), None, max, Some("unpriced")),
        (raw, Some(&bad), Some(&one), max, Some("price_not_independent")),
        (raw, Some(&one), Some(&bad), max, Some("price_not_independent")),
        (raw, Some(&one), Some(&one), max - U256::from(1), Some("over_max_usd")),
        (raw, Some(&one), Some(&one), max, None),
    ] { assert_eq!(math::price_skip(raw, 6, input, output, max), reason); }
    assert_eq!(math::quote_skip(None, raw), Some("no_route"));
    assert_eq!(math::quote_skip(Some(U256::ZERO), raw), Some("no_route"));
    assert_eq!(math::quote_skip(Some(raw - U256::from(1)), raw), Some("below_floor"));
    assert_eq!(math::quote_skip(Some(raw), raw), None);
}

#[test]
fn upfront_policy_refusals_and_receive_only_membership() {
    let mut p = policy();
    let receive = Address::repeat_byte(3);
    assert!(validate_policy(&p, receive, 100, Via::Market).is_ok());
    assert!(validate_policy(&p, receive, 200, Via::Market).is_err());
    p.action_mask = "4".into();
    assert!(validate_policy(&p, receive, 100, Via::Market).is_err());
    p.action_mask = "1".into(); p.tokens[0].allowed = false;
    assert!(validate_policy(&p, receive, 100, Via::Market).is_err());
}

fn trade(status: Option<TxStatus>, dry_run: bool) -> TradeOutcome {
    let address = Address::repeat_byte(1).to_string();
    serde_json::from_value(json!({"dry_run":dry_run,"mode":"agent-order","quote":{},"digest":"test",
        "order":{"agent":address,"generation":"1","router":address,"tokenIn":address,
        "amountIn":"1","tokenOut":address,"minOut":"1","nonce":"1","deadline":"1"},
        "self_submit": status.map(|status| SelfSubmitPreview { to: address.clone(), function: "executeAsAgent".into(), calldata: "0x".into(),
            spender: address.clone(), router_data: "0x".into(), tx_hash: Some("test-hash".into()), tx_status: Some(status), tx_error: None, tx_explorer_url: None })})).unwrap()
}

#[test]
fn stop_on_unknown_and_revert_continue_on_refusal_and_confirmed() {
    let mut row = Row::new(Address::repeat_byte(1).to_string());
    assert!(!row.record(Err(eyre!("refused"))));
    assert_eq!(row.outcome, "failed");
    for (status, stop) in [(TxStatus::Confirmed, false), (TxStatus::Reverted, true), (TxStatus::Unknown, true)] {
        let mut row = Row::new(String::new());
        assert_eq!(row.record(Ok(trade(Some(status), false))), stop);
        assert_eq!(row.tx_hash.as_deref(), Some("test-hash"));
        let output = Output { owner: String::new(), dry_run: false, note: None, tokens: vec![row] };
        let result = output.check();
        if stop { assert_eq!(crate::exit_code(&result.unwrap_err()), if status == TxStatus::Unknown { 4 } else { 3 }); }
        else { assert!(result.is_ok()); }
    }
    let mut row = Row::new(String::new());
    assert!(!row.record(Ok(trade(None, true)))); assert_eq!(row.outcome, "skipped"); assert_eq!(row.reason.as_deref(), Some("dry_run"));
    assert!(!row.record(Ok(trade(None, false)))); assert_eq!(row.outcome, "skipped"); assert_eq!(row.reason.as_deref(), Some("not_submitted"));
    row.record(Err(eyre!("refused")));
    assert_eq!(crate::exit_code(&Output { owner: String::new(), dry_run: false, note: None, tokens: vec![row] }.check().unwrap_err()), 1);
}

#[test]
fn cli_required_thresholds_and_mcp_default_dry_run() {
    use clap::Parser;
    let args = ["agentswap", "batch-sell", "plan", "--chainid", "8453", "--owner", "owner", "--agent", "agent", "--receive", "USDC", "--max-loss-bps", "100"];
    assert!(crate::cli::Cli::try_parse_from(args).is_ok());
    assert!(crate::cli::Cli::try_parse_from(&args[..11]).is_err());
    let input: Input = serde_json::from_value(json!({"chain_id":"8453","proxy":"proxy","receive":"USDC","max_usd":"5","max_loss_bps":100,"tokens":["token"]})).unwrap();
    assert!(input.dry_run && default_true());
}

#[test]
fn batch_sell_cli_refuses_loss_above_contract_limit() {
    use clap::Parser;
    let args = ["agentswap", "batch-sell", "plan", "--chainid", "8453", "--owner", "owner", "--agent", "agent", "--receive", "USDC", "--max-loss-bps"];
    assert!(crate::cli::Cli::try_parse_from(args.into_iter().chain(["5000"])).is_ok());
    assert!(crate::cli::Cli::try_parse_from(args.into_iter().chain(["5001"])).is_err());
}

#[test]
fn sweep_failure_text_is_wrapped_once() {
    let mut outcome = trade(Some(TxStatus::Unknown), false);
    outcome.self_submit.as_mut().unwrap().tx_error = Some("receipt timed out".into());
    let mut row = Row::new(String::new());
    row.record(Ok(outcome));
    assert_eq!(row.error.as_deref(), Some("receipt timed out"));
    let output = Output { owner: String::new(), dry_run: false, note: None, tokens: vec![row] };
    let error = output.check().unwrap_err().to_string();
    assert_eq!(error.matches("transaction test-hash").count(), 1, "{error}");
    assert_eq!(error.matches("receipt timed out").count(), 1, "{error}");
}
