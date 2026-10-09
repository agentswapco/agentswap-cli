// RPC/provider helpers for V6 reads and for signing a transaction before it is broadcast.
// Exports: chain_config, rpc_url, read_provider, sign_transaction.
// Deps: alloy providers/network, crate::signer and crate::tokens.

use crate::signer::Signer;
use alloy::consensus::TxEnvelope;
use alloy::network::{EthereumWallet, TxSigner};
use alloy::primitives::{Address, Signature};
use alloy::rpc::types::TransactionRequest;
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

// V6: one address set on Base 8453, Arbitrum 42161, BSC 56, Robinhood 4663, Arc 5042 and Arc Testnet 5042002.
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
        5042 => "https://rpc.mainnet.arc.io",
        5042002 => "https://rpc.testnet.arc.io",
        _ => return Err(eyre!("V6 intent protocol is unavailable on chain id {id}")),
    };
    Ok(ChainConfig { id, rpc, factory: FACTORY, settler: SETTLER, generation: INTENT_GENERATION, lens: LENS })
}

pub fn wrapped_native(chain: u64) -> Option<Address> {
    if chain == 56 {
        return Some(alloy::primitives::address!("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c"));
    }
    crate::tokens::resolve_token("WETH", chain).and_then(|(address, _, _)| address.parse().ok())
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

/// Fill nonce, gas, fees and chain id from the RPC and sign with the signer's wallet, without
/// sending: the hash is known before the broadcast. Returns a provider for the same RPC.
pub async fn sign_transaction(
    url: &str,
    signer: Arc<dyn Signer>,
    request: TransactionRequest,
) -> Result<(DynProvider, TxEnvelope)> {
    let url = url.parse().map_err(|e| eyre!("invalid RPC URL: {e}"))?;
    let wallet = EthereumWallet::new(WalletSigner(signer));
    let provider = ProviderBuilder::new().wallet(wallet).connect_http(url);
    let envelope = provider
        .fill(request)
        .await?
        .try_into_envelope()
        .map_err(|_| eyre!("the transaction was filled but not signed"))?;
    Ok((provider.erased(), envelope))
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

// Shared synthetic grant-parser interoperability fixtures.
#[cfg(test)]
pub const GRANT_FIXTURES: [&str; 2] = [
    "https://app.agentswap.co/grant?v=1&chain=8453&agent=0x2000000000000000000000000000000000000002&label=dust+sweep&t=0x1000000000000000000000000000000000000001%3A1.2345&t=0x1000000000000000000000000000000000000002%3A0.000000000000000005&t=0x3000000000000000000000000000000000000003%3A0&epoch=1w&expiry=2026-10-10T12%3A00%3A00Z&actions=market%2Cintent&owner=0x4000000000000000000000000000000000000004",
    "https://app.agentswap.co/grant?v=1&chain=42161&agent=0x2000000000000000000000000000000000000002&t=0x1000000000000000000000000000000000000001%3A250&t=0x1000000000000000000000000000000000000002%3A0.5&t=0x3000000000000000000000000000000000000003%3A0&epoch=1w&expiry=30d&actions=market%2Cintent",
];

#[cfg(test)]
pub const GRANT_TOKENS: [&str; 3] = [
    "0x1000000000000000000000000000000000000001",
    "0x1000000000000000000000000000000000000002",
    "0x3000000000000000000000000000000000000003",
];
#[cfg(test)]
pub const GRANT_AGENT: &str = "0x2000000000000000000000000000000000000002";
#[cfg(test)]
pub const GRANT_OWNER: &str = "0x4000000000000000000000000000000000000004";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_rpc_uses_the_shared_v6_contracts() {
        for alias in ["arc", "arc-mainnet", "5042"] {
            let arc = chain_config(alias).expect("Arc config");
            assert_eq!(arc.id, 5042);
            assert_eq!(arc.rpc, "https://rpc.mainnet.arc.io");
            for chain in ["base", "arbitrum", "bsc", "robinhood", "arc-testnet"] {
                let existing = chain_config(chain).unwrap();
                assert_eq!(arc.factory, existing.factory);
                assert_eq!(arc.settler, existing.settler);
                assert_eq!(arc.lens, existing.lens);
                assert_eq!(arc.generation, existing.generation);
            }
            assert_eq!(event_lookback_blocks(arc, None), 200_000);
            assert_eq!(event_lookback_blocks(arc, Some(300_000)), 300_000);
        }
    }

    #[test]
    fn arc_testnet_rpc_uses_the_shared_v6_contracts() {
        for alias in ["arc-testnet", "arct", "5042002"] {
            let arc = chain_config(alias).expect("Arc Testnet config");
            assert_eq!(arc.id, 5042002);
            assert_eq!(arc.rpc, "https://rpc.testnet.arc.io");
            for chain in ["base", "arbitrum", "bsc", "robinhood"] {
                let existing = chain_config(chain).unwrap();
                assert_eq!(arc.factory, existing.factory);
                assert_eq!(arc.settler, existing.settler);
                assert_eq!(arc.lens, existing.lens);
                assert_eq!(arc.generation, existing.generation);
            }
            assert_eq!(event_lookback_blocks(arc, None), 200_000);
            assert_eq!(event_lookback_blocks(arc, Some(300_000)), 300_000);
        }
    }

    #[test]
    fn resolved_chain_without_v6_deployment_reports_unavailability() {
        let error = chain_config("1").expect_err("Ethereum resolves but has no V6 deployment");
        assert_eq!(format!("{error}"), "V6 intent protocol is unavailable on chain id 1");
    }
}
