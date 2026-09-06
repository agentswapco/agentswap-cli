// V6 ABI, lens-capture, and fill-event decoding tests.
// Exports: unit tests for order hashes, preview layout, and IntentFilled.
// Deps: parent order types and alloy ABI traits.

use super::*;
use alloy::sol_types::{SolCall, SolEvent};

#[test]
fn v6_agent_order_has_generation_in_digest() {
    let order = UserProxyV6::AgentOrder {
        agent: parse_address("0x1000000000000000000000000000000000000001").expect("agent"),
        generation: 4,
        router: parse_address("0x2000000000000000000000000000000000000002").expect("router"),
        tokenIn: parse_address("0x3000000000000000000000000000000000000003").expect("token in"),
        amountIn: U256::from(100_000_000u64),
        tokenOut: parse_address("0x4000000000000000000000000000000000000004").expect("token out"),
        minOut: U256::from(50_000_000_000_000_000u64),
        nonce: U256::from(7u64),
        deadline: U256::from(4_000_000_000u64),
    };
    let proxy = parse_address("0x5000000000000000000000000000000000000005").expect("proxy");
    let digest = signing_hash(&order, &proxy_domain(8453, proxy));
    assert_ne!(digest, B256::ZERO);
}

#[test]
fn authorization_envelope_uses_agent_discriminator() {
    let auth = IntentAuthorization {
        orderHash: B256::ZERO,
        agent: Address::ZERO,
        generation: 1,
        nonce: U256::from(2),
        deadline: 3,
    };
    let encoded = authorization_envelope(&auth, &Bytes::from(vec![1, 2, 3]));
    assert_eq!(&encoded[..31], &[0u8; 31]);
    assert_eq!(encoded[31], 1);
}

const CAPTURE_A: &str = concat!(
    "cd013993fd9e6ba181b5e0775c1a6b7b86d5e6193b27419293b40975d9dab8dc",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000001",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000001",
    "00000000000000000000000000000000000000000000000000000000acda7d00",
    "00000000000000000000000000000000000000000000000000000000000d4670",
    "00000000000000000000000000000000000000000000000000000000ace7c370",
    "00000000000000000000000000000000000000000000000000000000acda7d00",
    "00000000000000000000000000000000000000000000000000000000ace7c370",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000006a9d7fe9",
    "0000000000000000000000000000000000000000000000000000000003099183",
);

const CAPTURE_B: &str = concat!(
    "82011beaccaa59aa75d0b665fbf7837461f5fa99f23f5d0b5334b66a13c07851",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000001",
    "0000000000000000000000000000000000000000000000000000000000000001",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "00000000000000000000000000000000000000000000000000000000b2d05e00",
    "00000000000000000000000000000000000000000000000000000000000dbba0",
    "00000000000000000000000000000000000000000000000000000000b2de19a0",
    "00000000000000000000000000000000000000000000000000000000b342cee0",
    "00000000000000000000000000000000000000000000000000000000b350934a",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000006a9d7feb",
    "0000000000000000000000000000000000000000000000000000000003099184",
);

fn decode_capture(raw: &str) -> IntentLensV3::IntentView {
    IntentLensV3::previewCall::abi_decode_returns(&hex::decode(raw).expect("capture hex"))
        .expect("nineteen-word lens capture")
}

#[test]
fn capture_a_decodes_the_v3_lens_layout() {
    let view = decode_capture(CAPTURE_A);
    let expected_id: B256 = "cd013993fd9e6ba181b5e0775c1a6b7b86d5e6193b27419293b40975d9dab8dc".parse().expect("id");
    assert_eq!(view.id, expected_id);
    assert!(view.inWindow);
    assert!(!view.exclusiveWindow);
    assert!(view.decayComplete);
    assert_eq!(view.floorNow, U256::from(2_900_000_000u64));
    assert_eq!(view.feeNow, U256::from(870_000u64));
    assert_eq!(view.requiredNow, U256::from(2_900_870_000u64));
    assert_eq!(view.floorForOutsider, U256::from(2_900_000_000u64));
    assert_eq!(view.requiredForOutsider, U256::from(2_900_870_000u64));
    assert_eq!(view.observedAt, U256::from(0x6a9d7fe9u64));
    assert_eq!(view.observedBlock, U256::from(0x3099183u64));
}

#[test]
fn capture_b_decodes_exclusive_window_pricing() {
    let view = decode_capture(CAPTURE_B);
    assert!(view.inWindow);
    assert!(view.exclusiveWindow);
    assert!(!view.decayComplete);
    assert_eq!(view.floorNow, U256::from(3_000_000_000u64));
    assert_eq!(view.feeNow, U256::from(900_000u64));
    assert_eq!(view.requiredNow, U256::from(3_000_900_000u64));
    assert_eq!(view.floorForOutsider, U256::from(3_007_500_000u64));
    assert_eq!(view.requiredForOutsider, U256::from(3_008_402_250u64));
    assert_eq!(view.observedAt, U256::from(0x6a9d7febu64));
    assert_eq!(view.observedBlock, U256::from(0x3099184u64));
}

#[test]
fn intent_filled_log_decodes_fee_between_required_and_received() {
    let id = B256::from([1u8; 32]);
    let owner = parse_address("0x1000000000000000000000000000000000000001").expect("owner");
    let solver = parse_address("0x2000000000000000000000000000000000000002").expect("solver");
    let recipient = parse_address("0x3000000000000000000000000000000000000003").expect("recipient");
    let caller = parse_address("0x4000000000000000000000000000000000000004").expect("caller");
    let expected = IntentSettlerV3::IntentFilled {
        id,
        owner,
        solver,
        recipient,
        caller,
        amountIn: U256::from(10),
        requiredOut: U256::from(20),
        fee: U256::from(3),
        receivedOut: U256::from(23),
        aboveFloor: U256::ZERO,
    };
    let event = IntentSettlerV3::IntentFilled::decode_raw_log(
        expected.encode_topics(),
        &expected.encode_data(),
    )
    .expect("fill log");
    assert_eq!(event.requiredOut, U256::from(20));
    assert_eq!(event.fee, U256::from(3));
    assert_eq!(event.receivedOut, U256::from(23));
}
