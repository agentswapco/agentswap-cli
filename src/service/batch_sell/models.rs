// Batch-sell caller inputs and stored request records.
// The zero-cap basket entry identifies the receive token using grant-link v1 semantics.
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
pub use crate::service::sweep::Via;

#[derive(Debug, Clone, Deserialize, JsonSchema, clap::Args)]
pub struct PlanInput {
    #[arg(long = "chainid", help = crate::tokens::CHAIN_ID_HELP)]
    #[schemars(description = crate::tokens::CHAIN_ID_HELP)]
    pub chain_id: String,
    /// Owner wallet whose ERC-20 holdings will be selected.
    #[arg(long)]
    pub owner: String,
    /// Agent wallet that will execute the confirmed batch sale.
    #[arg(long)]
    pub agent: String,
    /// Short agent name the owner will recognise, shown on the review page; always pass one. At most
    /// 32 characters counted in UTF-16 code units; control and invisible characters are refused.
    #[arg(long)]
    pub name: Option<String>,
    /// Reason for the sale, shown to the owner on the review page. At most 140 characters counted in
    /// UTF-16 code units; control and invisible characters are refused.
    #[arg(long)]
    pub note: Option<String>,
    /// Receive ERC-20 symbol or address; requires a floor-eligible independent price.
    #[arg(long)]
    pub receive: String,
    /// Requested discount from independent market value, confirmed or lowered by the owner.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=5000))]
    #[schemars(range(min = 1, max = 5000))]
    pub max_loss_bps: u16,
    /// Minimum holding value in USD; unsigned decimal with up to 18 fractional places.
    #[arg(long)]
    pub min_usd: Option<String>,
    /// Maximum holding value in USD; unsigned decimal with up to 18 fractional places.
    #[arg(long)]
    pub max_usd: Option<String>,
    /// Select only these ERC-20 addresses; repeatable. Omission selects all discovered holdings.
    #[arg(long = "token")]
    #[serde(default)]
    pub tokens: Vec<String>,
    /// ERC-20 address to leave out; repeatable and takes precedence over selection.
    #[arg(long)]
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, clap::Args)]
pub struct RunInput {
    /// Confirmed grant request ID or app grant URL.
    #[arg(long)]
    pub request: String,
    /// Relayed intent or self-submitted market trade; market mode requires agent gas funds.
    #[arg(long, value_enum, default_value = "intent")]
    #[serde(default)]
    pub via: Via,
    /// Wait up to this many seconds for intent settlement; only available in intent mode.
    #[arg(long)]
    pub wait: Option<u64>,
    /// Intent mode: start each intent this many basis points above independent market value,
    /// 0 to 1000; its price then falls linearly to the owner-confirmed discount floor at expiry.
    /// 0 starts at market value. Market mode ignores it.
    #[arg(long, default_value_t = crate::service::sweep::DEFAULT_START_PREMIUM_BPS,
        value_parser = clap::value_parser!(u16).range(0..=1000))]
    #[serde(default = "crate::service::sweep::default_start_premium_bps")]
    #[schemars(range(min = 0, max = 1000))]
    pub start_premium_bps: u16,
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
    #[serde(default)]
    pub confirmed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Record {
    pub id: String,
    pub status: String,
    pub request: GrantRequest,
    pub confirmed: Option<Confirmation>,
    #[serde(default, rename = "createdAt")]
    pub created_at: Option<String>,
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
