// Chains command for listing supported networks and DEX coverage.
// Exports: run.
// Deps: crate::client, comfy-table, eyre, serde_json.

use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::{eyre, Result};
use crate::client::Client;

pub async fn run(client: &Client, json: bool) -> Result<()> {
    let resp: serde_json::Value = client.get(crate::routes::CHAINS).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }

    let chains = resp.as_array().ok_or_else(|| eyre!("expected array"))?;

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["Chain", "ID", "DEXes", "Pools", "Executor"]);

    for chain in chains {
        let name = chain["name"].as_str().unwrap_or("?");
        let chain_id = chain["chain_id"].as_u64().unwrap_or(0);
        let dex_count = chain["dex_count"].as_u64().unwrap_or(0);
        let pool_count = chain["pool_count"].as_u64().unwrap_or(0);
        let executor = chain["executor"].as_str().unwrap_or("-");
        let short_exec = if executor.len() > 10 {
            format!(
                "{}...{}",
                &executor[..6],
                &executor[executor.len() - 4..]
            )
        } else {
            executor.to_string()
        };
        table.add_row(vec![
            name,
            &chain_id.to_string(),
            &dex_count.to_string(),
            &pool_count.to_string(),
            &short_exec,
        ]);
    }

    println!("{table}");
    Ok(())
}
