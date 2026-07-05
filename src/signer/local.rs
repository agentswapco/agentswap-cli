// Local private-key signer backend for agent orders and x402 payments.
// Exports: LocalKey.
// Deps: alloy::signers::local, zeroize, std fs.

use crate::signer::Signer;
use alloy::primitives::{Address, B256, Signature};
use alloy::signers::{local::PrivateKeySigner, SignerSync};
use async_trait::async_trait;
use eyre::{eyre, Result};
use zeroize::Zeroizing;

pub struct LocalKey {
    inner: PrivateKeySigner,
}

impl LocalKey {
    pub fn from_key_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| eyre!("failed to read key file '{path}': {e}"))?;
        let key = Zeroizing::new(content.trim().to_string());
        if key.is_empty() {
            return Err(eyre!("key file '{path}' is empty"));
        }
        Self::from_private_key(&key)
    }

    pub fn from_private_key(private_key: &str) -> Result<Self> {
        let inner: PrivateKeySigner = private_key
            .parse()
            .map_err(|e| eyre!("invalid private key: {e}"))?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl Signer for LocalKey {
    fn address(&self) -> Address {
        self.inner.address()
    }

    async fn sign_message(&self, message: &[u8]) -> Result<Signature> {
        self.inner
            .sign_message_sync(message)
            .map_err(|e| eyre!("signing failed: {e}"))
    }

    async fn sign_hash(&self, digest: B256) -> Result<Signature> {
        self.inner
            .sign_hash_sync(&digest)
            .map_err(|e| eyre!("signing failed: {e}"))
    }
}
