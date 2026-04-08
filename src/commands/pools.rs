// Pools command for inspecting a specific pool state from the API.
// Exports: Args, run.
// Deps: crate::{client, tokens}, comfy-table, eyre, serde_json.

use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::{eyre, Result};
use crate::client::Client;
use crate::tokens::chain_name_to_id;

pub struct Args {
    pub chain: String,
    pub address: String,
    pub json: bool,
}

pub async fn run(client: &Client, args: Args) -> Result<()> {
    let chain_id =
        chain_name_to_id(&args.chain).ok_or_else(|| eyre!("unknown chain: {}", args.chain))?;

    let resp = client.pool(chain_id, &args.address).await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }

    let found_in = resp["found_in"].as_str().unwrap_or("not found");
    let grade = resp.get("grade").and_then(|v| v.as_str()).unwrap_or("-");

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["Pool Info", ""]);
    table.add_row(vec!["Address", resp["address"].as_str().unwrap_or("?")]);
    table.add_row(vec!["Chain", &format!("{} ({})", args.chain, chain_id)]);
    table.add_row(vec!["Found In", found_in]);
    table.add_row(vec!["Grade", grade]);

    if let Some(v2) = resp.get("v2") {
        table.add_row(vec!["Type", "V2"]);
        table.add_row(vec!["Token0", v2["token0"].as_str().unwrap_or("?")]);
        table.add_row(vec!["Token1", v2["token1"].as_str().unwrap_or("?")]);
        table.add_row(vec!["Reserve0", v2["reserve0"].as_str().unwrap_or("0")]);
        table.add_row(vec!["Reserve1", v2["reserve1"].as_str().unwrap_or("0")]);
        table.add_row(vec!["DEX", v2["dex"].as_str().unwrap_or("?")]);
    }
    if let Some(v3) = resp.get("v3") {
        table.add_row(vec!["Type", "V3"]);
        table.add_row(vec!["Token0", v3["token0"].as_str().unwrap_or("?")]);
        table.add_row(vec!["Token1", v3["token1"].as_str().unwrap_or("?")]);
        table.add_row(vec!["Fee", &v3["fee"].as_u64().unwrap_or(0).to_string()]);
        table.add_row(vec!["Liquidity", v3["liquidity"].as_str().unwrap_or("0")]);
        table.add_row(vec!["Tick", &v3["tick"].as_i64().unwrap_or(0).to_string()]);
        table.add_row(vec![
            "Tick Count",
            &v3["tick_count"].as_u64().unwrap_or(0).to_string(),
        ]);
    }

    println!("{table}");
    Ok(())
}
