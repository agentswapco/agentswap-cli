// Sweep inputs and per-token results shared by CLI and MCP.
// Raw amounts remain decimal strings; transaction failures retain their hash and status.
use crate::service::{submit::{NotConfirmed, TxStatus}, trade::TradeOutcome};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    #[default]
    Intent,
    Market,
}

impl Via {
    pub(super) fn require_action(self, mask: u8) -> eyre::Result<()> {
        let (bit, name) = match self { Self::Intent => (0x04, "intent"), Self::Market => (0x01, "market") };
        eyre::ensure!(mask & bit != 0, "agent policy lacks {name} action (0x{bit:02x})");
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Input {
    pub chain_id: String,
    pub proxy: String,
    /// Receive ERC-20 symbol or address; must belong to the grant basket.
    pub receive: String,
    /// Unsigned USD decimal threshold for the owner's whole holding, up to 18 fractional places.
    pub max_usd: String,
    /// Maximum loss relative to independent prices, in basis points (0 or greater, less than 10000).
    pub max_loss_bps: u16,
    /// Confirmed basket ERC-20 addresses to sell.
    pub tokens: Vec<String>,
    /// Preview only; forced when live trading is disabled.
    #[serde(default = "super::default_true")]
    pub dry_run: bool,
    /// Relayed intent (gasless) or market trade; requires the corresponding policy action.
    #[serde(default)]
    pub via: Via,
    /// Wait up to this many seconds after placement for intent fill, expiry or cancellation.
    #[serde(default)]
    pub wait: Option<u64>,
    /// Market mode only: broadcast from the agent key's wallet, which pays gas.
    #[serde(default)]
    pub self_submit: bool,
    /// Intent mode only: start each intent this many bps above independent market value.
    #[serde(default = "super::default_start_premium_bps")]
    pub start_premium_bps: u16,
    #[serde(default)]
    pub confirmed_generation: Option<String>,
    #[serde(default)]
    pub request_caps: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Row {
    pub token: String,
    pub amount_raw: String,
    pub received_raw: Option<String>,
    pub value_usd: Option<String>,
    pub floor_raw: Option<String>,
    pub quote_out_raw: Option<String>,
    pub intent_id: Option<String>,
    pub start_out_raw: Option<String>,
    pub announce_status: Option<String>,
    pub relay: Option<serde_json::Value>,
    pub intent_status: Option<String>,
    pub status_error: Option<String>,
    pub wait_timed_out: bool,
    pub placement_block: Option<u64>,
    pub warnings: Vec<String>,
    pub outcome: String,
    pub reason: Option<String>,
    pub error: Option<String>,
    pub tx_hash: Option<String>,
    pub tx_status: Option<TxStatus>,
    pub trade: Option<TradeOutcome>,
}

impl Row {
    pub(crate) fn new(token: String) -> Self {
        Self { token, received_raw: None, amount_raw: "0".into(), value_usd: None, floor_raw: None,
            quote_out_raw: None, outcome: "skipped".into(), reason: None, error: None,
            tx_hash: None, tx_status: None, trade: None, intent_id: None, start_out_raw: None,
            announce_status: None, relay: None, intent_status: None, status_error: None, wait_timed_out: false, placement_block: None, warnings: Vec::new() }
    }

    pub(super) fn record(&mut self, result: eyre::Result<TradeOutcome>) -> bool {
        match result {
            Err(error) => { self.outcome = "failed".into(); self.error = Some(crate::redact::urls(&error.to_string())); }
            Ok(trade) => {
                if let Some(submit) = &trade.self_submit {
                    self.tx_hash = submit.tx_hash.clone(); self.tx_status = submit.tx_status;
                }
                self.reason = if trade.dry_run { Some("dry_run".into()) } else if self.tx_hash.is_none() { Some("not_submitted".into()) } else { None };
                self.outcome = if self.reason.is_some() { "skipped" } else { "sold" }.into();
                if let Some(failure) = trade.not_confirmed() {
                    self.outcome = "failed".into(); self.error = failure.error;
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
        eyre::ensure!(!self.tokens.iter().any(|r| r.outcome == "failed"), "batch sale contains failed placements or trades; see token rows");
        Ok(())
    }
}
