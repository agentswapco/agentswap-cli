// Quote command for requesting and formatting a swap quote.
// Exports: Args, run.
// Deps: crate::{client, tokens}, comfy-table, eyre, serde_json.

use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::Result;
use crate::client::Client;
use crate::service::quote::{self, QuoteInput};
use crate::tokens::{chain_id_to_name, format_amount};

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
    let json = args.json;
    let verify = args.verify;
    let out = quote::quote(client, QuoteInput {
        chain: args.chain,
        from: args.from,
        to: args.to,
        amount: args.amount,
        slippage: args.slippage,
        verify,
    })
    .await?;
    let resp = out.response;

    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }

    let chain_id = out.request.chain_id;
    let chain_name = chain_id_to_name(chain_id);
    let output_raw = resp["output"].as_str().unwrap_or("0");
    let output_fmt = format_amount(output_raw, out.request.token_out_decimals);
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
        &format!(
            "{} {}",
            format_amount(&out.request.amount_in, out.request.token_in_decimals),
            out.request.token_in_symbol
        ),
    ]);
    table.add_row(vec!["Output", &format!("{} {}", output_fmt, out.request.token_out_symbol)]);
    table.add_row(vec!["Route", route]);
    table.add_row(vec!["Source", source]);
    table.add_row(vec!["Price Impact", &impact]);
    table.add_row(vec!["Gas Estimate", gas]);

    if verify {
        if let Some(verified) = resp.get("verified_output").and_then(|v| v.as_str()) {
            let verified_fmt = format_amount(verified, out.request.token_out_decimals);
            table.add_row(vec![
                "Verified Output",
                &format!("{} {}", verified_fmt, out.request.token_out_symbol),
            ]);
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
