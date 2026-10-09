// Quote service logic shared by human CLI, MCP tools, and trade orchestration.
// Exports: QuoteInput, QuoteOutput, quote, batch_quote, build_quote_body.
// Deps: crate::{client, redact, service::token, tokens}, serde, eyre.

use crate::client::Client;
use crate::order_types::parse_raw_amount;
use crate::service::token::{self, Token};
use crate::tokens::{chain_name_to_id, format_amount, unknown_chain_id, CHAIN_ID_HELP};
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuoteInput {
    #[schemars(description = CHAIN_ID_HELP)]
    pub chain_id: String,
    pub from: String,
    pub to: String,
    /// Unsigned decimal amount in the input token's smallest unit.
    pub amount: String,
    pub slippage: Option<u16>,
    /// V6 proxy that calls the router; required for BNB Smart Chain quotes.
    #[serde(default)]
    pub taker: Option<String>,
    #[serde(default)]
    pub verify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuoteContext {
    pub chain_id: u64,
    pub token_in: String,
    pub token_in_symbol: String,
    pub token_in_decimals: u8,
    pub token_out: String,
    pub token_out_symbol: String,
    pub token_out_decimals: u8,
    pub amount_in: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuoteOutput {
    pub request: QuoteContext,
    pub response: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BatchQuoteResult {
    pub pair: String,
    pub output: Option<String>,
    pub route: Option<String>,
    pub error: Option<String>,
}

pub fn build_quote_body(
    input: &QuoteInput,
    chain_id: u64,
    from: &Token,
    to: &Token,
) -> (serde_json::Value, QuoteContext) {
    let amount_in = input.amount.clone();
    let mut body = serde_json::json!({
        "chain_id": chain_id,
        "token_in": from.address,
        "token_out": to.address,
        "amount_in": amount_in,
    });
    if let Some(slippage) = input.slippage {
        body["slippage_bps"] = serde_json::json!(slippage);
    }
    if let Some(taker) = &input.taker {
        body["taker"] = serde_json::json!(taker);
    }
    if input.verify {
        body["verify"] = serde_json::json!(true);
    }
    let context = QuoteContext {
        chain_id,
        token_in: from.address.clone(),
        token_in_symbol: from.symbol.clone(),
        token_in_decimals: from.decimals,
        token_out: to.address.clone(),
        token_out_symbol: to.symbol.clone(),
        token_out_decimals: to.decimals,
        amount_in,
    };
    (body, context)
}

/// Validate the amount and chain, resolve both tokens (an address outside the registry is read on
/// chain), then request the quote.
pub async fn quote(client: &Client, input: QuoteInput) -> Result<QuoteOutput> {
    // Quote amounts retain their existing zero-accepted policy.
    parse_raw_amount("quote amount", &input.amount)?;
    let chain_id = chain_name_to_id(&input.chain_id)
        .ok_or_else(|| eyre!("{}", unknown_chain_id(&input.chain_id)))?;
    let from = token::resolve(&input.from, chain_id).await?;
    let to = token::resolve(&input.to, chain_id).await?;
    let (body, request) = build_quote_body(&input, chain_id, &from, &to);
    let response = client.quote(&body).await?;
    Ok(QuoteOutput { request, response })
}

pub async fn batch_quote(
    client: &Client,
    chain_id: &str,
    pairs: &[String],
    amount: &str,
) -> Result<Vec<BatchQuoteResult>> {
    parse_raw_amount("batch quote amount", amount)?;
    let resolved_chain_id =
        chain_name_to_id(chain_id).ok_or_else(|| eyre!("{}", unknown_chain_id(chain_id)))?;
    let mut results = Vec::with_capacity(pairs.len());
    for pair in pairs {
        results.push(batch_one(client, chain_id, resolved_chain_id, pair, amount).await);
    }
    Ok(results)
}

async fn batch_one(
    client: &Client,
    chain_id_input: &str,
    chain_id: u64,
    pair: &str,
    amount: &str,
) -> BatchQuoteResult {
    let Some((from, to)) = pair.split_once('/') else {
        return batch_error(pair, format!("invalid pair format '{pair}', use FROM/TO"));
    };
    let input = QuoteInput {
        chain_id: chain_id_input.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        amount: amount.to_string(),
        slippage: None,
        verify: false, taker: None,
    };
    match quote(client, input).await {
        Ok(out) => format_batch_success(pair, chain_id, out),
        Err(err) => batch_error(pair, crate::redact::urls(&format!("{err}"))),
    }
}

fn format_batch_success(pair: &str, chain_id: u64, out: QuoteOutput) -> BatchQuoteResult {
    let output_raw = out.response["output"].as_str().unwrap_or("0");
    let output = format!(
        "{} {}",
        format_amount(output_raw, out.request.token_out_decimals),
        out.request.token_out_symbol
    );
    let route = out.response["route_path"].as_str().map(String::from);
    let _ = chain_id;
    BatchQuoteResult {
        pair: pair.to_string(),
        output: Some(output),
        route,
        error: None,
    }
}

fn batch_error(pair: &str, error: String) -> BatchQuoteResult {
    BatchQuoteResult {
        pair: pair.to_string(),
        output: None,
        route: None,
        error: Some(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn registry_body(input: &QuoteInput) -> (serde_json::Value, QuoteContext) {
        let chain_id = chain_name_to_id(&input.chain_id).expect("known chain");
        let from = token::from_registry(&input.from, chain_id).expect("registry token in");
        let to = token::from_registry(&input.to, chain_id).expect("registry token out");
        build_quote_body(input, chain_id, &from, &to)
    }

    #[test]
    fn quote_body_preserves_raw_digits_without_float_fields() {
        let input = QuoteInput {
            chain_id: "base".to_string(),
            from: "USDC".to_string(),
            to: "WETH".to_string(),
            amount: "1000000".to_string(),
            slippage: None,
            verify: false, taker: None,
        };
        let (body, context) = registry_body(&input);
        assert_eq!(body["amount_in"], "1000000");
        assert!(body.get("amount_usd").is_none());
        assert_eq!(context.amount_in, "1000000");
    }

    #[test]
    fn numeric_and_alias_chain_ids_resolve_to_the_same_config_and_quote_body() {
        let parse_quote = |chain_id: &str| {
            let cli = crate::cli::Cli::try_parse_from([
                "agentswap", "quote", "--chainid", chain_id, "--from", "USDC", "--to", "WETH",
                "--amount", "1000000", "--slippage", "50", "--verify",
            ])
            .expect("chainid quote arguments");
            let crate::cli::Commands::Quote { chain_id, from, to, amount, slippage, verify } = cli.command else {
                panic!("expected quote command");
            };
            QuoteInput { chain_id, from, to, amount, slippage, verify, taker: None }
        };
        let numeric = parse_quote("8453");
        let alias = parse_quote("base");
        let numeric_config = crate::evm::chain_config(&numeric.chain_id).expect("numeric config");
        let alias_config = crate::evm::chain_config(&alias.chain_id).expect("alias config");
        assert_eq!(numeric_config.id, alias_config.id);
        assert_eq!(numeric_config.rpc, alias_config.rpc);
        assert_eq!(numeric_config.factory, alias_config.factory);
        assert_eq!(numeric_config.settler, alias_config.settler);
        assert_eq!(numeric_config.generation, alias_config.generation);
        assert_eq!(numeric_config.lens, alias_config.lens);
        let numeric_body = registry_body(&numeric);
        let alias_body = registry_body(&alias);
        assert_eq!(numeric_body.0, alias_body.0);
        assert_eq!(numeric_body.1.chain_id, alias_body.1.chain_id);
        assert_eq!(numeric_body.1.token_in, alias_body.1.token_in);
        assert_eq!(numeric_body.1.token_out, alias_body.1.token_out);
        assert_eq!(numeric_body.1.amount_in, alias_body.1.amount_in);
    }

    #[tokio::test]
    async fn unknown_chain_id_fails_before_any_quote_request() {
        let input = QuoteInput {
            chain_id: "not-a-chain".to_string(),
            from: "USDC".to_string(),
            to: "WETH".to_string(),
            amount: "1000000".to_string(),
            slippage: None,
            verify: false, taker: None,
        };
        let error = quote(&Client::new("http://127.0.0.1:1", None), input)
            .await
            .expect_err("unknown chain ID must fail before requesting a quote");
        let message = format!("{error}");
        assert!(message.starts_with("unknown chain id: not-a-chain"));
        assert!(message.contains(crate::tokens::CHAIN_ID_HELP));
    }

    #[test]
    fn one_raw_unit_is_not_scaled_for_six_or_eighteen_decimal_tokens() {
        for (from, expected_decimals) in [("USDC", 6), ("WETH", 18)] {
            let input = QuoteInput {
                chain_id: "base".to_string(),
                from: from.to_string(),
                to: "USDC".to_string(),
                amount: "1".to_string(),
                slippage: None,
                verify: false, taker: None,
            };
            let (body, context) = registry_body(&input);
            assert_eq!(context.token_in_decimals, expected_decimals);
            assert_eq!(body["amount_in"], "1");
        }
    }

}
