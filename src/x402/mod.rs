// x402 payment retry support for paid AgentSwap API calls.
// Exports: Config and payment header construction.
// Deps: crate::signer, serde, reqwest response metadata.

pub mod eip3009;
pub mod select;
pub mod types;

use crate::signer::Signer;
use eyre::Result;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Config {
    pub enabled: bool,
    pub prefer_x402: bool,
    pub chain_id: u64,
    pub max_amount: String,
    pub asset: String,
}

impl Config {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            prefer_x402: false,
            chain_id: 8453,
            max_amount: "0".to_string(),
            asset: "USDC".to_string(),
        }
    }
}

pub async fn payment_header(
    body: &str,
    config: &Config,
    signer: Arc<dyn Signer>,
) -> Result<String> {
    let required = types::PaymentRequired::parse(body)?;
    let accept = select::select_accept(&required.accepts, config)?;
    eip3009::sign_payment(accept, signer, config.chain_id).await
}
