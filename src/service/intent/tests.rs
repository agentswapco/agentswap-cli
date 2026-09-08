// Intent tests: raw-unit order construction, early input validation, and the relay request.
// Exports: no production symbols.
// Deps: parent intent service, chain config, and V6 order types.

use super::*;

fn input(from: &str, amount: &str) -> PlaceInput {
    PlaceInput {
        chain_id: "base".to_string(),
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
    // `1_000` is the form the previous parser accepted silently: U256::from_str_radix skips
    // underscores, so a cap of `1_0` used to mean ten. Every listed form must fail here.
    for value in ["", " ", "1.5", "1e6", "raw:1", "-1", "0x1", "1_000"] {
        let error = validate_input_amounts(&input("USDC", value))
            .expect_err("invalid amount");
        assert!(format!("{error}").contains("intent amount"), "{value}: {error}");
    }
}

#[test]
fn relay_request_contains_chain_intent_generation() {
    let config = evm::chain_config("base").expect("base chain config");
    let order = Order {
        owner: Address::ZERO,
        recipient: Address::ZERO,
        tokenIn: Address::ZERO,
        amountIn: U256::from(1),
        tokenOut: Address::ZERO,
        startAmountOut: U256::from(2),
        endAmountOut: U256::from(1),
        startTime: U256::from(3),
        decayEndTime: U256::from(4),
        endTime: U256::from(5),
        appData: B256::ZERO,
        nonce: U256::from(6),
    };
    let body = announce_body(config, &order, &Bytes::new());
    let serialized = serde_json::to_string(&body).expect("serialize relay request");

    assert_eq!(body["chainId"], config.id);
    assert_eq!(body["generation"], config.generation);
    assert!(serialized.contains("\"generation\":\"v6\""));
    assert_eq!(body["announce"]["auth"], "0x");
}

#[test]
fn default_deadline_matches_order_end_time() {
    let order_end_time = 1_700_000_600u64;
    let now = 1_700_000_000u64;
    let deadline = resolve_deadline(order_end_time, None, now).expect("default deadline");
    assert_eq!(deadline, order_end_time);
}

#[test]
fn caller_supplied_deadline_earlier_than_end_time_is_refused() {
    let order_end_time = 1_700_000_600u64;
    let now = 1_700_000_000u64;
    let deadline_secs = 120u64;
    let err = resolve_deadline(order_end_time, Some(deadline_secs), now)
        .expect_err("should reject earlier deadline");
    let msg = err.to_string();
    assert!(msg.contains("1700000120"), "expected deadline 1700000120 in error: {msg}");
    assert!(msg.contains("1700000600"), "expected order endTime 1700000600 in error: {msg}");
}

#[test]
fn caller_supplied_deadline_after_end_time_is_accepted() {
    let order_end_time = 1_700_000_600u64;
    let now = 1_700_000_000u64;
    let deadline_secs = 700u64;
    let deadline = resolve_deadline(order_end_time, Some(deadline_secs), now)
        .expect("valid deadline");
    assert_eq!(deadline, 1_700_000_700);
}
