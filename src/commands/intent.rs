// CLI wrappers and concise displays for V6 intent operations.
// Exports: run_place, run_list, run_status, run_policy.
// Deps: crate::{service::intent, signer}, comfy_table, serde_json.

use crate::service::intent;
use crate::signer::Signer;
use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::Result;
use std::sync::Arc;

pub async fn run_place(
    client: &crate::client::Client,
    input: intent::PlaceInput,
    signer: Arc<dyn Signer>,
    allow_trade: bool,
    json: bool,
) -> Result<()> {
    let result = intent::place(client, input, signer, allow_trade).await?;
    if json { println!("{}", serde_json::to_string_pretty(&result)?); } else { print_place(&result); }
    Ok(())
}

pub async fn run_list(input: intent::ListInput, json: bool) -> Result<()> {
    let result = intent::list(input).await?;
    if json { println!("{}", serde_json::to_string_pretty(&result)?); } else { print_records(&result); }
    Ok(())
}

pub async fn run_status(input: intent::StatusInput, json: bool) -> Result<()> {
    let result = intent::status(input).await?;
    if json { println!("{}", serde_json::to_string_pretty(&result)?); } else { print_records(&[result]); }
    Ok(())
}

pub async fn run_policy(input: intent::PolicyInput, json: bool) -> Result<()> {
    let result = intent::policy(input).await?;
    if json { println!("{}", serde_json::to_string_pretty(&result)?); } else {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL_CONDENSED);
        table.set_header(vec!["AgentSwap Policy", ""]);
        table.add_row(vec!["Owner", result.owner.as_str()]);
        table.add_row(vec!["Agent", result.agent.as_str()]);
        table.add_row(vec!["Proxy", result.proxy.as_str()]);
        table.add_row(vec!["Expiry", result.expiry.as_str()]);
        table.add_row(vec!["Epoch Length", result.epoch_len.as_str()]);
        table.add_row(vec!["Action Mask", result.action_mask.as_str()]);
        table.add_row(vec!["Generation", result.generation.as_str()]);
        if let Some(note) = &result.note { table.add_row(vec!["Note", note]); }
        for token in &result.tokens { table.add_row(vec!["Token", &format!("{} cap={} used={} allowed={}", token.token, token.cap, token.used, token.allowed)]); }
        println!("{table}");
    }
    Ok(())
}

fn print_place(result: &intent::PlaceOutcome) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["AgentSwap Intent", ""]);
    table.add_row(vec!["Dry Run", &result.dry_run.to_string()]);
    table.add_row(vec!["Intent ID", result.id.as_str()]);
    table.add_row(vec!["Order", &format!("{} -> {} amount={} out={}..{}", result.order.token_in, result.order.token_out, result.order.amount_in, result.order.start_amount_out, result.order.end_amount_out)]);
    table.add_row(vec!["Agent", result.authorization.agent.as_str()]);
    table.add_row(vec!["Envelope", result.envelope.as_str()]);
    table.add_row(vec!["Digest", result.digest.as_str()]);
    table.add_row(vec!["Signature", result.signature.as_str()]);
    if let Some(relay) = &result.relay { table.add_row(vec!["Relay", &relay.to_string()]); }
    if let Some(tx_hash) = &result.tx_hash { table.add_row(vec!["Tx Hash", tx_hash.as_str()]); }
    println!("{table}");
}

fn print_records(records: &[intent::IntentRecord]) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["ID", "Placed By", "Pair", "Amount", "Window", "Exclusive", "Floor", "Fee", "Required", "Outsider Floor", "Outsider Required", "Status"]);
    for record in records {
        table.add_row(vec![record.id.as_str(), record.placed_by.as_str(), record.pair.as_str(), record.amount_in.as_str(), record.window.as_str(), &record.exclusive_window.to_string(), record.floor_now.as_str(), record.fee_now.as_str(), record.required_now.as_str(), record.floor_for_outsider.as_str(), record.required_for_outsider.as_str(), &format!("{} ({})", record.status, record.reason)]);
    }
    println!("{table}");
}
