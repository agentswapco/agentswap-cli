// Local private-key signer backend for agent orders and x402 payments.
// Exports: LocalKey.
// Deps: alloy::signers::local, zeroize, hex, std fs/io.

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
        use std::io::Read;
        let mut content = Zeroizing::new(String::new());
        std::fs::File::open(path)
            .and_then(|mut file| file.read_to_string(&mut content))
            .map_err(|e| eyre!("failed to read key file '{path}': {e}"))?;
        let key = content.trim();
        if key.is_empty() {
            return Err(eyre!("key file '{path}' is empty"));
        }
        Self::from_private_key(key)
    }

    pub fn from_private_key(private_key: &str) -> Result<Self> {
        let mut bytes = Zeroizing::new([0u8; 32]);
        let key = private_key.strip_prefix("0x")
            .or_else(|| private_key.strip_prefix("0X")).unwrap_or(private_key);
        hex::decode_to_slice(key, &mut *bytes)
            .map_err(|e| eyre!("invalid private key: {e}"))?;
        let inner = PrivateKeySigner::from_slice(&*bytes)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn key_file_trimming_signing_and_rejection() {
        let path = std::env::temp_dir().join(format!("agentswap-key-load-{}", std::process::id()));
        let path_str = path.to_str().unwrap();
        let canary = "01".repeat(32);
        std::fs::write(&path, format!(" \n0x{canary}\t\n")).unwrap();
        let loaded = LocalKey::from_key_file(path_str).unwrap();
        let expected = LocalKey::from_private_key(&canary).unwrap();
        assert_eq!(LocalKey::from_private_key(&format!("0X{canary}")).unwrap().address(), expected.address());
        assert_eq!(loaded.address(), expected.address());
        assert_eq!(loaded.sign_message(b"fixture").await.unwrap(), expected.sign_message(b"fixture").await.unwrap());
        assert_eq!(loaded.sign_hash(B256::ZERO).await.unwrap(), expected.sign_hash(B256::ZERO).await.unwrap());
        for invalid in ["  \n", "invalid", "00", &"00".repeat(32)] {
            std::fs::write(&path, invalid).unwrap();
            assert!(LocalKey::from_key_file(path_str).is_err());
        }
        std::fs::remove_file(&path).unwrap();
        assert!(LocalKey::from_key_file(path_str).is_err());
    }
}
