// Batch-sale report input and owner-facing results shared by the CLI and MCP.
// Raw amounts are decimal strings in smallest units; display amounts and USD values sit beside them.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, JsonSchema, clap::Args)]
pub struct ReportInput {
    /// Confirmed grant request ID or app grant URL.
    #[arg(long)]
    pub request: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReportOutput {
    pub request: String,
    pub chain_id: u64,
    pub owner: String,
    pub agent: String,
    pub receive: ReportToken,
    /// Owner-confirmed discount floor in basis points.
    pub max_loss_bps: u16,
    pub grant: GrantState,
    /// Chain time of the budget read. USD values and discounts use independent prices read with
    /// this report, not prices at fill time.
    pub as_of: String,
    pub tokens: Vec<ReportRow>,
    pub summary: Summary,
    /// What the owner or agent can do next.
    pub next: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReportToken {
    pub address: String,
    pub symbol: String,
    pub decimals: u8,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GrantState {
    /// Policy expiry, unix seconds, and as RFC 3339.
    pub expiry: u64,
    pub expires_at: String,
    /// The policy is unexpired and still the confirmed generation, so `batch-sell run` can sell more.
    pub active: bool,
    pub generation_matches: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReportRow {
    pub token: String,
    pub symbol: String,
    /// sold, partly_sold, open, expired, not_placed or unsold.
    pub status: String,
    pub cap_raw: String,
    pub cap: String,
    /// Spent from the grant budget for this token.
    pub sold_raw: String,
    pub sold: String,
    /// Net receive-token proceeds from fill records; null when no fill record is known.
    pub received_raw: Option<String>,
    pub received: Option<String>,
    /// Sold amount at the independent price, in USD.
    pub value_usd: Option<String>,
    /// Received versus the independent market value of the filled amount, in percent; negative
    /// when the sale beat the market.
    pub discount_pct: Option<String>,
    /// The part of the cap still held by the owner and not sold.
    pub unsold_raw: String,
    pub unsold_value_usd: Option<String>,
    /// Latest deadline of this token's open intents.
    pub open_until: Option<String>,
    pub fills: Vec<FillRow>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FillRow {
    pub intent_id: String,
    pub amount_in_raw: String,
    pub received_raw: Option<String>,
    pub tx_hash: Option<String>,
    pub filled_at: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Summary {
    pub tokens_sold: usize,
    pub tokens_total: usize,
    /// Net proceeds from fill records only; sold amounts without one are counted under
    /// `unknown_proceeds_*`, not here.
    pub received_raw: String,
    pub received: String,
    pub received_usd: Option<String>,
    /// Independent market value of every sold amount, in USD.
    pub sold_value_usd: String,
    /// Tokens with sold amounts that have fill records, and those amounts' value in USD; the
    /// counterpart of `received`.
    pub known_proceeds_tokens: usize,
    pub known_proceeds_value_usd: String,
    /// Tokens with sold amounts that have no fill record (market sales, or the intent index
    /// unreachable), and those amounts' value in USD; their proceeds are unknown.
    pub unknown_proceeds_tokens: usize,
    pub unknown_proceeds_value_usd: String,
    /// Discount over all fills with known proceeds, weighted by market value.
    pub average_discount_pct: Option<String>,
    pub worst_discount_pct: Option<String>,
    pub worst_discount_token: Option<String>,
    pub unsold_value_usd: String,
    pub open_intents: usize,
}
