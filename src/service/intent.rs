// V5 intent placement, discovery, status, and policy service operations.
// Exports: place, list, status, policy and their typed request/response models.
// Deps: crate::{evm, order_types, signer, tokens}, alloy RPC bindings, Client.

use crate::client::Client;
use crate::evm::{self, ChainConfig};
use crate::order_types::{self, Erc20Metadata, IntentAuthorization, IntentSettlerV2, Order, UserProxyFactoryV5, UserProxyV5, UserProxyV5Errors};
use crate::signer::Signer;
use crate::tokens::{resolve_token, scale_amount};
use alloy::primitives::{Address, B256, Bytes, U256};
use alloy::sol_types::{SolCall, SolInterface};
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

mod read;
pub use read::{list, policy, status};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlaceInput {
    pub chain: String,
    pub proxy_owner: String,
    pub from: String,
    pub to: String,
    pub amount: String,
    pub start_out: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StatusInput {
    pub chain: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicyInput {
    pub chain: String,
    pub owner: String,
    pub agent: String,
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
    let config = evm::chain_config(&input.chain)?;
    let owner = order_types::parse_address(&input.proxy_owner)?;
    let agent = signer.address();
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    if input.relay && input.self_submit
        || allow_trade && !input.dry_run && !input.relay && !input.self_submit
    {
        return Err(eyre!("choose exactly one of --relay or --self-submit"));
    }
    let order = build_order(&input, owner, config.id, &provider).await?;
    let proxy_address = proxy_for(&provider, config, owner).await?;
    let proxy = UserProxyV5::new(proxy_address, provider.clone());
    let policy = proxy.policyOf(agent).call().await?;
    enforce_cap(&order, input.max_amount.as_deref())?;
    let auth = IntentAuthorization {
        orderHash: order_types::order_id(&order),
        agent,
        generation: policy.generation,
        nonce: fresh_agent_nonce(&proxy, agent).await?,
        deadline: now()?.saturating_add(input.deadline_secs.unwrap_or(120)),
    };
    let settler = IntentSettlerV2::new(config.settler, provider.clone());
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
        let result = relay_client.announce_intent(&serde_json::json!({
            "chainId": config.id,
            "announce": {"order": order_types::dto_from_order(&order), "auth": hex_bytes(&envelope)},
        })).await?;
        (Some(result), None)
    } else {
        let wallet = evm::wallet_provider(&evm::rpc_url(config), signer)?;
        let pending = IntentSettlerV2::new(config.settler, wallet.clone())
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

async fn proxy_for(provider: &alloy::providers::DynProvider, config: ChainConfig, owner: Address) -> Result<Address> {
    let proxy = UserProxyFactoryV5::new(config.factory, provider.clone()).proxyOf(owner).call().await?;
    if proxy == Address::ZERO { return Err(eyre!("proxy is not deployed for owner {owner:?}")); }
    Ok(proxy)
}

async fn build_order(
    input: &PlaceInput,
    owner: Address,
    chain_id: u64,
    provider: &alloy::providers::DynProvider,
) -> Result<Order> {
    let (token_in, in_decimals) = resolve_order_token(&input.from, chain_id, provider).await?;
    let (token_out, out_decimals) = resolve_order_token(&input.to, chain_id, provider).await?;
    let amount = parse_amount(&input.amount, in_decimals)?;
    let start = parse_amount(&input.start_out, out_decimals)?;
    let end = parse_amount(&input.end_out, out_decimals)?;
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

async fn resolve_order_token(
    input: &str,
    chain_id: u64,
    provider: &alloy::providers::DynProvider,
) -> Result<(Address, u8)> {
    if let Some((address, _, decimals)) = resolve_token(input, chain_id) {
        return Ok((order_types::parse_address(address)?, decimals));
    }
    if !input.starts_with("0x") && !input.starts_with("0X") {
        return Err(eyre!("unknown token '{input}'"));
    }
    let address = order_types::parse_address(input)?;
    let decimals = Erc20Metadata::new(address, provider.clone()).decimals().call().await?;
    Ok((address, decimals))
}

fn enforce_cap(order: &Order, cap: Option<&str>) -> Result<()> {
    let Some(cap) = cap else { return Ok(()); };
    let cap = order_types::parse_u256(cap)?;
    if order.amountIn > cap { return Err(eyre!("order amountIn {} exceeds max-amount cap {}", order.amountIn, cap)); }
    Ok(())
}

async fn fresh_agent_nonce(proxy: &UserProxyV5::UserProxyV5Instance<alloy::providers::DynProvider>, agent: Address) -> Result<U256> {
    for _ in 0..8 {
        let nonce = U256::from(random_nonce()?);
        if !proxy.isAgentNonceUsed(agent, nonce).call().await? {
            return Ok(nonce);
        }
    }
    Err(eyre!("could not find an unused agent nonce"))
}

fn parse_amount(value: &str, decimals: u8) -> Result<U256> {
    let raw = value.strip_prefix("raw:").map(str::to_string)
        .unwrap_or_else(|| scale_amount(value, decimals));
    order_types::parse_u256(&raw)
}

fn authorization_error(error: alloy::contract::Error) -> eyre::Report {
    let Some(data) = error.as_revert_data() else {
        return eyre!("intent authorization transport failed: {error}");
    };
    let reason = match UserProxyV5Errors::UserProxyV5ErrorsErrors::abi_decode(&data) {
        Ok(decoded) => format!("{decoded:?}"),
        Err(_) => format!("unknown revert 0x{}", hex::encode(data)),
    };
    eyre!("intent authorization rejected: {reason}")
}

fn now() -> Result<u64> { Ok(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| eyre!("clock before unix epoch: {e}"))?.as_secs()) }
fn random_nonce() -> Result<u64> { let mut bytes = [0u8; 8]; getrandom::fill(&mut bytes).map_err(|e| eyre!("failed to generate nonce: {e}"))?; Ok(u64::from_be_bytes(bytes)) }
fn hex_bytes(value: &Bytes) -> String { format!("0x{}", hex::encode(value)) }
