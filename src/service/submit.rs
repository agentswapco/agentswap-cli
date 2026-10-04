// Self-submitted transactions: sign first, broadcast, then a bounded receipt wait.
// Exports: TxStatus, Submission, NotConfirmed, Wait, send.
// Deps: crate::{evm, redact, signer}, alloy provider/network types, tokio time.

use crate::evm;
use crate::signer::Signer;
use alloy::network::eip2718::Encodable2718;
use alloy::network::TransactionBuilder;
use alloy::primitives::{Address, Bytes, B256};
use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::TransactionRequest;
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout, Instant};

#[cfg(test)]
mod tests;

/// Exit status of a run whose transaction was mined and reverted.
pub const EXIT_REVERTED: i32 = 3;
/// Exit status of a run whose transaction was sent but whose receipt was never read.
pub const EXIT_UNKNOWN: i32 = 4;

/// The exit statuses, stated once for `--help` and the README.
pub const EXIT_STATUS_HELP: &str = "Exit status: 0 success; 1 the command failed, and no --self-submit transaction was broadcast; 2 invalid arguments; 3 a --self-submit transaction was mined and reverted; 4 a --self-submit transaction was sent but its receipt was not read, because the RPC failed or the bounded wait ended, so it may still be mined. With 3 and 4 the output, --json included, carries the transaction hash and its status; look the hash up before sending again. A --relay error exits 1 without showing whether the relay broadcast the announce; check intent list --agent before placing again.";

/// What is known about a transaction after it left this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TxStatus {
    /// Mined with receipt status 1.
    Confirmed,
    /// Mined with receipt status 0: the call reverted and only the gas was spent.
    Reverted,
    /// Sent, but no receipt was read before the wait ended or the RPC failed. The transaction may
    /// still be mined: look the hash up before sending again.
    Unknown,
}

impl TxStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Reverted => "reverted",
            Self::Unknown => "unknown",
        }
    }
}

/// A signed transaction's hash and what the receipt wait learned about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    pub hash: B256,
    pub status: TxStatus,
    /// Why the status is unknown; set only for `Unknown`.
    pub error: Option<String>,
}

/// How long the broadcast and the receipt wait may take.
#[derive(Debug, Clone, Copy)]
pub struct Wait {
    pub receipt: Duration,
    pub poll: Duration,
    pub call: Duration,
}

impl Wait {
    pub const DEFAULT: Self = Self {
        receipt: Duration::from_secs(120),
        poll: Duration::from_secs(2),
        call: Duration::from_secs(20),
    };
}

/// Sign `input` to `to` from the signer's wallet, broadcast it and wait for its receipt. An error
/// means nothing was broadcast; once the RPC may hold the transaction, the result carries its hash.
pub async fn send(url: &str, signer: Arc<dyn Signer>, to: Address, input: Bytes, wait: Wait) -> Result<Submission> {
    let request = TransactionRequest::default()
        .with_from(signer.address())
        .with_to(to)
        .with_input(input);
    let (provider, envelope) = evm::sign_transaction(url, signer, request).await?;
    let hash = *envelope.tx_hash();
    let raw = envelope.encoded_2718();
    let sent = timeout(wait.call, provider.send_raw_transaction(&raw)).await;
    let error = match sent {
        Ok(Ok(_)) => return Ok(await_receipt(&provider, hash, wait).await),
        Ok(Err(error)) => match error.as_error_resp() {
            Some(refusal) => return Err(eyre!("the RPC refused transaction {hash:?}, so it was not broadcast: {refusal}")),
            None => format!("eth_sendRawTransaction failed: {error}"),
        },
        Err(_) => format!("eth_sendRawTransaction gave no answer within {} s", wait.call.as_secs()),
    };
    Ok(Submission::unknown(hash, error))
}

/// Poll for the receipt until `wait.receipt` has passed. A failed poll is retried, so one
/// rate-limited request does not end the wait.
async fn await_receipt(provider: &DynProvider, hash: B256, wait: Wait) -> Submission {
    let deadline = Instant::now() + wait.receipt;
    let mut last_error = None;
    loop {
        let budget = wait.call.min(deadline.saturating_duration_since(Instant::now()));
        match timeout(budget, provider.get_transaction_receipt(hash)).await {
            Ok(Ok(Some(receipt))) => return Submission::mined(hash, receipt.status()),
            Ok(Ok(None)) => {}
            Ok(Err(error)) => last_error = Some(format!("eth_getTransactionReceipt failed: {error}")),
            Err(_) => last_error = Some("eth_getTransactionReceipt gave no answer".to_string()),
        }
        if Instant::now() + wait.poll >= deadline {
            let waited = format!("no receipt within {} s", wait.receipt.as_secs());
            let error = last_error.map_or(waited.clone(), |error| format!("{waited}; last error: {error}"));
            return Submission::unknown(hash, error);
        }
        sleep(wait.poll).await;
    }
}

impl Submission {
    fn mined(hash: B256, success: bool) -> Self {
        let status = if success { TxStatus::Confirmed } else { TxStatus::Reverted };
        Self { hash, status, error: None }
    }

    fn unknown(hash: B256, error: String) -> Self {
        Self { hash, status: TxStatus::Unknown, error: Some(crate::redact::urls(&error)) }
    }

    pub fn hash_hex(&self) -> String {
        format!("{:?}", self.hash)
    }
}

/// The error a command returns after printing an outcome whose transaction did not confirm; its
/// exit status tells a reverted transaction from one whose outcome is unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotConfirmed {
    pub hash: String,
    pub status: TxStatus,
    pub error: Option<String>,
}

impl NotConfirmed {
    /// `None` when no transaction was sent or it confirmed.
    pub fn check(hash: Option<&str>, status: Option<TxStatus>, error: Option<&str>) -> Option<Self> {
        let status = status.filter(|status| *status != TxStatus::Confirmed)?;
        let hash = hash.unwrap_or_default().to_string();
        Some(Self { hash, status, error: error.map(String::from) })
    }

    pub fn exit_code(&self) -> i32 {
        match self.status {
            TxStatus::Reverted => EXIT_REVERTED,
            _ => EXIT_UNKNOWN,
        }
    }
}

impl std::fmt::Display for NotConfirmed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hash = &self.hash;
        match self.status {
            TxStatus::Reverted => write!(f, "transaction {hash} was mined and reverted"),
            _ => write!(
                f,
                "transaction {hash} was sent but its outcome is unknown ({}); look the hash up before sending again",
                self.error.as_deref().unwrap_or("no receipt")
            ),
        }
    }
}

impl std::error::Error for NotConfirmed {}
