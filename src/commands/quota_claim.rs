// Quota claim command for redeeming purchased quote quota.
// Exports: Args, run.
// Deps: crate::{client, tokens}, eyre, serde_json.

use eyre::{eyre, Result};
use crate::client::Client;
use crate::tokens::{chain_name_to_id, unknown_chain_id};

pub struct Args {
    pub chain_id: String,
    pub tx_hash: String,
    pub json: bool,
}

pub async fn run(client: &Client, args: Args) -> Result<()> {
    let chain_id = chain_name_to_id(&args.chain_id)
        .ok_or_else(|| eyre!("{}", unknown_chain_id(&args.chain_id)))?;
    let resp = client.quota_claim(chain_id, &args.tx_hash).await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }

    println!("Quota claim successful");
    println!(
        "Chain:            {}",
        resp["chain_id"].as_u64().unwrap_or(chain_id)
    );
    println!(
        "Tx Hash:          {}",
        resp["tx_hash"].as_str().unwrap_or("?")
    );
    println!(
        "Purchased Quotes: {}",
        resp["purchased_quotes"].as_u64().unwrap_or(0)
    );
    println!(
        "Quota Total:      {}",
        resp["quota_total"].as_i64().unwrap_or(0)
    );
    println!(
        "Quota Used:       {}",
        resp["quota_used"].as_u64().unwrap_or(0)
    );
    println!(
        "Quota Remaining:  {}",
        resp["quota_remaining"].as_i64().unwrap_or(0)
    );
    Ok(())
}
