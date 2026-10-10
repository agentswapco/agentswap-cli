// Intent-index and lens fixtures shared by service tests: orders, history rows, fill records and
// IntentLensV3 views. Rows carry real ABI encodings, so decoders run as against live services.
use crate::order_types::{self, IntentAuthorization, IntentLensV3, Order};
use alloy::primitives::{Address, B256, Bytes, U256};
use alloy::sol_types::SolType;
use serde_json::{Value, json};

pub(crate) fn order(owner: Address, token_in: Address, amount: u64, token_out: Address, floor: u64, nonce: u64) -> Order {
    Order { owner, recipient: owner, tokenIn: token_in, amountIn: U256::from(amount), tokenOut: token_out,
        startAmountOut: U256::from(floor * 2), endAmountOut: U256::from(floor), startTime: U256::from(1_000),
        decayEndTime: U256::from(1_600), endTime: U256::from(1_600), appData: B256::ZERO, nonce: U256::from(nonce) }
}

pub(crate) fn item(chain: u64, order: &Order, agent: Address, generation: u64, status: &str, created_ms: u64, deadline_ms: u64) -> Value {
    let id = order_types::order_id(order);
    let auth = IntentAuthorization { orderHash: id, agent, generation, nonce: U256::from(7), deadline: 1_600 };
    let envelope = order_types::authorization_envelope(&auth, &Bytes::from(vec![1u8; 65]));
    json!({"id": format!("{chain}:agentswap:{id:?}"), "chain_id": chain, "platform": "agentswap", "order_hash": format!("{id:?}"),
        "owner": format!("{:?}", order.owner), "status": status, "standard": "agentswap-v6",
        "order": format!("0x{}", hex::encode(<Order as SolType>::abi_encode(order))),
        "signature": format!("0x{}", hex::encode(envelope)), "created_ms": created_ms, "updated_ms": created_ms,
        "deadline_ms": deadline_ms, "summary": null})
}

pub(crate) fn page(items: Vec<Value>, next: Option<u64>) -> String {
    json!({"owner": "0x", "intents": items, "next_before_ms": next}).to_string()
}

pub(crate) fn fill(id: B256, received: &str, tx: B256, filled_ms: u64) -> String {
    json!({"intent_hash": format!("{id:?}"), "tx_hash": format!("{tx:?}"), "platform": "agentswap", "chain": "base",
        "filled_ms": filled_ms, "output_amount": received, "output_usd": 1.5, "surplus_bps": 0}).to_string()
}

pub(crate) fn view(filled: bool, cancelled: bool, in_window: bool) -> IntentLensV3::IntentView {
    IntentLensV3::IntentView { id: B256::ZERO, cancelled, filled, nonceSpent: false, killedByOwner: false, proxyDeployed: true,
        inWindow: in_window, exclusiveWindow: false, decayComplete: false, floorNow: U256::ZERO, feeNow: U256::ZERO,
        requiredNow: U256::ZERO, floorForOutsider: U256::ZERO, requiredForOutsider: U256::ZERO, ownerBalance: U256::ZERO,
        ownerProxyAllowance: U256::ZERO, proxy: Address::ZERO, observedAt: U256::ZERO, observedBlock: U256::ZERO }
}
