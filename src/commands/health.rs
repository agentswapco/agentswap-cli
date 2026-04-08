// Health command for summarizing service health and capacity.
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

    let pools = &resp["pools"];
    let total_pools = pools["total"].as_u64().unwrap_or(0);
    let v2 = pools["v2"].as_u64().unwrap_or(0);
    let v3 = pools["v3"].as_u64().unwrap_or(0);
    let v4 = pools["v4"].as_u64().unwrap_or(0);

    let sync = &resp["sync"];
    let max_lag = sync["max_lag_ms"].as_u64().unwrap_or(0);

    let system = &resp["system"];
    let rss = system["rss_mb"].as_u64().unwrap_or(0);

    let quotes = &resp["quotes"];
    let sem_avail = quotes["semaphore_available"].as_u64().unwrap_or(0);
    let sem_total = quotes["semaphore_total"].as_u64().unwrap_or(0);

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
    table.add_row(vec!["Pools (total)", &total_pools.to_string()]);
    table.add_row(vec!["  V2 / V3 / V4", &format!("{v2} / {v3} / {v4}")]);
    table.add_row(vec!["Sync Lag", &format!("{max_lag}ms")]);
    table.add_row(vec!["Memory (RSS)", &format!("{rss}MB")]);
    table.add_row(vec!["Capacity", &format!("{sem_avail}/{sem_total} slots")]);
    println!("{table}");

    Ok(())
}
