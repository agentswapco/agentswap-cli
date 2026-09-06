// RPC-gated parity tests for deployed V6 hashing contracts.
// Exports: deployed_digest_parity.
// Deps: crate::{evm, order_types}, alloy provider and protocol bindings.

use super::{order_id, proxy_domain, signing_hash, IntentAuthorization, IntentSettlerV3, Order, UserProxyFactoryV6, UserProxyV6};
use crate::evm;
use alloy::primitives::{address, Address, B256, U256};

#[tokio::test]
async fn deployed_digest_parity() {
    let Ok(rpc) = std::env::var("AGENTSWAP_INTENT_PARITY_RPC_URL") else {
        eprintln!("deployed_digest_parity skipped: AGENTSWAP_INTENT_PARITY_RPC_URL is not set");
        return;
    };
    let Some(owner_raw) = std::env::var("AGENTSWAP_INTENT_PARITY_OWNER").ok() else {
        eprintln!("deployed_digest_parity skipped: AGENTSWAP_INTENT_PARITY_OWNER is not set");
        return;
    };
    let Some(proxy_raw) = std::env::var("AGENTSWAP_INTENT_PARITY_PROXY").ok() else {
        eprintln!("deployed_digest_parity skipped: AGENTSWAP_INTENT_PARITY_PROXY is not set");
        return;
    };
    let chain_id = std::env::var("AGENTSWAP_INTENT_PARITY_CHAIN_ID")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8453);
    let owner: Address = owner_raw.parse().expect("valid parity owner");
    let proxy: Address = proxy_raw.parse().expect("valid parity proxy");
    let settler = std::env::var("AGENTSWAP_INTENT_PARITY_SETTLER")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(evm::chain_config("base").expect("base config").settler);
    let provider = evm::read_provider(&rpc).expect("parity RPC");
    let factory_proxy = UserProxyFactoryV6::new(
        evm::chain_config("base").expect("base config").factory,
        provider.clone(),
    )
    .proxyOf(owner)
    .call()
    .await
    .expect("factory proxyOf");
    assert_eq!(factory_proxy, proxy, "configured proxy must match factory proxyOf");

    let order = Order {
        owner,
        recipient: owner,
        tokenIn: address!("0x4200000000000000000000000000000000000006"),
        amountIn: U256::from(1_000_000u64),
        tokenOut: address!("0x4200000000000000000000000000000000000007"),
        startAmountOut: U256::from(2_000_000u64),
        endAmountOut: U256::from(1_900_000u64),
        startTime: U256::from(1_800_000_000u64),
        decayEndTime: U256::from(1_800_000_300u64),
        endTime: U256::from(1_800_000_600u64),
        appData: B256::ZERO,
        nonce: U256::from(17u64),
    };
    let local_id = order_id(&order);
    let settler_id = IntentSettlerV3::new(settler, provider.clone())
        .orderHash(order.clone())
        .call()
        .await
        .expect("settler orderHash");
    assert_eq!(local_id, settler_id);

    let auth = IntentAuthorization {
        orderHash: local_id,
        agent: address!("0x1000000000000000000000000000000000000001"),
        generation: 3,
        nonce: U256::from(19u64),
        deadline: 1_800_000_900u64,
    };
    let proxy_contract = UserProxyV6::new(proxy, provider.clone());
    let local_auth_digest = signing_hash(&auth, &proxy_domain(chain_id, proxy));
    let chain_auth_digest = proxy_contract
        .hashIntentAuthorization(auth)
        .call()
        .await
        .expect("proxy hashIntentAuthorization");
    assert_eq!(local_auth_digest, chain_auth_digest);

    let agent_order = UserProxyV6::AgentOrder {
        agent: address!("0x1000000000000000000000000000000000000001"),
        generation: 3,
        router: address!("0x2000000000000000000000000000000000000002"),
        tokenIn: order.tokenIn,
        amountIn: order.amountIn,
        tokenOut: order.tokenOut,
        minOut: order.endAmountOut,
        nonce: U256::from(23u64),
        deadline: U256::from(1_800_000_900u64),
    };
    let local_agent_digest = signing_hash(&agent_order, &proxy_domain(chain_id, proxy));
    let chain_agent_digest = proxy_contract
        .hashAgentOrder(agent_order)
        .call()
        .await
        .expect("proxy hashAgentOrder");
    assert_eq!(local_agent_digest, chain_agent_digest);
}
