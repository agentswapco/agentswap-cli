// Batch quote command for multiple token pairs in one CLI invocation.
// Exports: Args, run.
// Deps: crate::{client, tokens}, comfy-table, eyre, serde_json.

use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::{eyre, Result};
use crate::client::Client;
use crate::tokens::{chain_name_to_id, format_amount, resolve_token, scale_amount};

pub struct Args {
    pub chain: String,
    pub pairs: Vec<String>,
    pub amount: String,
    pub json: bool,
}

pub async fn run(client: &Client, args: Args) -> Result<()> {
    let chain_id = chain_name_to_id(&args.chain)
        .ok_or_else(|| eyre!("unknown chain: {}", args.chain))?;

    let mut results = Vec::new();

    for pair in &args.pairs {
        let parts: Vec<&str> = pair.split('/').collect();
        if parts.len() != 2 {
            results.push(QuoteResult {
                pair: pair.clone(),
                output: None,
                route: None,
                error: Some(format!("invalid pair format '{}', use FROM/TO", pair)),
            });
            continue;
        }

        let (from_str, to_str) = (parts[0], parts[1]);

        let from = resolve_token(from_str, chain_id);
        let to = resolve_token(to_str, chain_id);

        if from.is_none() || to.is_none() {
            results.push(QuoteResult {
                pair: pair.clone(),
                output: None,
                route: None,
                error: Some(format!(
                    "unknown token: {}",
                    if from.is_none() { from_str } else { to_str }
                )),
            });
            continue;
        }

        let (from_addr, _from_sym, from_dec) = if let Some(token) = from {
            token
        } else {
            unreachable!("checked above")
        };
        let (to_addr, to_sym, to_dec) = if let Some(token) = to {
            token
        } else {
            unreachable!("checked above")
        };

        let amount_in = scale_amount(&args.amount, from_dec);
        let amount_usd: f64 = args.amount.parse().unwrap_or(0.0);

        let body = serde_json::json!({
            "chain_id": chain_id,
            "token_in": from_addr,
            "token_out": to_addr,
            "amount_in": amount_in,
            "amount_usd": amount_usd,
        });

        match client.quote(&body).await {
            Ok(resp) => {
                let output_raw = resp["output"].as_str().unwrap_or("0");
                let output_fmt = format!("{} {}", format_amount(output_raw, to_dec), to_sym);
                let route = resp["route_path"].as_str().map(String::from);
                results.push(QuoteResult {
                    pair: pair.clone(),
                    output: Some(output_fmt),
                    route,
                    error: None,
                });
            }
            Err(e) => {
                results.push(QuoteResult {
                    pair: pair.clone(),
                    output: None,
                    route: None,
                    error: Some(format!("{e}")),
                });
            }
        }
    }

    if args.json {
        let json_results: Vec<_> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "pair": r.pair,
                    "output": r.output,
                    "route": r.route,
                    "error": r.error,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_results)?);
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["Pair", "Output", "Route"]);

    for r in &results {
        if let Some(err) = &r.error {
            table.add_row(vec![&r.pair, err, "-"]);
        } else {
            table.add_row(vec![
                &r.pair,
                r.output.as_deref().unwrap_or("-"),
                r.route.as_deref().unwrap_or("-"),
            ]);
        }
    }

    println!("{table}");
    Ok(())
}

struct QuoteResult {
    pair: String,
    output: Option<String>,
    route: Option<String>,
    error: Option<String>,
}
