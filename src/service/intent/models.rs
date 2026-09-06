// Request and response models for the intent service, shared by the CLI and MCP.
// Exports: PlaceInput, PlaceOutcome, ListInput, StatusInput, PolicyInput, IntentRecord,
// PolicyOutput, TokenPolicy.
// Deps: serde, schemars, crate::order_types DTOs.

use crate::order_types;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlaceInput {
    pub chain: String,
    pub proxy_owner: String,
    pub from: String,
    pub to: String,
    /// Unsigned decimal input amount in the token's smallest unit.
    pub amount: String,
    /// Unsigned decimal starting output in the token's smallest unit.
    pub start_out: String,
    /// Unsigned decimal ending output in the token's smallest unit.
    pub end_out: String,
    #[serde(default)]
    pub decay_secs: Option<u64>,
    #[serde(default)]
    pub duration_secs: Option<u64>,
    #[serde(default)]
    pub deadline_secs: Option<u64>,
    #[serde(default)]
    pub relay: bool,
    #[serde(default)]
    pub self_submit: bool,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    /// Optional unsigned decimal cap in raw input units.
    pub max_amount: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlaceOutcome {
    pub dry_run: bool,
    pub chain_id: u64,
    pub order: order_types::OrderDto,
    pub id: String,
    pub authorization: order_types::IntentAuthorizationDto,
    pub envelope: String,
    pub digest: String,
    pub signature: String,
    pub relay: Option<serde_json::Value>,
    pub tx_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListInput {
    pub chain: String,
    pub owner: Option<String>,
    pub agent: Option<String>,
    #[serde(default)]
    pub lookback_blocks: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StatusInput {
    pub chain: String,
    pub id: String,
    #[serde(default)]
    pub lookback_blocks: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicyInput {
    pub chain: String,
    pub owner: String,
    pub agent: String,
    #[serde(default)]
    pub lookback_blocks: Option<u64>,
    #[serde(default)]
    pub tokens: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IntentRecord {
    pub id: String,
    pub placed_by: String,
    pub owner: String,
    pub agent: Option<String>,
    pub pair: String,
    pub amount_in: String,
    pub start_out: String,
    pub end_out: String,
    pub window: String,
    pub exclusive_window: bool,
    pub floor_now: String,
    pub fee_now: String,
    pub required_now: String,
    pub floor_for_outsider: String,
    pub required_for_outsider: String,
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicyOutput {
    pub owner: String,
    pub agent: String,
    pub proxy: String,
    pub expiry: String,
    pub epoch_len: String,
    pub action_mask: String,
    pub generation: String,
    pub tokens: Vec<TokenPolicy>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TokenPolicy {
    pub token: String,
    pub allowed: bool,
    pub cap: String,
    pub used: String,
    pub epoch_start: String,
}
