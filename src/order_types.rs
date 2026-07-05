// Canonical AgentSwap order ABI and digest helpers.
// Exports: UserProxyV3/IntentFactory sol structs plus signing_hash/order_domain.
// Deps: alloy sol-types; vendored from agentswap-gateway src/eth/{abi,sig}.rs.

use alloy::primitives::{Address, B256, U256};
use alloy::sol;
use alloy::sol_types::{eip712_domain, Eip712Domain, SolStruct};
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};

sol! {
    #[sol(rpc)]
    #[derive(Debug, Serialize, Deserialize)]
    contract UserProxyV3 {
        struct AgentOrder {
            address agent;
            address router;
            address tokenIn;
            uint256 amountIn;
            address tokenOut;
            uint256 minOut;
            uint256 nonce;
            uint256 deadline;
        }

        function executeAsAgent(AgentOrder o, bytes agentSig, address spender, bytes routerData)
            external returns (uint256 amountOut);
        function hashAgentOrder(AgentOrder o) external view returns (bytes32);
    }

    #[sol(rpc)]
    #[derive(Debug, Serialize, Deserialize)]
    contract IntentFactory {
        struct IntentOrder {
            address owner;
            address tokenIn;
            uint256 amountIn;
            address tokenOut;
            uint256 startAmountOut;
            uint256 endAmountOut;
            uint256 startTime;
            uint256 endTime;
            uint256 nonce;
        }

        function createIntent(IntentOrder o, bytes permit2Sig) external returns (address clone);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrderDto {
    pub agent: String,
    pub router: String,
    pub token_in: String,
    pub amount_in: String,
    pub token_out: String,
    pub min_out: String,
    pub nonce: String,
    pub deadline: String,
}

pub fn order_domain(chain_id: u64, proxy: Address) -> Eip712Domain {
    eip712_domain! {
        name: "AgentSwap UserProxy",
        version: "2",
        chain_id: chain_id,
        verifying_contract: proxy,
    }
}

pub fn signing_hash<S: SolStruct>(order: &S, chain_id: u64, proxy: Address) -> B256 {
    order.eip712_signing_hash(&order_domain(chain_id, proxy))
}

pub fn dto_from_agent_order(order: &UserProxyV3::AgentOrder) -> AgentOrderDto {
    AgentOrderDto {
        agent: format!("{:?}", order.agent),
        router: format!("{:?}", order.router),
        token_in: format!("{:?}", order.tokenIn),
        amount_in: order.amountIn.to_string(),
        token_out: format!("{:?}", order.tokenOut),
        min_out: order.minOut.to_string(),
        nonce: order.nonce.to_string(),
        deadline: order.deadline.to_string(),
    }
}

pub fn parse_address(value: &str) -> Result<Address> {
    value.parse().map_err(|e| eyre!("invalid address '{value}': {e}"))
}

pub fn parse_u256(value: &str) -> Result<U256> {
    U256::from_str_radix(value, 10).map_err(|e| eyre!("invalid uint '{value}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_order_digest_matches_gateway_vector() {
        let order = UserProxyV3::AgentOrder {
            agent: parse_address("0x1000000000000000000000000000000000000001").expect("agent"),
            router: parse_address("0x2000000000000000000000000000000000000002").expect("router"),
            tokenIn: parse_address("0x3000000000000000000000000000000000000003").expect("token in"),
            amountIn: U256::from(100_000_000u64),
            tokenOut: parse_address("0x4000000000000000000000000000000000000004").expect("token out"),
            minOut: U256::from(50_000_000_000_000_000u64),
            nonce: U256::from(7u64),
            deadline: U256::from(4_000_000_000u64),
        };
        let proxy = parse_address("0x5000000000000000000000000000000000000005").expect("proxy");
        let digest = signing_hash(&order, 8453, proxy);
        assert_eq!(
            format!("{digest:?}"),
            "0x8b4b0de0da2c9458592233938fc59e8e0ab2a3d63378f34956a363c12b31ae59"
        );
    }
}
