// V6 trade orchestration from quote to signed AgentOrder.
// Exports: TradeInput, TradeOutcome, SelfSubmitPreview, execute_trade.
// Deps: quote service, order_types, signer trait, crate::tokens for the chain-selector copy.

use crate::client::Client;
use crate::evm;
use crate::order_types::{self, AgentOrderDto, UserProxyV6};
use crate::service::quote::{self, QuoteInput, QuoteOutput};
use crate::service::submit::{NotConfirmed, Wait};
use crate::signer::Signer;
use crate::tokens::CHAIN_ID_HELP;
use alloy::primitives::{Address, U256};
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

mod self_submit;
pub use self_submit::SelfSubmitPreview;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TradeInput {
    #[schemars(description = CHAIN_ID_HELP)]
    pub chain_id: String,
    pub from: String,
    pub to: String,
    /// Unsigned decimal input amount in the token's smallest unit.
    pub amount: String,
    pub slippage: Option<u16>,
    /// Minimum output as unsigned decimal digits in raw token units. Required for a live trade:
    /// without it the trade is refused, because the quote server's output is not trusted as the
    /// protection floor. Optional in a dry run.
    pub min_out: Option<String>,
    /// Optional unsigned decimal per-trade cap in raw input units.
    #[serde(default)]
    pub max_amount: Option<String>,
    /// Only `agent-order` is implemented; any other value is refused.
    #[serde(default = "agent_order_mode")]
    pub mode: String,
    pub proxy: String,
    pub nonce: Option<String>,
    pub deadline_secs: Option<u64>,
    /// Preview the quote, unsigned AgentOrder and digest with on-chain hash parity checks.
    /// Never signs or returns signed calldata. Defaults to true; forced without --allow-trade.
    #[serde(default = "default_true")]
    pub dry_run: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_submit: Option<SelfSubmitPreview>,
}

impl TradeOutcome {
    /// The error to report after printing this outcome when its broadcast did not confirm.
    pub fn not_confirmed(&self) -> Option<NotConfirmed> {
        self.self_submit.as_ref().and_then(SelfSubmitPreview::not_confirmed)
    }
}

pub async fn execute_trade(
    client: &Client,
    signer: Arc<dyn Signer>,
    input: TradeInput,
    allow_trade: bool,
    wait: Wait,
) -> Result<TradeOutcome> {
    let mut input = input;
    validate_input_amounts(&input)?;
    if input.mode != "agent-order" {
        return Err(eyre!("only agent-order mode is implemented in this release"));
    }
    input.dry_run |= !allow_trade;
    let quote_out = quote::quote(client, quote_input(&input)).await?;
    let config = evm::chain_config(&input.chain_id)?;
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
    let (signature, self_submit) = if input.dry_run {
        (None, None)
    } else {
        let (signature, preview) =
            self_submit::sign_and_submit(signer, &input, &order, digest, &quote_out, wait).await?;
        (Some(signature), Some(preview))
    };
    Ok(TradeOutcome {
        dry_run: input.dry_run, mode: input.mode, quote: quote_out.response,
        order: order_types::dto_from_agent_order(&order), digest: format!("{digest:?}"),
        signature, self_submit,
    })
}

fn quote_input(input: &TradeInput) -> QuoteInput {
    QuoteInput {
        chain_id: input.chain_id.clone(),
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
        Some(value) => order_types::parse_raw_amount("trade min-out", value)?,
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
        amountIn: order_types::parse_raw_amount("quoted trade amount", &quote.request.amount_in)?,
        tokenOut: order_types::parse_address(&quote.request.token_out)?,
        minOut: min_out,
        nonce: next_nonce(input.nonce.as_deref())?,
        deadline: deadline(input.deadline_secs.unwrap_or(120))?,
    })
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

/// Refuse to sign/self-submit an order whose amountIn exceeds the configured cap.
fn enforce_notional_cap(order: &UserProxyV6::AgentOrder, cap: Option<&str>) -> Result<()> {
    let Some(cap) = cap else {
        return Ok(());
    };
    let cap = order_types::parse_raw_amount("trade max-amount", cap)?;
    if order.amountIn > cap {
        return Err(eyre!(
            "order amountIn {} exceeds trade max-amount cap {}; refusing to sign/self-submit",
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

fn validate_input_amounts(input: &TradeInput) -> Result<()> {
    // Trade input, min-out, and cap retain their existing zero-accepted policies.
    order_types::parse_raw_amount("trade amount", &input.amount)?;
    if let Some(min_out) = &input.min_out {
        order_types::parse_raw_amount("trade min-out", min_out)?;
    }
    if let Some(max_amount) = &input.max_amount {
        order_types::parse_raw_amount("trade max-amount", max_amount)?;
    }
    Ok(())
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

fn agent_order_mode() -> String {
    "agent-order".to_string()
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests;
