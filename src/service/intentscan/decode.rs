// Intent history rows decoded into protocol orders and agent authorizations.
// A row is kept only when its chain matches and its order hashes to its stated id.
use super::Intent;
use crate::order_types::{self, Order};
use alloy::{primitives::B256, sol_types::SolType};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct Item {
    chain_id: u64,
    order_hash: String,
    status: String,
    order: String,
    signature: String,
    pub(super) created_ms: u64,
    deadline_ms: u64,
}

pub(super) fn intent(item: Item, chain: u64) -> Option<Intent> {
    if item.chain_id != chain { return None; }
    let id: B256 = item.order_hash.parse().ok()?;
    let order = Order::abi_decode(&hex::decode(item.order.trim_start_matches("0x")).ok()?).ok()?;
    if order_types::order_id(&order) != id { return None; }
    let envelope = hex::decode(item.signature.trim_start_matches("0x")).ok()?;
    let agent = crate::service::intent::agent_authorization(&envelope).ok()?;
    Some(Intent { id, order, agent, status: item.status, deadline_ms: item.deadline_ms })
}
