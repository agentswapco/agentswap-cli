// Observed owner proceeds: intent fills from the intent index's fill records, else from
// IntentFilled logs; market trades from their receipts. Only net proceeds count as received.
use crate::{evm, order_types::{self, IntentSettlerV3}, service::{intentscan::{self, Origins}, sweep::{Output, Row}, submit::TxStatus}};
use alloy::{primitives::{Address, B256}, providers::{DynProvider, Provider}, sol_types::SolEvent};
use eyre::{Result, eyre};

alloy::sol! {
    event ExecutedAsAgent(address indexed agent, address indexed router, address indexed relayer,
        address tokenIn, address tokenOut, uint256 amountIn, uint256 amountOut);
}

pub(super) async fn report(chain: &str, proxy: &str, receive: &str, origins: &Origins<'_>, output: &mut Output) -> Result<()> {
    let config = evm::chain_config(chain)?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    for row in &mut output.tokens {
        let received = if row.intent_status.as_deref() == Some("filled") {
            row.outcome = "sold".into();
            match indexed_received(origins, row).await {
                Some(raw) => Ok(raw),
                None => intent_received(&provider, config, row).await,
            }
        } else if row.tx_status == Some(TxStatus::Confirmed) {
            market_received(&provider, proxy.parse()?, receive.parse()?, row).await
        } else {
            if matches!(row.intent_status.as_deref(), Some("expired" | "cancelled" | "dead" | "not_placed")) {
                row.outcome = "skipped".into(); row.reason = row.intent_status.clone();
            }
            continue;
        };
        match received {
            Ok(raw) => row.received_raw = Some(raw),
            Err(error) => row.warnings.push(format!("received amount unavailable: {}", crate::redact::urls(&error.to_string()))),
        }
    }
    Ok(())
}

/// Net output from the index's fill record; None when the index has no fill for the intent yet.
async fn indexed_received(origins: &Origins<'_>, row: &Row) -> Option<String> {
    let id = row.intent_id.as_deref()?.parse().ok()?;
    intentscan::fill(origins, id).await.ok().flatten().map(|fill| fill.received.to_string())
}

async fn intent_received(provider: &DynProvider, config: evm::ChainConfig, row: &Row) -> Result<String> {
    let id: B256 = row.intent_id.as_deref().ok_or_else(|| eyre!("missing intent id"))?.parse()?;
    let first = row.placement_block.ok_or_else(|| eyre!("missing placement block"))?;
    let latest = provider.get_block_number().await?;
    let contract = IntentSettlerV3::new(config.settler, provider.clone());
    let mut start = first;
    while start <= latest {
        let end = start.saturating_add(evm::EVENT_CHUNK_SIZE - 1).min(latest);
        let mut filter = contract.IntentFilled_filter();
        filter.filter = filter.filter.from_block(start).to_block(end).topic1(id);
        if let Some((event, _)) = filter.query().await?.into_iter().next() {
            return Ok(event.receivedOut.checked_sub(event.fee).ok_or_else(|| eyre!("fill fee exceeds gross received"))?.to_string());
        }
        if end == latest { break; }
        start = end + 1;
    }
    Err(eyre!("fill event unavailable"))
}

async fn market_received(provider: &DynProvider, proxy: Address, receive: Address, row: &Row) -> Result<String> {
    let hash = row.tx_hash.as_deref().ok_or_else(|| eyre!("missing transaction hash"))?.parse()?;
    let receipt = provider.get_transaction_receipt(hash).await?.ok_or_else(|| eyre!("receipt unavailable"))?;
    for log in receipt.inner.logs() {
        if log.address() != proxy { continue; }
        if let Ok(event) = ExecutedAsAgent::decode_log(&log.inner) {
            if event.tokenIn == order_types::parse_address(&row.token)? && event.tokenOut == receive
                && event.amountIn == order_types::parse_u256(&row.amount_raw)? {
                return Ok(event.amountOut.to_string());
            }
        }
    }
    Err(eyre!("execution event unavailable"))
}

#[cfg(test)]
#[path = "received_tests.rs"]
mod tests;
