// Route explanation command for inspecting a saved quote by hash.
// Exports: Args, run.
// Deps: crate::client, comfy-table, eyre, serde_json.

use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::Result;
use crate::client::Client;

pub struct Args {
    pub hash: String,
    pub json: bool,
}

pub async fn run(client: &Client, args: Args) -> Result<()> {
    let resp = client.quote_lookup(&args.hash).await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }

    let route = resp["route_path"].as_str().unwrap_or("unknown");
    let output = resp["output"].as_str().unwrap_or("0");
    let source = resp["source"].as_str().unwrap_or("?");
    let gas = resp["gas_estimate"].as_str().unwrap_or("?");
    let executable = resp["route_executable"].as_bool().unwrap_or(false);
    let reason = resp["route_unexecutable_reason"].as_str().unwrap_or("");
    let impact = resp
        .get("price_impact_bps")
        .and_then(|v| v.as_u64())
        .map(|b| format!("{:.2}%", b as f64 / 100.0))
        .unwrap_or_else(|| "-".into());
    let grades = resp["pool_grades"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|value| value.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "-".into());

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["Route Explanation", ""]);
    table.add_row(vec!["Hash", &args.hash]);
    table.add_row(vec!["Route", route]);
    table.add_row(vec!["Output", output]);
    table.add_row(vec!["Source", source]);
    table.add_row(vec!["Pool Grades", &grades]);
    table.add_row(vec!["Price Impact", &impact]);
    table.add_row(vec!["Gas Estimate", gas]);
    table.add_row(vec!["Executable", &executable.to_string()]);
    if !executable && !reason.is_empty() {
        table.add_row(vec!["Reason", reason]);
    }

    println!("{table}");
    Ok(())
}
