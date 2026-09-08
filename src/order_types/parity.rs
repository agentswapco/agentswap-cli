// Hashing parity tests for V6 contracts: deployed RPC checks and offline golden vectors.
// Exports: deployed_digest_parity, offline golden digest tests.
// Deps: crate::{evm, order_types}, alloy provider and protocol bindings.

use super::{authorization_envelope, order_id, proxy_domain, signing_hash, IntentAuthorization, IntentSettlerV3, Order, UserProxyFactoryV6, UserProxyV6};
use crate::evm;
use alloy::primitives::{address, b256, Address, B256, Bytes, U256};

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

#[test]
fn offline_intent_authorization_digest_matches_golden_vector() {
    // Golden vector computed by running UserProxyV6.hashIntentAuthorization in
    // agentswap-protocol test/V6TestBase.sol fixture for chainId 8453, proxy
    // 0xD0Cbf7f4884dc454bf58A8F27972f385176491A9, and the shared agent authorization inputs.
    let chain_id = 8453u64;
    let proxy = address!("0xD0Cbf7f4884dc454bf58A8F27972f385176491A9");
    let domain = proxy_domain(chain_id, proxy);
    assert_eq!(domain.name.as_deref(), Some("AgentSwap UserProxy"));
    assert_eq!(domain.version.as_deref(), Some("5"));

    let auth = IntentAuthorization {
        orderHash: b256!("da2743275525fadf0df5710b30e89e7f60af2257c0dedd306deaeb5255839d0f"),
        agent: address!("0x3507A251bbd388eb31C630627E2DdFE10Eb5aD6F"),
        generation: 1,
        nonce: U256::from(102u64),
        deadline: 1_086_400u64,
    };

    let digest = signing_hash(&auth, &domain);
    let expected = b256!("4f1ae85bfbb66fe338d82bef0563e74e92f0b3f3457db9c3c8bec9153349be70");
    assert_eq!(digest, expected);
}

#[test]
fn offline_authorization_envelope_matches_protocol_golden_vector() {
    // Reuses the protocol golden agent envelope vector from the relay's
    // tests/unit/intent-auth-envelope.spec.ts, pinning the exact byte encoding across repos.
    let auth = IntentAuthorization {
        orderHash: b256!("da2743275525fadf0df5710b30e89e7f60af2257c0dedd306deaeb5255839d0f"),
        agent: address!("0x3507A251bbd388eb31C630627E2DdFE10Eb5aD6F"),
        generation: 1,
        nonce: U256::from(102u64),
        deadline: 1_086_400u64,
    };
    let sig_bytes = hex::decode(
        "07dc47e1f152e8ac40c03efb440105adfb36428232eaa8a976cf4f57ed2c4eb9004fbb0f442fc8d534f809a485d164dc50fbe4bbe0dc9a8a7d1aa02c657aa7a81c"
    ).expect("valid golden signature hex");
    let envelope = authorization_envelope(&auth, &Bytes::from(sig_bytes));

    let expected_hex = concat!(
        "0000000000000000000000000000000000000000000000000000000000000001",
        "0000000000000000000000000000000000000000000000000000000000000040",
        "0000000000000000000000000000000000000000000000000000000000000140",
        "da2743275525fadf0df5710b30e89e7f60af2257c0dedd306deaeb5255839d0f",
        "0000000000000000000000003507a251bbd388eb31c630627e2ddfe10eb5ad6f",
        "0000000000000000000000000000000000000000000000000000000000000001",
        "0000000000000000000000000000000000000000000000000000000000000066",
        "00000000000000000000000000000000000000000000000000000000001093c0",
        "00000000000000000000000000000000000000000000000000000000000000c0",
        "0000000000000000000000000000000000000000000000000000000000000041",
        "07dc47e1f152e8ac40c03efb440105adfb36428232eaa8a976cf4f57ed2c4eb9",
        "004fbb0f442fc8d534f809a485d164dc50fbe4bbe0dc9a8a7d1aa02c657aa7a8",
        "1c00000000000000000000000000000000000000000000000000000000000000"
    );
    assert_eq!(hex::encode(&envelope), expected_hex);
}
