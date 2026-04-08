// Tokens command for listing token metadata returned by the API.
// Exports: run.
// Deps: crate::{client, display, tokens}, comfy-table, eyre, serde_json.

use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::Result;
use crate::client::Client;
use crate::tokens::{chain_id_to_short, chain_name_to_id};

pub async fn run(client: &Client, json: bool, chain: Option<&str>) -> Result<()> {
    let resp = client.tokens().await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }

    let chain_filter = chain.and_then(chain_name_to_id);

    let obj = resp
        .as_object()
        .ok_or_else(|| eyre::eyre!("expected object"))?;

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["Chain", "Address", "Symbol", "Decimals"]);

    let mut entries: Vec<_> = obj.iter().collect();
    entries.sort_by_key(|(_, v)| v["symbol"].as_str().unwrap_or("").to_string());

    for (addr, entry) in entries {
        let cid = entry["chain_id"].as_u64().unwrap_or(0);
        if let Some(filter) = chain_filter
            && cid != filter
        {
            continue;
        }

        let symbol = entry["symbol"].as_str().unwrap_or("?");
        let decimals = entry["decimals"].as_u64().unwrap_or(0);
        let short_addr = crate::display::short_addr(addr);
        let chain_name = chain_id_to_short(cid);
        table.add_row(vec![chain_name, &short_addr, symbol, &decimals.to_string()]);
    }

    println!("{table}");
    Ok(())
}
