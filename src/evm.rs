// RPC/provider helpers for V5 reads and signed transaction submission.
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
    pub lens: Address,
}

const FACTORY: Address = alloy::primitives::address!("0x1b1086b82b7a3935cc4158ab289a7331a2dc63c6");
const SETTLER: Address = alloy::primitives::address!("0xc0b66ee5345170dfd4855cfda917bf316d8058e2");
const LENS: Address = alloy::primitives::address!("0x805fe607e265477227ab5492963f6f0bef1f3860");
const EVENT_LOOKBACK_BLOCKS: u64 = 200_000;

pub fn chain_config(chain: &str) -> Result<ChainConfig> {
    let id = crate::tokens::chain_name_to_id(chain)
        .ok_or_else(|| eyre!("unknown chain: {chain}"))?;
    let rpc = match id {
        8453 => "https://mainnet.base.org",
        42161 => "https://arb1.arbitrum.io/rpc",
        56 => "https://bsc-dataseed.binance.org",
        4663 => "https://rpc.mainnet.chain.robinhood.com",
        _ => return Err(eyre!("the earlier contract set intent protocol is unavailable on chain {id}")),
    };
    Ok(ChainConfig { id, rpc, factory: FACTORY, settler: SETTLER, lens: LENS })
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

pub async fn event_start_block(provider: &DynProvider, _address: Address) -> Result<u64> {
    let latest = provider.get_block_number().await?;
    Ok(latest.saturating_sub(EVENT_LOOKBACK_BLOCKS))
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
