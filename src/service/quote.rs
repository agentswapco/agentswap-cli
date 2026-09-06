// Quote service logic shared by human CLI, MCP tools, and trade orchestration.
// Exports: QuoteInput, QuoteOutput, quote, batch_quote, build_quote_body.
// Deps: crate::{client, tokens}, serde, eyre.

use crate::client::Client;
use crate::order_types::parse_raw_amount;
use crate::tokens::{chain_name_to_id, format_amount, resolve_token};
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuoteInput {
    pub chain: String,
    pub from: String,
    pub to: String,
    /// Unsigned decimal amount in the input token's smallest unit.
    pub amount: String,
    pub slippage: Option<u16>,
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

pub fn build_quote_body(input: &QuoteInput) -> Result<(serde_json::Value, QuoteContext)> {
    // Quote amounts retain their existing zero-accepted policy.
    parse_raw_amount("quote amount", &input.amount)?;
    let chain_id = chain_name_to_id(&input.chain).ok_or_else(|| {
        eyre!(
            "unknown chain: {}. Use: ethereum, base, arbitrum",
            input.chain
        )
    })?;
    let (from_addr, from_sym, from_dec) = resolve_token(&input.from, chain_id)
        .ok_or_else(|| eyre!("unknown token '{}' on chain {}", input.from, input.chain))?;
    let (to_addr, to_sym, to_dec) = resolve_token(&input.to, chain_id)
        .ok_or_else(|| eyre!("unknown token '{}' on chain {}", input.to, input.chain))?;
    let amount_in = input.amount.clone();
    let mut body = serde_json::json!({
        "chain_id": chain_id,
        "token_in": from_addr,
        "token_out": to_addr,
        "amount_in": amount_in,
    });
    if let Some(slippage) = input.slippage {
        body["slippage_bps"] = serde_json::json!(slippage);
    }
    if input.verify {
        body["verify"] = serde_json::json!(true);
    }
    let context = QuoteContext {
        chain_id,
        token_in: from_addr.to_string(),
        token_in_symbol: from_sym.to_string(),
        token_in_decimals: from_dec,
        token_out: to_addr.to_string(),
        token_out_symbol: to_sym.to_string(),
        token_out_decimals: to_dec,
        amount_in,
    };
    Ok((body, context))
}

pub async fn quote(client: &Client, input: QuoteInput) -> Result<QuoteOutput> {
    let (body, request) = build_quote_body(&input)?;
    let response = client.quote(&body).await?;
    Ok(QuoteOutput { request, response })
}

pub async fn batch_quote(
    client: &Client,
    chain: &str,
    pairs: &[String],
    amount: &str,
) -> Result<Vec<BatchQuoteResult>> {
    parse_raw_amount("batch quote amount", amount)?;
    let chain_id = chain_name_to_id(chain).ok_or_else(|| eyre!("unknown chain: {chain}"))?;
    let mut results = Vec::with_capacity(pairs.len());
    for pair in pairs {
        results.push(batch_one(client, chain, chain_id, pair, amount).await);
    }
    Ok(results)
}

async fn batch_one(
    client: &Client,
    chain: &str,
    chain_id: u64,
    pair: &str,
    amount: &str,
) -> BatchQuoteResult {
    let Some((from, to)) = pair.split_once('/') else {
        return batch_error(pair, format!("invalid pair format '{pair}', use FROM/TO"));
    };
    let input = QuoteInput {
        chain: chain.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        amount: amount.to_string(),
        slippage: None,
        verify: false,
    };
    match quote(client, input).await {
        Ok(out) => format_batch_success(pair, chain_id, out),
        Err(err) => batch_error(pair, format!("{err}")),
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

    #[test]
    fn quote_body_preserves_raw_digits_without_float_fields() {
        let input = QuoteInput {
            chain: "base".to_string(),
            from: "USDC".to_string(),
            to: "WETH".to_string(),
            amount: "1000000".to_string(),
            slippage: None,
            verify: false,
        };
        let (body, context) = build_quote_body(&input).expect("quote body");
        assert_eq!(body["amount_in"], "1000000");
        assert!(body.get("amount_usd").is_none());
        assert_eq!(context.amount_in, "1000000");
    }

    #[test]
    fn one_raw_unit_is_not_scaled_for_six_or_eighteen_decimal_tokens() {
        for (from, expected_decimals) in [("USDC", 6), ("WETH", 18)] {
            let input = QuoteInput {
                chain: "base".to_string(),
                from: from.to_string(),
                to: "USDC".to_string(),
                amount: "1".to_string(),
                slippage: None,
                verify: false,
            };
            let (body, context) = build_quote_body(&input).expect("quote body");
            assert_eq!(context.token_in_decimals, expected_decimals);
            assert_eq!(body["amount_in"], "1");
        }
    }

}
