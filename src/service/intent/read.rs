// V6 intent discovery, status, and policy reads with RPC-compatible log windows.
// Exports: list, status, policy.
// Deps: parent intent models, crate::{evm, order_types}, alloy providers.

use super::*;
use crate::evm::{self, ChainConfig};
use crate::order_types::{
    self, IntentAbiCodec, IntentLensV3, IntentSettlerV3, Order, UserProxyV6,
};
use alloy::primitives::U256;
use alloy::providers::Provider;
use alloy::sol_types::SolType;
use std::collections::BTreeSet;

pub async fn list(input: ListInput) -> Result<Vec<IntentRecord>> {
    let config = evm::chain_config(&input.chain_id)?;
    let owner = input.owner.as_deref().map(order_types::parse_address).transpose()?;
    let agent = input.agent.as_deref().map(order_types::parse_address).transpose()?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    let lens = IntentLensV3::new(config.lens, provider.clone());
    let layout = lens.PREVIEW_LAYOUT().call().await?;
    if layout != U256::from(3) {
        return Err(eyre!("IntentLensV3 PREVIEW_LAYOUT is {layout}, expected 3"));
    }
    let settler = IntentSettlerV3::new(config.settler, provider.clone());
    let lookback = evm::event_lookback_blocks(config, input.lookback_blocks);
    let first_block = evm::event_start_block(&provider, lookback).await?;
    let latest = provider.get_block_number().await?;
    let mut out = Vec::new();
    let mut block = first_block;
    while block <= latest {
        let end = block.saturating_add(evm::EVENT_CHUNK_SIZE - 1).min(latest);
        let mut filter = settler.IntentAnnounced_filter();
        if let Some(owner) = owner { filter.filter = filter.filter.topic2(owner); }
        filter.filter = filter.filter.from_block(block).to_block(end);
        for (event, _) in filter
            .query()
            .await
            .map_err(|error| evm::event_query_error(config, block, end, error))?
        {
            let (order, auth_agent) = decode_event(&event)?;
            if agent.is_some() && auth_agent != agent { continue; }
            out.push(record(&provider, config, order, auth_agent).await?);
        }
        if end == latest { break; }
        block = end.saturating_add(1);
    }
    Ok(out)
}

pub async fn status(input: StatusInput) -> Result<IntentRecord> {
    let config = evm::chain_config(&input.chain_id)?;
    let id = parse_b256(&input.id)?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    let lens = IntentLensV3::new(config.lens, provider.clone());
    let layout = lens.PREVIEW_LAYOUT().call().await?;
    if layout != U256::from(3) {
        return Err(eyre!("IntentLensV3 PREVIEW_LAYOUT is {layout}, expected 3"));
    }
    let settler = IntentSettlerV3::new(config.settler, provider.clone());
    let lookback = evm::event_lookback_blocks(config, input.lookback_blocks);
    let first_block = evm::event_start_block(&provider, lookback).await?;
    let latest = provider.get_block_number().await?;
    let mut block = first_block;
    while block <= latest {
        let end = block.saturating_add(evm::EVENT_CHUNK_SIZE - 1).min(latest);
        let mut filter = settler.IntentAnnounced_filter();
        filter.filter = filter.filter.topic1(id).from_block(block).to_block(end);
        if let Some((event, _)) = filter
            .query()
            .await
            .map_err(|error| evm::event_query_error(config, block, end, error))?
            .into_iter()
            .next()
        {
            let (order, auth_agent) = decode_event(&event)?;
            return record(&provider, config, order, auth_agent).await;
        }
        if end == latest { break; }
        block = end.saturating_add(1);
    }
    Err(eyre!("intent {id:?} was not found in IntentAnnounced logs"))
}

pub async fn policy(input: PolicyInput) -> Result<PolicyOutput> {
    let config = evm::chain_config(&input.chain_id)?;
    let owner = order_types::parse_address(&input.owner)?;
    let agent = order_types::parse_address(&input.agent)?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    let proxy_address = super::proxy_for(&provider, config, owner).await?;
    let proxy = UserProxyV6::new(proxy_address, provider.clone());
    let policy = proxy.policyOf(agent).call().await?;
    let generation = policy.generation;
    let explicit_tokens = !input.tokens.is_empty();
    let mut tokens = input.tokens.iter().map(|token| order_types::parse_address(token)).collect::<Result<BTreeSet<_>>>()?;
    let mut event_tokens = BTreeSet::new();
    let lookback = evm::event_lookback_blocks(config, input.lookback_blocks);
    let first_block = evm::event_start_block(&provider, lookback).await?;
    let latest = provider.get_block_number().await?;
    let mut block = first_block;
    while block <= latest {
        let end = block.saturating_add(evm::EVENT_CHUNK_SIZE - 1).min(latest);
        let mut event_filter = proxy.AgentCapSet_filter();
        event_filter.filter = event_filter.filter.topic1(agent).from_block(block).to_block(end);
        for (event, _) in event_filter
            .query()
            .await
            .map_err(|error| evm::event_query_error(config, block, end, error))?
        {
            if event.generation == generation {
                event_tokens.insert(event.token);
                tokens.insert(event.token);
            }
        }
        if end == latest { break; }
        block = end.saturating_add(1);
    }
    let mut token_out = Vec::new();
    for token in tokens {
        let info = proxy.agentTokenInfo(agent, token).call().await?;
        token_out.push(TokenPolicy { token: format!("{token:?}"), allowed: info.allowed, cap: info.cap.to_string(), used: info.used.to_string(), epoch_start: info.epochStart.to_string() });
    }
    let note = if event_tokens.is_empty() && !explicit_tokens && generation != 0 && policy.expiry > super::now()? {
        Some(format!("no cap events within {lookback} blocks; pass the token addresses to read (`policy --tokens`, MCP `policy.tokens`)"))
    } else {
        None
    };
    Ok(PolicyOutput { owner: format!("{owner:?}"), agent: format!("{agent:?}"), proxy: format!("{proxy_address:?}"), expiry: policy.expiry.to_string(), epoch_len: policy.epochLen.to_string(), action_mask: policy.actionMask.to_string(), generation: generation.to_string(), tokens: token_out, note })
}

