// Trade command for quote-to-AgentOrder signing and optional self-submit.
// Exports: Args, run.
// Deps: crate::{client, service::trade, signer}.

use crate::client::Client;
use crate::service::trade::{self, TradeInput};
use crate::signer::Signer;
use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use eyre::Result;
use std::sync::Arc;

pub struct Args {
    pub chain_id: String,
    pub from: String,
    pub to: String,
    pub amount: String,
    pub slippage: Option<u16>,
    pub min_out: Option<String>,
    pub max_amount: Option<String>,
    pub mode: String,
    pub proxy: String,
    pub nonce: Option<String>,
    pub deadline_secs: Option<u64>,
    pub dry_run: bool,
    pub self_submit: bool,
    pub json: bool,
}

pub async fn run(
    client: &Client,
    signer: Arc<dyn Signer>,
    args: Args,
    allow_trade: bool,
) -> Result<()> {
    let json = args.json;
    let outcome = trade::execute_trade(client, signer, input(args), allow_trade).await?;
    if outcome.dry_run || outcome.self_submit.is_some() {
        print_outcome(outcome, json)?;
    }
    Ok(())
}

fn input(args: Args) -> TradeInput {
    TradeInput {
        chain_id: args.chain_id,
        from: args.from,
        to: args.to,
        amount: args.amount,
        slippage: args.slippage,
        min_out: args.min_out,
        max_amount: args.max_amount,
        mode: args.mode,
        proxy: args.proxy,
        nonce: args.nonce,
        deadline_secs: args.deadline_secs,
        dry_run: args.dry_run,
        self_submit: args.self_submit,
    }
}

fn print_outcome(outcome: trade::TradeOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
        return Ok(());
    }
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(vec!["AgentSwap Trade", ""]);
    table.add_row(vec!["Mode", outcome.mode.as_str()]);
    table.add_row(vec!["Dry Run", &outcome.dry_run.to_string()]);
    table.add_row(vec!["Digest", outcome.digest.as_str()]);
    table.add_row(vec!["Agent", outcome.order.agent.as_str()]);
    table.add_row(vec!["Router", outcome.order.router.as_str()]);
    table.add_row(vec!["Token In", outcome.order.token_in.as_str()]);
    table.add_row(vec!["Amount In", outcome.order.amount_in.as_str()]);
    table.add_row(vec!["Token Out", outcome.order.token_out.as_str()]);
    table.add_row(vec!["Min Out", outcome.order.min_out.as_str()]);
    if let Some(preview) = &outcome.self_submit {
        table.add_row(vec!["Self Submit To", preview.to.as_str()]);
        table.add_row(vec!["Self Submit Calldata", preview.calldata.as_str()]);
        if let Some(hash) = &preview.tx_hash {
            table.add_row(vec!["Self Submit Tx", hash.as_str()]);
        }
    }
    println!("{table}");
    Ok(())
}
