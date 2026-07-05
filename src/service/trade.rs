// Trade orchestration from quote to signed AgentOrder.
// Exports: TradeInput, TradeOptions, TradeOutcome, execute_trade.
// Deps: quote service, order_types, signer trait.

use crate::client::Client;
use crate::order_types::{self, AgentOrderDto, UserProxyV3};
use crate::service::quote::{self, QuoteInput, QuoteOutput};
use crate::signer::Signer;
use alloy::primitives::{Address, Bytes, U256};
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
}

pub async fn execute_trade(
    client: &Client,
    relay_client: &Client,
    signer: Arc<dyn Signer>,
    input: TradeInput,
    allow_trade: bool,
) -> Result<TradeOutcome> {
    if input.mode != "agent-order" {
        return Err(eyre!("only agent-order mode is implemented in this release"));
    }
    let quote_out = quote::quote(client, quote_input(&input)).await?;
    let order = build_order(&input, signer.address(), &quote_out)?;
    enforce_notional_cap(&order, input.max_amount.as_deref())?;
    let proxy = order_types::parse_address(&input.proxy)?;
    let digest = order_types::signing_hash(&order, quote_out.request.chain_id, proxy);
    let sig = signer.sign_hash(digest).await?;
    let sig_hex = format!("0x{}", hex::encode(sig.as_bytes()));
    let self_submit = self_submit_preview(proxy, &order, &sig_hex, &quote_out)?;
    let order_dto = order_types::dto_from_agent_order(&order);
    let relay = if input.relay && !input.dry_run {
        if !allow_trade {
            return Err(eyre!("trade execution requires --allow-trade"));
        }
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
        return Err(eyre!("self-submit broadcasting is deferred; dry-run returns calldata"));
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

fn build_order(input: &TradeInput, agent: Address, quote: &QuoteOutput) -> Result<UserProxyV3::AgentOrder> {
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
    Ok(UserProxyV3::AgentOrder {
        agent,
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
    order: &UserProxyV3::AgentOrder,
    sig_hex: &str,
    quote: &QuoteOutput,
) -> Result<Option<SelfSubmitPreview>> {
    let router_data = field(&quote.response, &["execution", "calldata"])
        .or_else(|| field(&quote.response, &["calldata"]))
        .ok_or_else(|| eyre!("quote response missing router calldata"))?;
    let spender_value = field(&quote.response, &["execution", "spender"])
        .map(String::from)
        .unwrap_or_else(|| format!("{:?}", order.router));
    let call = UserProxyV3::executeAsAgentCall {
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
fn enforce_notional_cap(order: &UserProxyV3::AgentOrder, cap: Option<&str>) -> Result<()> {
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
mod tests {
    use super::*;
    use crate::service::quote::QuoteContext;

    fn quote_output() -> QuoteOutput {
        QuoteOutput {
            request: QuoteContext {
                chain_id: 8453,
                token_in: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
                token_in_symbol: "USDC".to_string(),
                token_in_decimals: 6,
                token_out: "0x4200000000000000000000000000000000000006".to_string(),
                token_out_symbol: "WETH".to_string(),
                token_out_decimals: 18,
                amount_in: "1000000".to_string(),
            },
            response: serde_json::json!({
                "router": "0x1111111111111111111111111111111111111111",
                "output": "500000000000000000",
            }),
        }
    }

    fn trade_input(min_out: Option<String>, dry_run: bool) -> TradeInput {
        TradeInput {
            chain: "base".to_string(),
            from: "USDC".to_string(),
            to: "WETH".to_string(),
            amount: "1".to_string(),
            slippage: Some(50),
            min_out,
            max_amount: None,
            mode: "agent-order".to_string(),
            proxy: "0x2222222222222222222222222222222222222222".to_string(),
            nonce: Some("1".to_string()),
            deadline_secs: Some(120),
            dry_run,
            relay: false,
            self_submit: false,
        }
    }

    #[test]
    fn refuses_fund_moving_trade_without_min_out() {
        let agent = Address::ZERO;
        let err = build_order(&trade_input(None, false), agent, &quote_output())
            .expect_err("must refuse missing min_out on fund-moving trade");
        assert!(format!("{err}").contains("explicit --min-out floor"), "{err}");
    }

    #[test]
    fn allows_dry_run_without_min_out() {
        let agent = Address::ZERO;
        build_order(&trade_input(None, true), agent, &quote_output())
            .expect("dry-run may derive min_out");
    }

    #[test]
    fn rejects_slippage_over_10000_bps() {
        let response = serde_json::json!({ "output": "1000000" });
        assert!(slippage_min_out(&response, 10_001).is_err());
        assert!(slippage_min_out(&response, 10_000).is_ok());
    }

    #[test]
    fn enforces_notional_cap() {
        let agent = Address::ZERO;
        let order = build_order(&trade_input(Some("1".to_string()), false), agent, &quote_output())
            .expect("build order");
        // amountIn is 1_000_000; a cap of 500_000 must be rejected.
        assert!(enforce_notional_cap(&order, Some("500000")).is_err());
        assert!(enforce_notional_cap(&order, Some("1000000")).is_ok());
        assert!(enforce_notional_cap(&order, None).is_ok());
    }
}
