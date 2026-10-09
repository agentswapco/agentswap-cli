// Token resolution for quote, trade and buy-quota: the built-in registry first, then an address
// read on chain over the chain's RPC, so an address works on every chain with a V6 deployment.
// Exports: Token, from_registry, resolve.
// Deps: crate::{display, evm, order_types::Erc20Metadata, tokens}.

use crate::evm;
use crate::order_types::{self, Erc20Metadata};
use crate::tokens::{chain_id_to_name, resolve_token};
use alloy::primitives::Address;
use alloy::providers::DynProvider;
use eyre::{eyre, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub address: String,
    pub symbol: String,
    /// Used only to render a display value beside a raw amount.
    pub decimals: u8,
}

/// A symbol or address from the built-in registry for `chain_id`.
pub fn from_registry(input: &str, chain_id: u64) -> Option<Token> {
    resolve_token(input, chain_id).map(|(address, symbol, decimals)| Token {
        address: address.to_string(),
        symbol: symbol.to_string(),
        decimals,
    })
}

/// Resolve a registry symbol or a token address. An address outside the registry must answer
/// decimals() on the chain, so a mistyped or non-ERC-20 address fails before any quote or order.
pub async fn resolve(input: &str, chain_id: u64) -> Result<Token> {
    if let Some(token) = from_registry(input, chain_id) {
        return Ok(token);
    }
    if !input.starts_with("0x") && !input.starts_with("0X") {
        return Err(eyre!("unknown token '{input}' on chain id {chain_id}"));
    }
    let address = order_types::parse_address(input)?;
    let config = evm::chain_config(&chain_id.to_string()).map_err(|_| {
        eyre!("token {input} is not in the built-in registry for chain id {chain_id}; an address outside it is accepted only on a chain with a V6 deployment, where it is read on chain")
    })?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    read_metadata(&provider, address, chain_id).await
}

/// decimals() is required. symbol() is text chosen by the token's deployer, so it is shown only
/// when it is short printable ASCII, and always beside the shortened address, so an off-registry
/// token that calls itself USDC never reads like the registry's USDC.
pub(crate) async fn read_metadata(provider: &DynProvider, address: Address, chain_id: u64) -> Result<Token> {
    let token = Erc20Metadata::new(address, provider.clone());
    let decimals = token.decimals().call().await.map_err(|error| {
        eyre!("token {address} does not answer decimals() on {}: {error}", chain_id_to_name(chain_id))
    })?;
    let short = crate::display::short_addr(&address.to_string());
    let symbol = match token.symbol().call().await {
        Ok(symbol) if is_display_symbol(&symbol) => format!("{symbol} ({short})"),
        _ => short,
    };
    Ok(Token { address: address.to_string(), symbol, decimals })
}

fn is_display_symbol(symbol: &str) -> bool {
    !symbol.is_empty() && symbol.len() <= 32 && symbol.chars().all(|c| c.is_ascii_graphic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order_types::Erc20Metadata::{decimalsCall, symbolCall};
    use crate::service::test_rpc::{failure, ok, TestRpc};
    use alloy::sol_types::SolCall;

    fn token_chain(symbol: &'static str, decimals: Option<u8>) -> TestRpc {
        TestRpc::start(move |body| {
            let data = body["params"][0]["input"].as_str().or(body["params"][0]["data"].as_str()).unwrap_or_default();
            let selector = hex::decode(data.trim_start_matches("0x").get(..8).unwrap_or_default()).unwrap_or_default();
            let encoded = if selector == decimalsCall::SELECTOR {
                decimals.map(|value| decimalsCall::abi_encode_returns(&value))
            } else if selector == symbolCall::SELECTOR {
                Some(symbolCall::abi_encode_returns(&symbol.to_string()))
            } else {
                None
            };
            Some(match encoded {
                Some(bytes) => ok(body, serde_json::json!(format!("0x{}", hex::encode(bytes)))),
                None => failure(body, "execution reverted"),
            })
        })
    }

    #[tokio::test]
    async fn an_address_outside_the_registry_is_read_on_chain() {
        let rpc = token_chain("USDT", Some(18));
        let provider = evm::read_provider(&rpc.url).unwrap();
        let address = Address::repeat_byte(0x55);
        let token = read_metadata(&provider, address, 56).await.expect("an ERC-20");
        assert_eq!(token, Token { address: address.to_string(), symbol: "USDT (0x5555...5555)".to_string(), decimals: 18 });
    }

    #[tokio::test]
    async fn an_unprintable_symbol_is_replaced_by_the_short_address() {
        let rpc = token_chain("\u{1b}[31mUSDC", Some(6));
        let provider = evm::read_provider(&rpc.url).unwrap();
        let token = read_metadata(&provider, Address::repeat_byte(0x55), 56).await.expect("an ERC-20");
        assert_eq!(token.symbol, "0x5555...5555");
        assert_eq!(token.decimals, 6);
    }

    #[tokio::test]
    async fn an_address_without_decimals_is_refused_and_the_rpc_path_is_not_quoted() {
        let rpc = token_chain("X", None);
        let provider = evm::read_provider(&rpc.url).unwrap();
        let error = read_metadata(&provider, Address::repeat_byte(0x55), 4663).await.expect_err("not an ERC-20");
        let message = crate::redact::urls(&format!("{error}"));
        assert!(message.contains("does not answer decimals() on Robinhood Chain"), "{message}");
        assert!(!message.contains("SECRET"), "{message}");
    }

    #[tokio::test]
    async fn registry_entries_resolve_without_an_rpc_and_other_inputs_are_refused() {
        let usdc = resolve("usdc", 8453).await.expect("registry symbol");
        assert_eq!((usdc.symbol.as_str(), usdc.decimals), ("USDC", 6));
        let error = resolve("NOT-A-TOKEN", 8453).await.expect_err("neither symbol nor address");
        assert!(format!("{error}").starts_with("unknown token 'NOT-A-TOKEN'"), "{error}");
        let error = resolve("0x5555555555555555555555555555555555555555", 1).await.expect_err("no V6 RPC");
        assert!(format!("{error}").contains("only on a chain with a V6 deployment"), "{error}");
    }
}
