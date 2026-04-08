// Quote command for requesting and formatting a swap quote.
// Exports: Args, run.
// Deps: crate::{client, tokens}, comfy-table, eyre, serde_json.

use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::{eyre, Result};
use crate::client::Client;
use crate::tokens::{
    chain_id_to_name, chain_name_to_id, format_amount, resolve_token, scale_amount,
};

pub struct Args {
    pub chain: String,
    pub from: String,
    pub to: String,
    pub amount: String,
    pub slippage: Option<u16>,
    pub verify: bool,
    pub json: bool,
}

pub async fn run(client: &Client, args: Args) -> Result<()> {
    let chain_id = chain_name_to_id(&args.chain).ok_or_else(|| {
        eyre!(
            "unknown chain: {}. Use: ethereum, base, arbitrum",
            args.chain
        )
    })?;

    let (from_addr, from_sym, from_dec) = resolve_token(&args.from, chain_id)
        .ok_or_else(|| eyre!("unknown token '{}' on chain {}", args.from, args.chain))?;

    let (to_addr, to_sym, to_dec) = resolve_token(&args.to, chain_id)
        .ok_or_else(|| eyre!("unknown token '{}' on chain {}", args.to, args.chain))?;

    let (amount_in, amount_usd) = if let Some(raw) = args.amount.strip_prefix("raw:") {
        (raw.to_string(), 0.0)
    } else {
        let scaled = scale_amount(&args.amount, from_dec);
        let usd_estimate: f64 = args.amount.parse().unwrap_or(0.0);
        (scaled, usd_estimate)
    };

    let mut body = serde_json::json!({
        "chain_id": chain_id,
        "token_in": from_addr,
        "token_out": to_addr,
        "amount_in": amount_in,
        "amount_usd": amount_usd,
    });
    if let Some(s) = args.slippage {
        body["slippage_bps"] = serde_json::json!(s);
    }
    if args.verify {
        body["verify"] = serde_json::json!(true);
    }

    let resp = client.quote(&body).await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }

    let chain_name = chain_id_to_name(chain_id);
    let output_raw = resp["output"].as_str().unwrap_or("0");
    let output_fmt = format_amount(output_raw, to_dec);
    let source = resp["source"].as_str().unwrap_or("unknown");
    let gas = resp["gas_estimate"].as_str().unwrap_or("?");
    let route = resp["route_path"].as_str().unwrap_or("-");
    let impact = resp
        .get("price_impact_bps")
        .and_then(|v| v.as_u64())
        .map(|b| format!("{:.2}%", b as f64 / 100.0))
        .unwrap_or_else(|| "-".into());

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["AgentSwap Quote", ""]);
    table.add_row(vec!["Chain", &format!("{chain_name} ({chain_id})")]);
    table.add_row(vec![
        "Input",
        &format!("{} {from_sym}", format_amount(&amount_in, from_dec)),
    ]);
    table.add_row(vec!["Output", &format!("{output_fmt} {to_sym}")]);
    table.add_row(vec!["Route", route]);
    table.add_row(vec!["Source", source]);
    table.add_row(vec!["Price Impact", &impact]);
    table.add_row(vec!["Gas Estimate", gas]);

    if args.verify {
        if let Some(verified) = resp.get("verified_output").and_then(|v| v.as_str()) {
            let verified_fmt = format_amount(verified, to_dec);
            table.add_row(vec!["Verified Output", &format!("{verified_fmt} {to_sym}")]);
        }
        if let Some(dev) = resp.get("deviation_pct").and_then(|v| v.as_f64()) {
            table.add_row(vec!["Deviation", &format!("{dev:.2}%")]);
        }
    }

    if let Some(exec) = resp.get("executable").and_then(|v| v.as_str()) {
        let short = if exec.len() > 20 {
            format!("{}...{}", &exec[..10], &exec[exec.len() - 8..])
        } else {
            exec.to_string()
        };
        table.add_row(vec!["Executable", &short]);
    }

    println!("{table}");
    Ok(())
}
