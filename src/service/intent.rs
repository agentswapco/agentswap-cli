// V6 intent placement, discovery, status, and policy service operations.
// Exports: place, list, status, policy and their typed request/response models.
// Deps: crate::{evm, order_types, signer, tokens}, alloy RPC bindings, Client.

use crate::client::Client;
use crate::evm::{self, ChainConfig};
use crate::order_types::{self, Erc20Metadata, IntentAuthorization, IntentSettlerV3, Order, UserProxyFactoryV6, UserProxyV6, UserProxyV6Errors};
use crate::signer::Signer;
use crate::order_types::parse_raw_amount;
use crate::tokens::resolve_token;
use alloy::primitives::{Address, B256, Bytes, U256};
use alloy::sol_types::{SolCall, SolInterface};
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

mod read;
#[cfg(test)]
mod tests;
pub use read::{list, policy, status};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlaceInput {
    pub chain: String,
    pub proxy_owner: String,
    pub from: String,
    pub to: String,
    /// Unsigned decimal input amount in the token's smallest unit.
    pub amount: String,
    /// Unsigned decimal starting output in the token's smallest unit.
    pub start_out: String,
    /// Unsigned decimal ending output in the token's smallest unit.
    pub end_out: String,
    #[serde(default)]
    pub decay_secs: Option<u64>,
    #[serde(default)]
    pub duration_secs: Option<u64>,
    #[serde(default)]
    pub deadline_secs: Option<u64>,
    #[serde(default)]
    pub relay: bool,
    #[serde(default)]
    pub self_submit: bool,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    /// Optional unsigned decimal cap in raw input units.
    pub max_amount: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlaceOutcome {
    pub dry_run: bool,
    pub chain_id: u64,
    pub order: order_types::OrderDto,
    pub id: String,
    pub authorization: order_types::IntentAuthorizationDto,
    pub envelope: String,
    pub digest: String,
    pub signature: String,
    pub relay: Option<serde_json::Value>,
    pub tx_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListInput {
    pub chain: String,
    pub owner: Option<String>,
    pub agent: Option<String>,
    #[serde(default)]
    pub lookback_blocks: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StatusInput {
    pub chain: String,
    pub id: String,
    #[serde(default)]
    pub lookback_blocks: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicyInput {
    pub chain: String,
    pub owner: String,
    pub agent: String,
    #[serde(default)]
    pub lookback_blocks: Option<u64>,
    #[serde(default)]
    pub tokens: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IntentRecord {
    pub id: String,
    pub placed_by: String,
    pub owner: String,
    pub agent: Option<String>,
    pub pair: String,
    pub amount_in: String,
    pub start_out: String,
    pub end_out: String,
    pub window: String,
    pub exclusive_window: bool,
    pub floor_now: String,
    pub fee_now: String,
    pub required_now: String,
    pub floor_for_outsider: String,
    pub required_for_outsider: String,
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicyOutput {
    pub owner: String,
    pub agent: String,
    pub proxy: String,
    pub expiry: String,
    pub epoch_len: String,
    pub action_mask: String,
    pub generation: String,
    pub tokens: Vec<TokenPolicy>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TokenPolicy {
    pub token: String,
    pub allowed: bool,
    pub cap: String,
    pub used: String,
    pub epoch_start: String,
}

pub async fn place(
    relay_client: &Client,
    input: PlaceInput,
    signer: Arc<dyn Signer>,
    allow_trade: bool,
) -> Result<PlaceOutcome> {
    validate_input_amounts(&input)?;
    let config = evm::chain_config(&input.chain)?;
    let owner = order_types::parse_address(&input.proxy_owner)?;
    let agent = signer.address();
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    if input.relay && input.self_submit
        || allow_trade && !input.dry_run && !input.relay && !input.self_submit
    {
        return Err(eyre!("choose exactly one of --relay or --self-submit"));
    }
    verify_raw_token_addresses(&provider, config.id, &input).await?;
    let order = build_order(&input, owner, config.id).await?;
    let proxy_address = proxy_for(&provider, config, owner).await?;
    let proxy = UserProxyV6::new(proxy_address, provider.clone());
    let policy = proxy.policyOf(agent).call().await?;
    enforce_cap(&order, input.max_amount.as_deref())?;
    let auth = IntentAuthorization {
        orderHash: order_types::order_id(&order),
        agent,
        generation: policy.generation,
        nonce: fresh_agent_nonce(&proxy, agent).await?,
        deadline: now()?.saturating_add(input.deadline_secs.unwrap_or(120)),
    };
    let settler = IntentSettlerV3::new(config.settler, provider.clone());
    let chain_id = settler.orderHash(order.clone()).call().await?;
    if chain_id != auth.orderHash {
        return Err(eyre!("local intent id does not match settler.orderHash"));
    }
    let digest = order_types::signing_hash(&auth, &order_types::proxy_domain(config.id, proxy_address));
    let chain_digest = proxy.hashIntentAuthorization(auth.clone()).call().await?;
    if digest != chain_digest {
        return Err(eyre!("local authorization digest does not match proxy.hashIntentAuthorization"));
    }
    let sig = signer.sign_hash(digest).await?;
    let sig_bytes: Bytes = sig.as_bytes().to_vec().into();
    let envelope = order_types::authorization_envelope(&auth, &sig_bytes);
    proxy.isIntentAuthorized(order.clone(), envelope.clone())
        .call()
        .await
        .map_err(authorization_error)?;
    let dry_run = input.dry_run || !allow_trade;
    let (relay, tx_hash) = if dry_run {
        (None, None)
    } else if input.relay {
        let result = relay_client.announce_intent(&announce_body(config, &order, &envelope)).await?;
        (Some(result), None)
    } else {
        let wallet = evm::wallet_provider(&evm::rpc_url(config), signer)?;
        let pending = IntentSettlerV3::new(config.settler, wallet.clone())
            .announce(order.clone(), envelope.clone())
            .send().await?;
        let hash = *pending.tx_hash();
        pending.get_receipt().await?;
        (None, Some(format!("{hash:?}")))
    };
    Ok(PlaceOutcome {
        dry_run, chain_id: config.id, order: order_types::dto_from_order(&order),
        id: format!("{:?}", order_types::order_id(&order)),
        authorization: order_types::dto_from_authorization(&auth),
        envelope: hex_bytes(&envelope), digest: format!("{digest:?}"),
        signature: format!("0x{}", hex::encode(sig.as_bytes())), relay, tx_hash,
    })
}

fn announce_body(config: ChainConfig, order: &Order, envelope: &Bytes) -> serde_json::Value {
    serde_json::json!({
        "chainId": config.id,
        "generation": config.generation,
        "announce": {"order": order_types::dto_from_order(order), "auth": hex_bytes(envelope)},
    })
}

async fn proxy_for(provider: &alloy::providers::DynProvider, config: ChainConfig, owner: Address) -> Result<Address> {
    let proxy = UserProxyFactoryV6::new(config.factory, provider.clone()).proxyOf(owner).call().await?;
    if proxy == Address::ZERO { return Err(eyre!("proxy is not deployed for owner {owner:?}")); }
    Ok(proxy)
}

async fn build_order(
    input: &PlaceInput,
    owner: Address,
    chain_id: u64,
) -> Result<Order> {
    let token_in = resolve_order_token(&input.from, chain_id)?;
    let token_out = resolve_order_token(&input.to, chain_id)?;
    let amount = parse_raw_amount("intent amount", &input.amount)?;
    let start = parse_raw_amount("intent start-out", &input.start_out)?;
    let end = parse_raw_amount("intent end-out", &input.end_out)?;
    // Intent placement retains its existing nonzero policy for all three amounts.
    if start == U256::ZERO || end == U256::ZERO || start < end || amount == U256::ZERO {
        return Err(eyre!("intent amounts must be non-zero and start-out must be at least end-out"));
    }
    let start_time = now()?;
    let duration = input.duration_secs.unwrap_or(600);
    let decay = input.decay_secs.unwrap_or(duration).min(duration);
    if decay == 0 { return Err(eyre!("decay-secs must be greater than zero")); }
    let decay_end = start_time.checked_add(decay).ok_or_else(|| eyre!("decay time overflow"))?;
    let end_time = start_time.checked_add(duration).ok_or_else(|| eyre!("end time overflow"))?;
    Ok(Order { owner, recipient: owner, tokenIn: token_in, amountIn: amount, tokenOut: token_out, startAmountOut: start, endAmountOut: end, startTime: U256::from(start_time), decayEndTime: U256::from(decay_end), endTime: U256::from(end_time), appData: B256::ZERO, nonce: U256::from(random_nonce()?), })
}

/// A token given as a raw address must answer `decimals()`, so a mistyped or non-ERC-20 address
/// fails before the owner's agent signs an order that could never settle. Registry symbols are
/// already known to be tokens. The value itself is unused: amounts are raw units.
async fn verify_raw_token_addresses(
    provider: &alloy::providers::DynProvider,
    chain_id: u64,
    input: &PlaceInput,
) -> Result<()> {
    for value in [&input.from, &input.to] {
        if resolve_token(value, chain_id).is_some() {
            continue;
        }
        let address = order_types::parse_address(value)?;
        Erc20Metadata::new(address, provider.clone())
            .decimals()
            .call()
            .await
            .map_err(|error| eyre!("token {value} does not answer decimals(): {error}"))?;
    }
    Ok(())
}

fn resolve_order_token(input: &str, chain_id: u64) -> Result<Address> {
    if let Some((address, _, _)) = resolve_token(input, chain_id) {
        return order_types::parse_address(address);
    }
    if !input.starts_with("0x") && !input.starts_with("0X") {
        return Err(eyre!("unknown token '{input}'"));
    }
    let address = order_types::parse_address(input)?;
    Ok(address)
}

fn enforce_cap(order: &Order, cap: Option<&str>) -> Result<()> {
    let Some(cap) = cap else { return Ok(()); };
    let cap = parse_raw_amount("intent max-amount", cap)?;
    if order.amountIn > cap { return Err(eyre!("order amountIn {} exceeds max-amount cap {}", order.amountIn, cap)); }
    Ok(())
}

async fn fresh_agent_nonce(proxy: &UserProxyV6::UserProxyV6Instance<alloy::providers::DynProvider>, agent: Address) -> Result<U256> {
    for _ in 0..8 {
        let nonce = U256::from(random_nonce()?);
        if !proxy.isAgentNonceUsed(agent, nonce).call().await? {
            return Ok(nonce);
        }
    }
    Err(eyre!("could not find an unused agent nonce"))
}

fn validate_input_amounts(input: &PlaceInput) -> Result<()> {
    parse_raw_amount("intent amount", &input.amount)?;
    parse_raw_amount("intent start-out", &input.start_out)?;
    parse_raw_amount("intent end-out", &input.end_out)?;
    if let Some(max_amount) = &input.max_amount {
        parse_raw_amount("intent max-amount", max_amount)?;
    }
    Ok(())
}

fn authorization_error(error: alloy::contract::Error) -> eyre::Report {
    let Some(data) = error.as_revert_data() else {
        return eyre!("intent authorization transport failed: {error}");
    };
    let reason = match UserProxyV6Errors::UserProxyV6ErrorsErrors::abi_decode(&data) {
        Ok(decoded) => format!("{decoded:?}"),
        Err(_) => format!("unknown revert 0x{}", hex::encode(data)),
    };
    eyre!("intent authorization rejected: {reason}")
}

fn now() -> Result<u64> { Ok(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| eyre!("clock before unix epoch: {e}"))?.as_secs()) }
fn random_nonce() -> Result<u64> { let mut bytes = [0u8; 8]; getrandom::fill(&mut bytes).map_err(|e| eyre!("failed to generate nonce: {e}"))?; Ok(u64::from_be_bytes(bytes)) }
fn hex_bytes(value: &Bytes) -> String { format!("0x{}", hex::encode(value)) }
