// ERC-20 candidate discovery from explicit addresses and a fallback registry.
// Reuses V6 RPC configuration; non-indexed balances come from balanceOf.
use crate::{evm, order_types, tokens};
use alloy::{primitives::{Address, U256}, providers::DynProvider};
use eyre::{Result, eyre};
use std::collections::{BTreeMap, BTreeSet};

alloy::sol! {
    #[sol(rpc)]
    contract BalanceReader {
        function balanceOf(address owner) external view returns (uint256);
    }
}

pub type Candidates = BTreeMap<Address, BTreeSet<String>>;

pub fn config(chain: &str) -> Result<evm::ChainConfig> {
    let config = evm::chain_config(chain)?;
    if !matches!(config.id, 8453 | 42161 | 56 | 4663) { return Err(eyre!("{}", tokens::HOLDINGS_CHAINS_NOTE)); }
    Ok(config)
}

pub fn erc20(value: &str) -> Result<Address> {
    let address = order_types::parse_address(value)?;
    if address == Address::ZERO || address == Address::repeat_byte(0xee) {
        return Err(eyre!("native tokens are not ERC-20 tokens"));
    }
    Ok(address)
}

pub fn add(out: &mut Candidates, address: Address, source: &str) {
    if erc20(&format!("{address:?}")).is_ok() {
        out.entry(address).or_default().insert(source.into());
    }
}

pub fn discover(config: evm::ChainConfig, explicit: &[String], indexed: bool) -> Result<Candidates> {
    let mut out = Candidates::new();
    if !indexed {
        for token in tokens::registry_addresses(config.id) { add(&mut out, erc20(token)?, "registry"); }
    }
    for token in explicit { add(&mut out, erc20(token)?, "explicit"); }
    Ok(out)
}

pub async fn balance(provider: &DynProvider, token: Address, owner: Address) -> Result<U256> {
    Ok(BalanceReader::new(token, provider.clone()).balanceOf(owner).call().await?)
}
