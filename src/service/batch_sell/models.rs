// Batch-sell caller inputs and stored request records.
// The zero-cap basket entry identifies the receive token using grant-link v1 semantics.
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
pub use crate::service::sweep::Via;

#[derive(Debug, Clone, Deserialize, JsonSchema, clap::Args)]
pub struct PlanInput {
    #[arg(long = "chainid", help = crate::tokens::CHAIN_ID_HELP)]
    pub chain_id: String,
    #[arg(long)]
    pub owner: String,
    #[arg(long)]
    pub agent: String,
    #[arg(long)]
    pub receive: String,
    /// Requested discount from independent market value, confirmed or lowered by the owner.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=5000))]
    #[schemars(range(min = 1, max = 5000))]
    pub max_loss_bps: u16,
    #[arg(long)]
    pub min_usd: Option<String>,
    #[arg(long)]
    pub max_usd: Option<String>,
    /// Select only these ERC-20 addresses; repeatable. Omission selects all discovered holdings.
    #[arg(long = "token")]
    #[serde(default)]
    pub tokens: Vec<String>,
    #[arg(long)]
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, clap::Args)]
pub struct RunInput {
    #[arg(long)]
    pub request: String,
    #[arg(long, value_enum, default_value = "intent")]
    #[serde(default)]
    pub via: Via,
    #[arg(long)]
    pub wait: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GrantToken {
    pub address: String,
    pub cap: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantRequest {
    pub v: u8,
    pub chain_id: u64,
    pub agent: String,
    pub owner: Option<String>,
    pub label: Option<String>,
    pub note: Option<String>,
    pub tokens: Vec<GrantToken>,
    #[serde(default)]
    pub epoch: String,
    #[serde(default)]
    pub expiry: String,
    #[serde(default)]
    pub actions: Vec<String>,
    pub purpose: String,
    pub max_loss_bps: u16,
    pub signature: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Confirmation {
    pub proxy: String,
    pub generation: String,
    pub max_loss_bps: u16,
}

#[derive(Debug, Deserialize)]
pub struct Record {
    pub id: String,
    pub status: String,
    pub request: GrantRequest,
    pub confirmed: Option<Confirmation>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShortLink {
    pub id: String,
    pub url: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LeftOut {
    pub token: String,
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PlanOutput {
    pub requests: Vec<ShortLink>,
    pub count: usize,
    pub total_value_usd: String,
    pub max_loss_bps: u16,
    pub left_out: Vec<LeftOut>,
    pub warnings: Vec<String>,
}
