// One Multicall3 read of an agent grant: proxy owner, policy and chain time, then per token the
// budget, decimals, symbol and owner balance. No per-token round trips and no log scans.
use crate::order_types::{Erc20Metadata, UserProxyV6};
use crate::service::portfolio::discovery::BalanceReader;
use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::{DynProvider, MULTICALL3_ADDRESS, Provider, bindings::IMulticall3};
use alloy::rpc::types::{TransactionInput, TransactionRequest};
use alloy::sol_types::SolCall;
use eyre::{Result, ensure, eyre};

/// Calls per token: agentTokenInfo, decimals, symbol, balanceOf.
const PER_TOKEN: usize = 4;

pub(super) struct Budget {
    pub expiry: u64,
    pub generation: u64,
    pub now: u64,
    pub tokens: Vec<TokenState>,
}

pub(super) struct TokenState {
    pub allowed: bool,
    pub cap: U256,
    pub used: U256,
    pub balance: Option<U256>,
    pub decimals: Option<u8>,
    pub symbol: Option<String>,
}

fn call(target: Address, allow_failure: bool, data: Vec<u8>) -> IMulticall3::Call3 {
    IMulticall3::Call3 { target, allowFailure: allow_failure, callData: data.into() }
}

fn calls(proxy: Address, agent: Address, owner: Address, tokens: &[Address]) -> Vec<IMulticall3::Call3> {
    let mut calls = vec![call(proxy, false, UserProxyV6::ownerCall {}.abi_encode()),
        call(proxy, false, UserProxyV6::policyOfCall { agent }.abi_encode()),
        call(MULTICALL3_ADDRESS, false, IMulticall3::getCurrentBlockTimestampCall {}.abi_encode())];
    for &token in tokens {
        calls.push(call(proxy, false, UserProxyV6::agentTokenInfoCall { agent, token }.abi_encode()));
        calls.push(call(token, true, Erc20Metadata::decimalsCall {}.abi_encode()));
        calls.push(call(token, true, Erc20Metadata::symbolCall {}.abi_encode()));
        calls.push(call(token, true, BalanceReader::balanceOfCall { owner }.abi_encode()));
    }
    calls
}

/// Reads the grant `proxy` gives `agent` for `tokens`; refuses a proxy whose owner is not `owner`.
pub(super) async fn read(provider: &DynProvider, proxy: Address, agent: Address, owner: Address, tokens: &[Address]) -> Result<Budget> {
    let data = IMulticall3::aggregate3Call { calls: calls(proxy, agent, owner, tokens) }.abi_encode();
    let request = TransactionRequest::default().to(MULTICALL3_ADDRESS).input(TransactionInput::new(Bytes::from(data)));
    let results = IMulticall3::aggregate3Call::abi_decode_returns(&provider.call(request).await?)?;
    ensure!(results.len() == 3 + PER_TOKEN * tokens.len(), "multicall returned {} results", results.len());
    ensure!(UserProxyV6::ownerCall::abi_decode_returns(&results[0].returnData)? == owner, "proxy owner differs from the request owner");
    let policy = UserProxyV6::policyOfCall::abi_decode_returns(&results[1].returnData)?;
    let now = IMulticall3::getCurrentBlockTimestampCall::abi_decode_returns(&results[2].returnData)?;
    let tokens = results[3..].chunks(PER_TOKEN).map(token).collect::<Result<_>>()?;
    Ok(Budget { expiry: policy.expiry, generation: policy.generation, now: u64::try_from(now).map_err(|_| eyre!("block time overflow"))?, tokens })
}

fn token(results: &[IMulticall3::Result]) -> Result<TokenState> {
    let info = UserProxyV6::agentTokenInfoCall::abi_decode_returns(&results[0].returnData)?;
    let ok = |index: usize| results[index].success.then_some(&results[index].returnData);
    Ok(TokenState { allowed: info.allowed, cap: info.cap, used: info.used,
        decimals: ok(1).and_then(|data| Erc20Metadata::decimalsCall::abi_decode_returns(data).ok()),
        symbol: ok(2).and_then(|data| Erc20Metadata::symbolCall::abi_decode_returns(data).ok()),
        balance: ok(3).and_then(|data| BalanceReader::balanceOfCall::abi_decode_returns(data).ok()) })
}
