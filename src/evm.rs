// RPC/provider helpers for V6 reads and signed transaction submission.
// Exports: chain_config, rpc_url, read_provider, wallet_provider.
// Deps: alloy providers/network, crate::signer and crate::tokens.

use crate::signer::Signer;
use alloy::network::{EthereumWallet, TxSigner};
use alloy::primitives::{Address, Signature};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::signers::Error as SignerError;
use eyre::{eyre, Result};
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub struct ChainConfig {
    pub id: u64,
    pub rpc: &'static str,
    pub factory: Address,
    pub settler: Address,
    pub generation: &'static str,
    pub lens: Address,
}

// V6: one address set on Base 8453, Arbitrum 42161, BSC 56 and Robinhood 4663.
const FACTORY: Address = alloy::primitives::address!("0xc1660e4BbC825f8367dA92b60dccc17E4E10bc26");
const SETTLER: Address = alloy::primitives::address!("0x2dd81c4fD1FC38b009Ab10D5C9b1f01Ca51cE462");
const INTENT_GENERATION: &str = "v6";
const LENS: Address = alloy::primitives::address!("0x3AFfAafAF3Ec0A8A535723CBf0891482680F30C3");
const EVENT_LOOKBACK_BLOCKS: u64 = 200_000;
const BSC_DEFAULT_EVENT_LOOKBACK_BLOCKS: u64 = 9_000;
pub const EVENT_CHUNK_SIZE: u64 = 5_000;

pub fn chain_config(chain_id: &str) -> Result<ChainConfig> {
    let id = crate::tokens::chain_name_to_id(chain_id)
        .ok_or_else(|| eyre!("{}", crate::tokens::unknown_chain_id(chain_id)))?;
    let rpc = match id {
        8453 => "https://mainnet.base.org",
        42161 => "https://arb1.arbitrum.io/rpc",
        56 => "https://bsc-rpc.publicnode.com",
        4663 => "https://rpc.mainnet.chain.robinhood.com",
        _ => return Err(eyre!("V6 intent protocol is unavailable on chain id {id}")),
    };
    Ok(ChainConfig { id, rpc, factory: FACTORY, settler: SETTLER, generation: INTENT_GENERATION, lens: LENS })
}

pub fn rpc_url(config: ChainConfig) -> String {
    let key = format!("AGENTSWAP_RPC_URL_{}", config.id);
    std::env::var(key)
        .or_else(|_| std::env::var("AGENTSWAP_RPC_URL"))
        .unwrap_or_else(|_| config.rpc.to_string())
}

pub fn read_provider(url: &str) -> Result<DynProvider> {
    let url = url.parse().map_err(|e| eyre!("invalid RPC URL: {e}"))?;
    Ok(ProviderBuilder::new().connect_http(url).erased())
}

pub fn event_lookback_blocks(config: ChainConfig, override_blocks: Option<u64>) -> u64 {
    if let Some(blocks) = override_blocks {
        return blocks;
    }
    if config.id == 56 && !has_rpc_override(config.id) {
        return BSC_DEFAULT_EVENT_LOOKBACK_BLOCKS;
    }
    EVENT_LOOKBACK_BLOCKS
}

pub async fn event_start_block(provider: &DynProvider, lookback_blocks: u64) -> Result<u64> {
    let latest = provider.get_block_number().await?;
    Ok(latest.saturating_sub(lookback_blocks))
}

pub fn event_query_error(
    config: ChainConfig,
    from_block: u64,
    to_block: u64,
    error: impl std::fmt::Display,
) -> eyre::Report {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    if lower.contains("limit exceeded") || lower.contains("archive") {
        return eyre!(
            "eth_getLogs refused on {} for block span {}..={}; set AGENTSWAP_RPC_URL_{} (or AGENTSWAP_RPC_URL) to a keyed RPC endpoint",
            crate::tokens::chain_id_to_name(config.id),
            from_block,
            to_block,
            config.id
        );
    }
    eyre!("{message}")
}

fn has_rpc_override(chain_id: u64) -> bool {
    let chain_key = format!("AGENTSWAP_RPC_URL_{chain_id}");
    std::env::var_os(chain_key).is_some() || std::env::var_os("AGENTSWAP_RPC_URL").is_some()
}

pub fn wallet_provider(url: &str, signer: Arc<dyn Signer>) -> Result<DynProvider> {
    let url = url.parse().map_err(|e| eyre!("invalid RPC URL: {e}"))?;
    let wallet = EthereumWallet::new(WalletSigner(signer));
    Ok(ProviderBuilder::new().wallet(wallet).connect_http(url).erased())
}

#[derive(Clone)]
struct WalletSigner(Arc<dyn Signer>);

impl std::fmt::Debug for WalletSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("WalletSigner").field(&self.0.address()).finish()
    }
}

#[async_trait::async_trait]
impl TxSigner<Signature> for WalletSigner {
    fn address(&self) -> Address {
        self.0.address()
    }

    async fn sign_transaction(
        &self,
        tx: &mut dyn alloy::consensus::SignableTransaction<Signature>,
    ) -> alloy::signers::Result<Signature> {
        self.0
            .sign_hash(tx.signature_hash())
            .await
            .map_err(SignerError::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_chain_without_v6_deployment_reports_unavailability() {
        let error = chain_config("1").expect_err("Ethereum resolves but has no V6 deployment");
        assert_eq!(format!("{error}"), "V6 intent protocol is unavailable on chain id 1");
    }
}
