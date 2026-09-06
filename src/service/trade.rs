// V6 trade orchestration from quote to signed AgentOrder.
// Exports: TradeInput, TradeOptions, TradeOutcome, execute_trade.
// Deps: quote service, order_types, signer trait.

use crate::client::Client;
use crate::evm;
use crate::order_types::{self, AgentOrderDto, UserProxyV6};
use crate::service::quote::{self, QuoteInput, QuoteOutput};
use crate::signer::Signer;
use alloy::primitives::{Address, Bytes, U256};
use alloy::network::TransactionBuilder;
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TradeInput {
    pub chain: String,
    pub from: String,
    pub to: String,
    pub amount: String,
    pub slippage: Option<u16>,
    pub min_out: Option<String>,
    /// Per-trade notional ceiling on amountIn (raw token units). None = no cap.
    #[serde(default)]
    pub max_amount: Option<String>,
    #[serde(default = "agent_order_mode")]
    pub mode: String,
    pub proxy: String,
    pub nonce: Option<String>,
    pub deadline_secs: Option<u64>,
    #[serde(default = "default_true")]
    pub dry_run: bool,
    #[serde(default)]
    pub relay: bool,
    #[serde(default)]
    pub self_submit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TradeOutcome {
    pub dry_run: bool,
    pub mode: String,
    pub quote: serde_json::Value,
    pub order: AgentOrderDto,
    pub digest: String,
    pub signature: String,
    pub relay: Option<serde_json::Value>,
    pub self_submit: Option<SelfSubmitPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelfSubmitPreview {
    pub to: String,
    pub function: String,
    pub calldata: String,
    pub spender: String,
    pub router_data: String,
    pub tx_hash: Option<String>,
}

pub async fn execute_trade(
    client: &Client,
    relay_client: &Client,
    signer: Arc<dyn Signer>,
    input: TradeInput,
    allow_trade: bool,
) -> Result<TradeOutcome> {
    let mut input = input;
    if input.mode != "agent-order" {
        return Err(eyre!("only agent-order mode is implemented in this release"));
    }
    input.dry_run |= !allow_trade;
    if input.relay && input.self_submit && !input.dry_run {
        return Err(eyre!("choose exactly one of --relay or --self-submit"));
    }
    let quote_out = quote::quote(client, quote_input(&input)).await?;
    let config = evm::chain_config(&input.chain)?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    let proxy_address = order_types::parse_address(&input.proxy)?;
    let proxy = UserProxyV6::new(proxy_address, provider.clone());
    let policy = proxy.policyOf(signer.address()).call().await?;
    let generation = policy.generation;
    let order = build_order(&input, signer.address(), generation, &quote_out)?;
    enforce_notional_cap(&order, input.max_amount.as_deref())?;
    let digest = order_types::signing_hash(
        &order,
        &order_types::proxy_domain(quote_out.request.chain_id, proxy_address),
    );
    let chain_digest = proxy.hashAgentOrder(order.clone()).call().await?;
    if digest != chain_digest {
        return Err(eyre!("local AgentOrder digest does not match proxy.hashAgentOrder"));
    }
    let sig = signer.sign_hash(digest).await?;
    let sig_hex = format!("0x{}", hex::encode(sig.as_bytes()));
    let mut self_submit = self_submit_preview(proxy_address, &order, &sig_hex, &quote_out)?;
    let order_dto = order_types::dto_from_agent_order(&order);
    let relay = if input.relay && !input.dry_run {
        Some(relay_client.submit_intent(&serde_json::json!({
            "swap": {
                "order": order_dto,
                "agentSig": sig_hex,
            }
        })).await?)
    } else {
        None
    };
    if input.self_submit && !input.dry_run {
        let wallet = evm::wallet_provider(&evm::rpc_url(config), signer.clone())?;
        let preview = self_submit.as_mut().ok_or_else(|| eyre!("missing self-submit calldata"))?;
        let call = UserProxyV6::executeAsAgentCall {
            o: order,
            agentSig: hex_bytes(&sig_hex)?,
            spender: order_types::parse_address(&preview.spender)?,
            routerData: hex_bytes(&preview.router_data)?,
        };
        let pending = wallet.send_transaction(
            alloy::rpc::types::TransactionRequest::default()
                .with_from(signer.address())
                .with_kind(alloy::primitives::TxKind::Call(proxy_address))
                .with_input(call.abi_encode()),
        ).await?;
        let hash = *pending.tx_hash();
        pending.get_receipt().await?;
        preview.tx_hash = Some(format!("{hash:?}"));
    }
    Ok(TradeOutcome {
        dry_run: input.dry_run,
        mode: input.mode,
        quote: quote_out.response,
        order: order_dto,
        digest: format!("{digest:?}"),
        signature: sig_hex,
        relay,
        self_submit,
    })
}

fn quote_input(input: &TradeInput) -> QuoteInput {
    QuoteInput {
        chain: input.chain.clone(),
        from: input.from.clone(),
        to: input.to.clone(),
        amount: input.amount.clone(),
        slippage: input.slippage,
        verify: true,
    }
}

fn build_order(input: &TradeInput, agent: Address, generation: u64, quote: &QuoteOutput) -> Result<UserProxyV6::AgentOrder> {
    let router = field(&quote.response, &["execution", "target"])
        .or_else(|| field(&quote.response, &["router"]))
        .ok_or_else(|| eyre!("quote response missing execution target/router"))?;
    let min_out = match &input.min_out {
        Some(value) => order_types::parse_u256(value)?,
        None => {
            // Fund-moving trades must not derive their protection floor from the
            // untrusted quote server's output. Only dry-run previews may.
            if !input.dry_run {
                return Err(eyre!(
                    "refusing to trade without an explicit --min-out floor; the quote server's output cannot be trusted as the protection floor"
                ));
            }
            slippage_min_out(&quote.response, input.slippage.unwrap_or(50))?
        }
    };
    Ok(UserProxyV6::AgentOrder {
        agent,
        generation,
        router: order_types::parse_address(router)?,
        tokenIn: order_types::parse_address(&quote.request.token_in)?,
        amountIn: order_types::parse_u256(&quote.request.amount_in)?,
        tokenOut: order_types::parse_address(&quote.request.token_out)?,
        minOut: min_out,
        nonce: next_nonce(input.nonce.as_deref())?,
        deadline: deadline(input.deadline_secs.unwrap_or(120))?,
    })
}

fn self_submit_preview(
    proxy: Address,
    order: &UserProxyV6::AgentOrder,
    sig_hex: &str,
    quote: &QuoteOutput,
) -> Result<Option<SelfSubmitPreview>> {
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
    Ok(Some(SelfSubmitPreview {
        to: format!("{proxy:?}"),
        function: "executeAsAgent".to_string(),
        calldata: format!("0x{}", hex::encode(call.abi_encode())),
        spender: spender_value,
        router_data: router_data.to_string(),
        tx_hash: None,
    }))
}

fn slippage_min_out(response: &serde_json::Value, bps: u16) -> Result<U256> {
    if bps > 10_000 {
        return Err(eyre!("slippage {bps} bps exceeds 100% (max 10000)"));
    }
    let output = response["output"]
        .as_str()
        .ok_or_else(|| eyre!("quote response missing output"))?;
    let quoted = order_types::parse_u256(output)?;
    Ok((quoted * U256::from(10_000u64 - u64::from(bps))) / U256::from(10_000u64))
}

/// Refuse to sign/relay an order whose amountIn exceeds the configured cap.
fn enforce_notional_cap(order: &UserProxyV6::AgentOrder, cap: Option<&str>) -> Result<()> {
    let Some(cap) = cap else {
        return Ok(());
    };
    let cap = order_types::parse_u256(cap)?;
    if order.amountIn > cap {
        return Err(eyre!(
            "order amountIn {} exceeds trade max-amount cap {}; refusing to sign/relay",
            order.amountIn,
            cap
        ));
    }
    Ok(())
}

fn next_nonce(explicit: Option<&str>) -> Result<U256> {
    if let Some(value) = explicit {
        return order_types::parse_u256(value);
    }
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|e| eyre!("failed to generate nonce: {e}"))?;
    Ok(U256::from(u64::from_be_bytes(bytes)))
}

fn deadline(secs: u64) -> Result<U256> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| eyre!("clock before unix epoch: {e}"))?
        .as_secs();
    Ok(U256::from(now + secs))
}

fn field<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str()
}

fn hex_bytes(value: &str) -> Result<Bytes> {
    let trimmed = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(trimmed).map_err(|e| eyre!("invalid hex bytes: {e}"))?;
    Ok(bytes.into())
}

fn agent_order_mode() -> String {
    "agent-order".to_string()
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests;
