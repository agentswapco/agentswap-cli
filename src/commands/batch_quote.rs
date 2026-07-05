// Batch quote command for multiple token pairs in one CLI invocation.
// Exports: Args, run.
// Deps: crate::{client, tokens}, comfy-table, eyre, serde_json.

use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::Result;
use crate::client::Client;
use crate::service::quote;

pub struct Args {
    pub chain: String,
    pub pairs: Vec<String>,
    pub amount: String,
    pub json: bool,
}

pub async fn run(client: &Client, args: Args) -> Result<()> {
    let results = quote::batch_quote(client, &args.chain, &args.pairs, &args.amount).await?;

    if args.json {
        let json_results: Vec<_> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "pair": r.pair,
                    "output": r.output,
                    "route": r.route,
                    "error": r.error,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_results)?);
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["Pair", "Output", "Route"]);

    for r in &results {
        if let Some(err) = &r.error {
            table.add_row(vec![&r.pair, err, "-"]);
        } else {
            table.add_row(vec![
                &r.pair,
                r.output.as_deref().unwrap_or("-"),
                r.route.as_deref().unwrap_or("-"),
            ]);
        }
    }

    println!("{table}");
    Ok(())
}
