// ERC-20 candidate discovery from registry, explicit addresses and best-effort incoming logs.
// Reuses V6 RPC configuration and event windows; balances always come from balanceOf.
use crate::{evm, order_types, tokens};
use alloy::{primitives::{Address, U256, keccak256}, providers::{DynProvider, Provider}, rpc::types::Filter};
use eyre::{Result, eyre};
use serde::Serialize;
use schemars::JsonSchema;
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

#[derive(Debug, Serialize, JsonSchema)]
pub struct LogScan {
    pub from_block: Option<u64>,
    pub to_block_scanned: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub async fn discover(provider: &DynProvider, config: evm::ChainConfig, owner: Address,
    explicit: &[String], lookback: Option<u64>) -> Result<(Candidates, LogScan, Option<u64>)> {
    let mut out = Candidates::new();
    for token in tokens::registry_addresses(config.id) { add(&mut out, erc20(token)?, "registry"); }
    for token in explicit { add(&mut out, erc20(token)?, "explicit"); }
    let mut scan = LogScan { from_block: None, to_block_scanned: None, error: None };
    let latest = match provider.get_block_number().await {
        Ok(latest) => latest,
        Err(_) => {
            scan.error = Some("Could not read latest block; no Transfer logs scanned".into());
            return Ok((out, scan, None));
        }
    };
    let from = latest.saturating_sub(evm::event_lookback_blocks(config, lookback));
    scan.from_block = Some(from);
    let mut block = from;
    while block <= latest {
        let end = block.saturating_add(evm::EVENT_CHUNK_SIZE - 1).min(latest);
        let filter = Filter::new().event_signature(keccak256("Transfer(address,address,uint256)"))
            .topic2(owner).from_block(block).to_block(end);
        let logs = match provider.get_logs(&filter).await {
            Ok(logs) => logs,
            Err(error) => {
                scan.error = Some(crate::redact::urls(&evm::event_query_error(config, block, end, error).to_string()));
                break;
            }
        };
        for log in logs {
            if log.topics().len() == 3 && log.data().data.len() == 32 { add(&mut out, log.address(), "transfer"); }
        }
        scan.to_block_scanned = Some(end);
        if end == latest { break; }
        block = end.saturating_add(1);
    }
    Ok((out, scan, Some(latest)))
}

pub async fn balance(provider: &DynProvider, token: Address, owner: Address) -> Result<U256> {
    Ok(BalanceReader::new(token, provider.clone()).balanceOf(owner).call().await?)
}
