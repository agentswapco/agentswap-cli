// Signing abstraction for local and future remote agent signers.
// Exports: Signer trait and LocalKey backend.
// Deps: alloy signers, async-trait.

pub mod local;
pub mod remote;

use alloy::primitives::{Address, B256, Signature};
use async_trait::async_trait;
use eyre::Result;

#[async_trait]
pub trait Signer: Send + Sync {
    fn address(&self) -> Address;

    #[allow(dead_code)]
    async fn sign_message(&self, message: &[u8]) -> Result<Signature>;
    async fn sign_hash(&self, digest: B256) -> Result<Signature>;
    async fn sign_typed_hash(&self, digest: B256) -> Result<Signature> {
        self.sign_hash(digest).await
    }
}
