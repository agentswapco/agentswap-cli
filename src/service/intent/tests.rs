// Intent relay request tests.
// Exports: no production symbols.
// Deps: parent intent service and V6 order types.

use super::*;

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
