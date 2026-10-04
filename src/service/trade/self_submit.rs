// Signed executeAsAgent calldata for a live trade and its optional broadcast by the agent wallet.
// Exports: SelfSubmitPreview, sign_and_submit.
// Deps: parent trade module, crate::{evm, order_types, service::submit, signer}.

use super::{field, TradeInput};
use crate::evm;
use crate::order_types::{self, UserProxyV6};
use crate::service::quote::QuoteOutput;
use crate::service::submit::{self, NotConfirmed, Submission, TxStatus};
use crate::signer::Signer;
use alloy::primitives::{Address, Bytes, B256};
use alloy::sol_types::SolCall;
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelfSubmitPreview {
    pub to: String,
    pub function: String,
    pub calldata: String,
    pub spender: String,
    pub router_data: String,
    /// Hash of the broadcast executeAsAgent transaction, set as soon as it was sent.
    pub tx_hash: Option<String>,
    /// Receipt outcome of the broadcast transaction: confirmed, reverted, or unknown when no
    /// receipt was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_status: Option<TxStatus>,
    /// Why tx_status is unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_error: Option<String>,
    /// Explorer page of the transaction, on chains with a known explorer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_explorer_url: Option<String>,
}

impl SelfSubmitPreview {
    fn record(&mut self, submission: Submission, chain_id: u64) {
        let hash = submission.hash_hex();
        self.tx_explorer_url = crate::display::tx_url(chain_id, &hash);
        self.tx_hash = Some(hash);
        self.tx_status = Some(submission.status);
        self.tx_error = submission.error;
    }

    pub fn not_confirmed(&self) -> Option<NotConfirmed> {
        NotConfirmed::check(self.tx_hash.as_deref(), self.tx_status, self.tx_error.as_deref())
    }
}

/// Sign the AgentOrder digest and, with --self-submit, broadcast the previewed calldata. An error
/// means nothing was broadcast; a sent transaction's hash and outcome are in the preview.
pub(super) async fn sign_and_submit(
    signer: Arc<dyn Signer>,
    input: &TradeInput,
    order: &UserProxyV6::AgentOrder,
    digest: B256,
    quote_out: &QuoteOutput,
    wait: submit::Wait,
) -> Result<(String, SelfSubmitPreview)> {
    let proxy_address = order_types::parse_address(&input.proxy)?;
    let sig = signer.sign_hash(digest).await?;
    let sig_hex = format!("0x{}", hex::encode(sig.as_bytes()));
    let mut preview = self_submit_preview(proxy_address, order, &sig_hex, quote_out)?;
    if input.self_submit {
        let config = evm::chain_config(&input.chain_id)?;
        let calldata = hex_bytes(&preview.calldata)?;
        let rpc = evm::rpc_url(config);
        let submission = submit::send(&rpc, signer, proxy_address, calldata, wait).await?;
        preview.record(submission, config.id);
    }
    Ok((sig_hex, preview))
}

fn self_submit_preview(
    proxy: Address,
    order: &UserProxyV6::AgentOrder,
    sig_hex: &str,
    quote: &QuoteOutput,
) -> Result<SelfSubmitPreview> {
    let router_data = field(&quote.response, &["execution", "calldata"])
        .or_else(|| field(&quote.response, &["calldata"]))
        .ok_or_else(|| eyre!("quote response missing router calldata"))?;
    let spender_value = field(&quote.response, &["execution", "spender"])
        .map(String::from)
        .unwrap_or_else(|| format!("{:?}", order.router));
    let call = UserProxyV6::executeAsAgentCall {
        o: order.clone(),
        agentSig: hex_bytes(sig_hex)?,
        spender: order_types::parse_address(&spender_value)?,
        routerData: hex_bytes(router_data)?,
    };
    Ok(SelfSubmitPreview {
        to: format!("{proxy:?}"),
        function: "executeAsAgent".to_string(),
        calldata: format!("0x{}", hex::encode(call.abi_encode())),
        spender: spender_value,
        router_data: router_data.to_string(),
        tx_hash: None,
        tx_status: None,
        tx_error: None,
        tx_explorer_url: None,
    })
}

fn hex_bytes(value: &str) -> Result<Bytes> {
    let trimmed = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(trimmed).map_err(|e| eyre!("invalid hex bytes: {e}"))?;
    Ok(bytes.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(status: Option<TxStatus>) -> SelfSubmitPreview {
        let mut preview = SelfSubmitPreview {
            to: String::new(), function: String::new(), calldata: String::new(), spender: String::new(),
            router_data: String::new(), tx_hash: None, tx_status: None, tx_error: None,
            tx_explorer_url: None,
        };
        if let Some(status) = status {
            preview.record(Submission { hash: B256::repeat_byte(7), status, error: None }, 5042002);
        }
        preview
    }

    #[test]
    fn a_recorded_submission_sets_hash_and_status_in_the_camel_case_json() {
        let value = serde_json::to_value(preview(Some(TxStatus::Reverted))).unwrap();
        assert_eq!(value["txHash"], format!("{:?}", B256::repeat_byte(7)));
        assert_eq!(value["txStatus"], "reverted");
        assert!(value.get("txError").is_none());
        let url = format!("https://testnet.arcscan.app/tx/{:?}", B256::repeat_byte(7));
        assert_eq!(value["txExplorerUrl"], url);
        let unsent = serde_json::to_value(preview(None)).unwrap();
        assert!(unsent["txHash"].is_null() && unsent.get("txStatus").is_none());
    }

    #[test]
    fn only_a_sent_transaction_that_did_not_confirm_is_not_confirmed() {
        assert!(preview(None).not_confirmed().is_none());
        assert!(preview(Some(TxStatus::Confirmed)).not_confirmed().is_none());
        let reverted = preview(Some(TxStatus::Reverted)).not_confirmed().expect("reverted");
        assert_eq!(reverted.exit_code(), submit::EXIT_REVERTED);
        assert_eq!(reverted.hash, format!("{:?}", B256::repeat_byte(7)));
    }
}
