// Key info command for cached API-key status and quota details.
// Exports: run.
// Deps: crate::client, comfy-table, eyre, serde_json.

use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::Result;
use crate::client::Client;

pub async fn run(client: &Client, json: bool) -> Result<()> {
    let resp = client.key_info().await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }

    let status = resp["status"].as_str().unwrap_or("unknown");
    let quota_total = resp["quota_total"].as_u64().unwrap_or(0);
    let quota_used = resp["quota_used"].as_u64().unwrap_or(0);
    let quota_remaining = resp["quota_remaining"].as_u64().unwrap_or(0);
    let created = resp["created_at"].as_str().unwrap_or("-");
    let wallet = resp["wallet"].as_str().unwrap_or("-");

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["API Key Info", ""]);
    table.add_row(vec!["Status", status]);
    table.add_row(vec!["Wallet", wallet]);
    table.add_row(vec!["Quota Total", &quota_total.to_string()]);
    table.add_row(vec!["Quota Used", &quota_used.to_string()]);
    table.add_row(vec!["Quota Remaining", &quota_remaining.to_string()]);
    table.add_row(vec!["Created", created]);

    println!("{table}");
    Ok(())
}
