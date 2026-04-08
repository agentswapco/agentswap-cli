// Pricing command for quote pricing and quota contract metadata.
// Exports: run.
// Deps: crate::client, comfy-table, eyre, serde_json.

use comfy_table::{presets::UTF8_FULL, Table};
use eyre::Result;
use crate::client::Client;

pub async fn run(client: &Client, json: bool) -> Result<()> {
    let resp = client.pricing().await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }

    let unit_price_usd = resp["unit_price_usd"].as_f64().unwrap_or(0.0);
    let unit_price_6dec = resp["unit_price_usdc_6dec"].as_u64().unwrap_or(0);
    let x402_version = resp["x402_version"].as_u64().unwrap_or(0);

    let mut overview = Table::new();
    overview.load_preset(UTF8_FULL);
    overview.set_header(vec!["Pricing", ""]);
    overview.add_row(vec!["x402 Version", &x402_version.to_string()]);
    overview.add_row(vec!["USD / Quote", &format!("{unit_price_usd:.4}")]);
    overview.add_row(vec!["USDC(6dec) / Quote", &unit_price_6dec.to_string()]);
    println!("{overview}");

    let mut contracts = Table::new();
    contracts.load_preset(UTF8_FULL);
    contracts.set_header(vec![
        "Chain",
        "Network",
        "Quota Contract",
        "Token",
        "Amount",
    ]);
    if let Some(rows) = resp["contracts"].as_array() {
        for row in rows {
            contracts.add_row(vec![
                &row["chain_id"].to_string(),
                row["network"].as_str().unwrap_or("?"),
                row["quota_contract"].as_str().unwrap_or("?"),
                row["payment_token"].as_str().unwrap_or("?"),
                row["amount_per_quote"].as_str().unwrap_or("?"),
            ]);
        }
    }
    println!("{contracts}");

    Ok(())
}
