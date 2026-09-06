// Intent raw-unit tests for order construction and early input validation.
// Exports: module-local tests.
// Deps: super intent service and alloy primitives.

use super::*;

fn input(from: &str, amount: &str) -> PlaceInput {
    PlaceInput {
        chain: "base".to_string(),
        proxy_owner: format!("{:?}", Address::ZERO),
        from: from.to_string(),
        to: "USDC".to_string(),
        amount: amount.to_string(),
        start_out: "2".to_string(),
        end_out: "1".to_string(),
        decay_secs: Some(1),
        duration_secs: Some(1),
        deadline_secs: None,
        relay: false,
        self_submit: false,
        dry_run: true,
        max_amount: None,
    }
}

#[tokio::test]
async fn intent_order_keeps_one_raw_unit_for_six_and_eighteen_decimal_tokens() {
    for token in ["USDC", "WETH"] {
        let order = build_order(&input(token, "1"), Address::ZERO, 8453)
            .await
            .expect("intent order");
        assert_eq!(order.amountIn, U256::from(1));
        assert_eq!(order.startAmountOut, U256::from(2));
        assert_eq!(order.endAmountOut, U256::from(1));
    }
}

#[test]
fn intent_amount_validation_rejects_invalid_forms_before_rpc() {
    for value in ["", "1.5", "1e6", "raw:1", "-1", "0x1"] {
        let error = validate_input_amounts(&input("USDC", value))
            .expect_err("invalid amount");
        assert!(format!("{error}").contains("intent amount"));
    }
}
