// MCP input and output envelopes for quote and discovery tools.
// Schema derives expose the shared service contracts.
use crate::{service::{quote, intent}, tokens::CHAIN_ID_HELP};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct BatchQuoteInput {
    #[schemars(description = CHAIN_ID_HELP)]
    pub(super) chain_id: String,
    pub(super) pairs: Vec<String>,
    /// Unsigned decimal amount in the input token's smallest unit.
    pub(super) amount: String,
    /// Owner's V6 proxy; required for BNB Smart Chain meta-aggregator quotes.
    pub(super) taker: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct BatchQuoteOutput {
    pub(super) results: Vec<quote::BatchQuoteResult>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct IntentListOutput {
    pub(super) intents: Vec<intent::IntentRecord>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ValueOutput {
    pub(super) value: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct TokensInput {
    #[schemars(description = CHAIN_ID_HELP)]
    pub(super) chain_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct PoolsInput {
    #[schemars(description = CHAIN_ID_HELP)]
    pub(super) chain_id: String,
    pub(super) address: String,
}