async fn record(provider: &alloy::providers::DynProvider, config: ChainConfig, order: Order, agent: Option<Address>) -> Result<IntentRecord> {
    let view = IntentLensV3::new(config.lens, provider.clone()).preview(order.clone()).call().await?;
    let (status, reason) = status_word(&view, agent.is_none());
    Ok(IntentRecord {
        id: format!("{:?}", view.id),
        placed_by: agent.map(|a| format!("{a:?}")).unwrap_or_else(|| format!("{:?}", order.owner)),
        owner: format!("{:?}", order.owner),
        agent: agent.map(|a| format!("{a:?}")),
        pair: format!("{:?}/{:?}", order.tokenIn, order.tokenOut),
        amount_in: order.amountIn.to_string(),
        start_out: order.startAmountOut.to_string(),
        end_out: order.endAmountOut.to_string(),
        window: format!("{}..{}", order.startTime, order.endTime),
        exclusive_window: view.exclusiveWindow,
        floor_now: view.floorNow.to_string(),
        fee_now: view.feeNow.to_string(),
        required_now: view.requiredNow.to_string(),
        floor_for_outsider: view.floorForOutsider.to_string(),
        required_for_outsider: view.requiredForOutsider.to_string(),
        status,
        reason,
    })
}

fn status_word(view: &IntentLensV3::IntentView, owner_order: bool) -> (String, String) {
    if view.filled { return ("filled".into(), "settler marked filled".into()); }
    if view.cancelled { return ("cancelled".into(), "settler cancellation".into()); }
    if !view.proxyDeployed { return ("dead".into(), "proxy not deployed".into()); }
    if view.killedByOwner { return ("dead".into(), "killed by owner".into()); }
    if owner_order && view.nonceSpent { return ("dead".into(), "owner nonce spent".into()); }
    if !view.inWindow { return ("expired".into(), "intent window closed".into()); }
    if view.exclusiveWindow {
        return (
            "open".into(),
            "exclusive window: only the system solver fills at the floor; an outsider pays the floor plus the exclusivity override".into(),
        );
    }
    ("open".into(), "inside the announced intent window".into())
}

fn decode_event(event: &IntentSettlerV3::IntentAnnounced) -> Result<(Order, Option<Address>)> {
    let order = Order::abi_decode(&event.order)?;
    let outer = IntentAbiCodec::encodeEnvelopeCall::abi_decode_raw(&event.ownerSig)?;
    if outer.kind != 1 { return Ok((order, None)); }
    let inner = IntentAbiCodec::encodeAuthorizationCall::abi_decode_raw(&outer.payload)?;
    Ok((order, Some(inner.agent)))
}

fn parse_b256(value: &str) -> Result<B256> {
    value.parse().map_err(|e| eyre!("invalid bytes32 '{value}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_word_explains_v3_exclusive_window() {
        let view = IntentLensV3::IntentView {
            id: B256::ZERO,
            cancelled: false,
            filled: false,
            nonceSpent: false,
            killedByOwner: false,
            proxyDeployed: true,
            inWindow: true,
            exclusiveWindow: true,
            decayComplete: false,
            floorNow: U256::from(3_000_000_000u64),
            feeNow: U256::from(900_000u64),
            requiredNow: U256::from(3_000_900_000u64),
            floorForOutsider: U256::from(3_007_500_000u64),
            requiredForOutsider: U256::from(3_008_402_250u64),
            ownerBalance: U256::ZERO,
            ownerProxyAllowance: U256::ZERO,
            proxy: Address::ZERO,
            observedAt: U256::ZERO,
            observedBlock: U256::ZERO,
        };
        assert_eq!(
            status_word(&view, false),
            ("open".to_string(), "exclusive window: only the system solver fills at the floor; an outsider pays the floor plus the exclusivity override".to_string())
        );
    }
}
