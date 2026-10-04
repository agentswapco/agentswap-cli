// Self-submitted transactions: sign first, print the hash, broadcast, then a bounded receipt wait.
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
pub const EXIT_STATUS_HELP: &str = "Exit status: 0 success; 1 the command failed, and no --self-submit transaction was broadcast; 2 invalid arguments; 3 a --self-submit transaction was mined and reverted; 4 a --self-submit transaction was sent but its receipt was not read, because the RPC failed or the bounded wait ended, so it may still be mined. --self-submit prints the transaction hash on stderr before it sends the transaction; with 3 and 4 the output, --json included, also carries the hash and its status. Look the hash up before sending again. A --relay error exits 1 without showing whether the relay broadcast the announce; check intent list --agent before placing again.";

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

/// How long the broadcast, the hash lookup and the receipt wait may take.
#[derive(Debug, Clone, Copy)]
pub struct Wait {
    pub receipt: Duration,
    pub poll: Duration,
    pub call: Duration,
}

impl Wait {
    /// The command line: long enough for a congested block.
    pub const CLI: Self = Self {
        receipt: Duration::from_secs(120),
        poll: Duration::from_secs(2),
        call: Duration::from_secs(20),
    };
    /// MCP tools: the broadcast, the lookup and the receipt wait together end well inside a 60 s
    /// client request timeout, so the client receives the hash as an unknown outcome instead of
    /// timing out without it.
    pub const MCP: Self = Self {
        receipt: Duration::from_secs(30),
        poll: Duration::from_secs(2),
        call: Duration::from_secs(8),
    };
}

/// Error answers to eth_sendRawTransaction that a node gives only before it accepts a transaction,
/// so none of them can follow a broadcast. Matched case-insensitively within the message.
const REFUSALS: [&str; 10] = [
    "insufficient funds",
    "intrinsic gas too low",
    "less than block base fee",
    "invalid sender",
    "invalid chain id",
    "chain id mismatch",
    "exceeds block gas limit",
    "oversized data",
    "transaction type not supported",
    "tx type not supported",
];

/// How an error answer to eth_sendRawTransaction is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendAnswer {
    /// A refusal from `REFUSALS`: nothing was broadcast.
    Refused,
    /// The RPC already holds this transaction.
    AlreadyKnown,
    /// Any other answer, which may follow an accepted send: look the hash up.
    Ambiguous,
}

fn classify(message: &str) -> SendAnswer {
    let message = message.to_ascii_lowercase();
    if message.contains("already known") || message.contains("already imported") || message.starts_with("known transaction") {
        SendAnswer::AlreadyKnown
    } else if REFUSALS.iter().any(|refusal| message.contains(refusal)) {
        SendAnswer::Refused
    } else {
        SendAnswer::Ambiguous
    }
}

/// Sign `input` to `to` from the signer's wallet, print its hash on stderr, broadcast it and wait
/// for its receipt. An error means nothing was broadcast; once the RPC may hold the transaction,
/// the result carries its hash.
pub async fn send(url: &str, signer: Arc<dyn Signer>, to: Address, input: Bytes, wait: Wait) -> Result<Submission> {
    let request = TransactionRequest::default()
        .with_from(signer.address())
        .with_to(to)
        .with_input(input);
    let (provider, envelope) = evm::sign_transaction(url, signer, request).await?;
    let hash = *envelope.tx_hash();
    eprintln!("sending transaction {hash:?}");
    let sent = timeout(wait.call, provider.send_raw_transaction(&envelope.encoded_2718())).await;
    let failure = match sent {
        Ok(Ok(_)) => return Ok(await_receipt(&provider, hash, wait).await),
        Ok(Err(error)) => match error.as_error_resp() {
            Some(answer) => match classify(&answer.message) {
                SendAnswer::Refused => {
                    return Err(eyre!("the RPC refused transaction {hash:?}, so it was not broadcast: {answer}"));
                }
                SendAnswer::AlreadyKnown => return Ok(await_receipt(&provider, hash, wait).await),
                SendAnswer::Ambiguous => format!("eth_sendRawTransaction answered {answer}"),
            },
            None => format!("eth_sendRawTransaction failed: {error}"),
        },
        Err(_) => "eth_sendRawTransaction gave no answer".to_string(),
    };
    if rpc_knows(&provider, hash, wait.call).await {
        return Ok(await_receipt(&provider, hash, wait).await);
    }
    Ok(Submission::unknown(hash, format!("{failure}; a lookup by hash did not find it")))
}

/// One eth_getTransactionByHash; any non-null answer counts, whatever the chain's format.
async fn rpc_knows(provider: &DynProvider, hash: B256, limit: Duration) -> bool {
    let lookup = provider.raw_request::<_, serde_json::Value>("eth_getTransactionByHash".into(), (hash,));
    matches!(timeout(limit, lookup).await, Ok(Ok(found)) if !found.is_null())
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
            let waited = "no receipt before the wait ended";
            let error = last_error.map_or(waited.to_string(), |error| format!("{waited}; last error: {error}"));
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
