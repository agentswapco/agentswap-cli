// Canonical V6 protocol ABI types and EIP-712/hash helpers.
// Exports: Order, IntentAuthorization, UserProxyV6, IntentSettlerV3, IntentLensV3.
// Deps: alloy sol-types, serde, crate::signer-independent primitives.

use alloy::primitives::{keccak256, Address, B256, Bytes, U256};
use alloy::sol;
use alloy::sol_types::{eip712_domain, Eip712Domain, SolCall, SolStruct, SolType};
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};

mod errors;
pub use errors::UserProxyV6Errors;

sol! {
    #[derive(Debug)]
    struct Order {
        address owner;
        address recipient;
        address tokenIn;
        uint256 amountIn;
        address tokenOut;
        uint256 startAmountOut;
        uint256 endAmountOut;
        uint256 startTime;
        uint256 decayEndTime;
        uint256 endTime;
        bytes32 appData;
        uint256 nonce;
    }

    #[derive(Debug)]
    struct IntentAuthorization {
        bytes32 orderHash;
        address agent;
        uint64 generation;
        uint256 nonce;
        uint64 deadline;
    }

    #[sol(rpc)]
    #[derive(Debug)]
    contract UserProxyV6 {
        struct AgentOrder {
            address agent;
            uint64 generation;
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
        function isIntentAuthorized(Order o, bytes auth) external view returns (bool);
        function hashIntentAuthorization(IntentAuthorization auth) external view returns (bytes32);
        function isAgentNonceUsed(address agent, uint256 nonce) external view returns (bool);
        function policyOf(address agent) external view returns (uint64 expiry, uint32 epochLen, uint8 actionMask, uint64 generation);
        function agentTokenInfo(address agent, address token)
            external view returns (bool allowed, uint256 cap, uint256 used, uint64 epochStart);
        function owner() external view returns (address);

        event AgentCapSet(address indexed agent, uint64 generation, address indexed token, uint256 cap);
    }

    #[sol(rpc)]
    contract Erc20Metadata {
        function decimals() external view returns (uint8);
    }

    #[sol(rpc)]
    contract IntentAbiCodec {
        function encodeEnvelope(uint8 kind, bytes payload) external;
        function encodeAuthorization(
            bytes32 orderHash, address agent, uint64 generation, uint256 nonce,
            uint64 deadline, bytes agentSig
        ) external;
    }

    #[sol(rpc)]
    #[derive(Debug)]
    contract UserProxyFactoryV6 {
        function proxyOf(address user) external view returns (address);
    }

    #[sol(rpc)]
    #[derive(Debug)]
    contract IntentSettlerV3 {
        function announce(Order o, bytes auth) external;
        function orderHash(Order o) external pure returns (bytes32);
        function cancelled(bytes32 id) external view returns (bool);
        function filled(bytes32 id) external view returns (bool);

        event IntentAnnounced(
            bytes32 indexed id,
            address indexed owner,
            bytes32 indexed appData,
            bytes order,
            bytes ownerSig
        );
        event IntentFilled(
            bytes32 indexed id,
            address indexed owner,
            address indexed solver,
            address recipient,
            address caller,
            uint256 amountIn,
            uint256 requiredOut,
            uint256 fee,
            uint256 receivedOut,
            uint256 aboveFloor
        );
        event IntentCancelled(bytes32 indexed id, address indexed owner, address indexed caller);
    }

    #[sol(rpc)]
    #[derive(Debug)]
    contract IntentLensV3 {
        struct IntentView {
            bytes32 id;
            bool cancelled;
            bool filled;
            bool nonceSpent;
            bool killedByOwner;
            bool proxyDeployed;
            bool inWindow;
            bool exclusiveWindow;
            bool decayComplete;
            uint256 floorNow;
            uint256 feeNow;
            uint256 requiredNow;
            uint256 floorForOutsider;
            uint256 requiredForOutsider;
            uint256 ownerBalance;
            uint256 ownerProxyAllowance;
            address proxy;
            uint256 observedAt;
            uint256 observedBlock;
        }

        function PREVIEW_LAYOUT() external view returns (uint256);
        function preview(Order o) external view returns (IntentView);
        function previewMany(Order[] o) external view returns (IntentView[]);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrderDto {
    pub agent: String,
    pub generation: String,
    pub router: String,
    pub token_in: String,
    pub amount_in: String,
    pub token_out: String,
    pub min_out: String,
    pub nonce: String,
    pub deadline: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrderDto {
    pub owner: String,
    pub recipient: String,
    pub token_in: String,
    pub amount_in: String,
    pub token_out: String,
    pub start_amount_out: String,
    pub end_amount_out: String,
    pub start_time: String,
    pub decay_end_time: String,
    pub end_time: String,
    pub app_data: String,
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntentAuthorizationDto {
    pub order_hash: String,
    pub agent: String,
    pub generation: String,
    pub nonce: String,
    pub deadline: String,
}

pub fn proxy_domain(chain_id: u64, proxy: Address) -> Eip712Domain {
    eip712_domain! {
        name: "AgentSwap UserProxy",
        version: "5",
        chain_id: chain_id,
        verifying_contract: proxy,
    }
}

pub fn signing_hash<S: SolStruct>(value: &S, domain: &Eip712Domain) -> B256 {
    value.eip712_signing_hash(domain)
}

pub fn order_id(order: &Order) -> B256 {
    keccak256(<Order as SolType>::abi_encode(order))
}

pub fn authorization_envelope(auth: &IntentAuthorization, sig: &Bytes) -> Bytes {
    let payload_call = IntentAbiCodec::encodeAuthorizationCall {
        orderHash: auth.orderHash, agent: auth.agent, generation: auth.generation,
        nonce: auth.nonce, deadline: auth.deadline, agentSig: sig.clone(),
    };
    let payload = payload_call.abi_encode();
    IntentAbiCodec::encodeEnvelopeCall {
        kind: 1, payload: payload[4..].to_vec().into(),
    }.abi_encode()[4..].to_vec().into()
}

pub fn dto_from_agent_order(order: &UserProxyV6::AgentOrder) -> AgentOrderDto {
    AgentOrderDto {
        agent: format!("{:?}", order.agent),
        generation: order.generation.to_string(),
        router: format!("{:?}", order.router),
        token_in: format!("{:?}", order.tokenIn),
        amount_in: order.amountIn.to_string(),
        token_out: format!("{:?}", order.tokenOut),
        min_out: order.minOut.to_string(),
        nonce: order.nonce.to_string(),
        deadline: order.deadline.to_string(),
    }
}

pub fn dto_from_order(order: &Order) -> OrderDto {
    OrderDto {
        owner: format!("{:?}", order.owner),
        recipient: format!("{:?}", order.recipient),
        token_in: format!("{:?}", order.tokenIn),
        amount_in: order.amountIn.to_string(),
        token_out: format!("{:?}", order.tokenOut),
        start_amount_out: order.startAmountOut.to_string(),
        end_amount_out: order.endAmountOut.to_string(),
        start_time: order.startTime.to_string(),
        decay_end_time: order.decayEndTime.to_string(),
        end_time: order.endTime.to_string(),
        app_data: format!("{:?}", order.appData),
        nonce: order.nonce.to_string(),
    }
}

pub fn dto_from_authorization(auth: &IntentAuthorization) -> IntentAuthorizationDto {
    IntentAuthorizationDto {
        order_hash: format!("{:?}", auth.orderHash),
        agent: format!("{:?}", auth.agent),
        generation: auth.generation.to_string(),
        nonce: auth.nonce.to_string(),
        deadline: auth.deadline.to_string(),
    }
}

pub fn parse_address(value: &str) -> Result<Address> {
    value.parse().map_err(|e| eyre!("invalid address '{value}': {e}"))
}

pub fn parse_u256(value: &str) -> Result<U256> {
    parse_raw_amount("uint", value)
}

pub fn parse_raw_amount(field: &str, value: &str) -> Result<U256> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(eyre!(
            "invalid {field} '{value}': expected unsigned decimal digits"
        ));
    }
    U256::from_str_radix(value, 10)
        .map_err(|_| eyre!("invalid {field} '{value}': integer overflow"))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod parity;
