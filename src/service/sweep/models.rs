// Sweep inputs and per-token results shared by CLI and MCP.
// Raw amounts remain decimal strings; transaction failures retain their hash and status.
use crate::service::{submit::{NotConfirmed, TxStatus}, trade::TradeOutcome};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(Debug, Clone, Deserialize, JsonSchema, clap::Args)]
pub struct Input {
    #[arg(long = "chainid", help = crate::tokens::CHAIN_ID_HELP)]
    #[schemars(description = crate::tokens::CHAIN_ID_HELP)]
    pub chain_id: String,
    #[arg(long)]
    pub proxy: String,
    /// Receive ERC-20 symbol or address; must belong to the grant basket.
    #[arg(long)]
    pub receive: String,
    /// Unsigned USD decimal threshold, up to 18 fractional places.
    #[arg(long)]
    pub max_usd: String,
    /// Maximum loss relative to independent prices, in basis points (0..=10000).
    #[arg(long)]
    pub max_loss_bps: u16,
    #[arg(long = "token")]
    #[serde(default)]
    pub tokens: Vec<String>,
    #[arg(long)]
    pub lookback_blocks: Option<u64>,
    /// Preview only; forced without --allow-trade. MCP defaults to true.
    #[arg(long)]
    #[serde(default = "super::default_true")]
    pub dry_run: bool,
    /// Broadcast from the agent key's wallet, which pays gas.
    #[arg(long)]
    #[serde(default)]
    pub self_submit: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Row {
    pub token: String,
    pub amount_raw: String,
    pub value_usd: Option<String>,
    pub floor_raw: Option<String>,
    pub quote_out_raw: Option<String>,
    pub outcome: String,
    pub reason: Option<String>,
    pub error: Option<String>,
    pub tx_hash: Option<String>,
    pub tx_status: Option<TxStatus>,
    pub trade: Option<TradeOutcome>,
}

impl Row {
    pub(super) fn new(token: String) -> Self {
        Self { token, amount_raw: "0".into(), value_usd: None, floor_raw: None,
            quote_out_raw: None, outcome: "skipped".into(), reason: None, error: None,
            tx_hash: None, tx_status: None, trade: None }
    }

    pub(super) fn record(&mut self, result: eyre::Result<TradeOutcome>) -> bool {
        match result {
            Err(error) => { self.outcome = "failed".into(); self.error = Some(crate::redact::urls(&error.to_string())); }
            Ok(trade) => {
                if let Some(submit) = &trade.self_submit {
                    self.tx_hash = submit.tx_hash.clone(); self.tx_status = submit.tx_status;
                }
                self.outcome = if trade.dry_run { "dry_run" } else if self.tx_hash.is_none() { "signed" } else { "sold" }.into();
                if let Some(failure) = trade.not_confirmed() {
                    self.outcome = "failed".into(); self.error = Some(failure.to_string());
                }
                self.trade = Some(trade);
            }
        }
        matches!(self.tx_status, Some(TxStatus::Reverted | TxStatus::Unknown))
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Output {
    pub dry_run: bool,
    pub owner: String,
    pub note: Option<String>,
    pub tokens: Vec<Row>,
}

impl Output {
    pub fn check(&self) -> eyre::Result<()> {
        for row in &self.tokens {
            if let Some(failure) = NotConfirmed::check(row.tx_hash.as_deref(), row.tx_status, row.error.as_deref()) {
                return Err(failure.into());
            }
        }
        eyre::ensure!(!self.tokens.iter().any(|r| r.outcome == "failed"), "sweep contains failed trades; see token rows");
        Ok(())
    }
}
