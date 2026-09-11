// Health command for reporting service status, version and uptime.
// Exports: run.
// Deps: crate::{client, display}, comfy-table, eyre.

use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::Result;
use crate::client::Client;
use crate::display::format_duration;

pub async fn run(client: &Client, json: bool) -> Result<()> {
    let resp = client.health().await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }

    let status = resp["status"].as_str().unwrap_or("unknown");
    let version = resp["version"].as_str().unwrap_or("?");
    let uptime = resp["uptime_secs"].as_u64().unwrap_or(0);

    let status_icon = match status {
        "ok" => "OK",
        "degraded" => "DEGRADED",
        _ => "ERROR",
    };

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["AgentSwap Health", ""]);
    table.add_row(vec!["Status", status_icon]);
    table.add_row(vec!["Version", version]);
    table.add_row(vec!["Uptime", &format_duration(uptime)]);
    println!("{table}");

    Ok(())
}
