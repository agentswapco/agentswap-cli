// Batch-sale CLI orchestration and review/execution reporting.
// Planning and unconfirmed-request refusal happen before loading any signer.
use crate::{cli::{Cli, BatchSellCommands}, client::Client, service::{batch_sell, submit::Wait}};
use eyre::Result;

pub async fn dispatch(cli: &Cli, command: &BatchSellCommands) -> Result<()> {
    match command {
        BatchSellCommands::Plan(input) => {
            let client = Client::new(&cli.url, cli.api_key.clone());
            let output = batch_sell::plan(&client, input.clone()).await?;
            if cli.json { println!("{}", serde_json::to_string_pretty(&output)?); }
            else {
                for link in &output.requests { println!("{}", link.url); }
                println!("{} tokens; total ${}; requested discount {} bps", output.count, output.total_value_usd, output.max_loss_bps);
                for row in &output.left_out { println!("Left out {}: {}", row.token, row.reason); }
                for warning in &output.warnings { eprintln!("{warning}"); }
            }
            Ok(())
        }
        BatchSellCommands::Run(input) => {
            let record = batch_sell::load(input).await?;
            let signer = crate::startup::signer_from_file(cli.key_file.as_deref())?
                .ok_or_else(|| eyre::eyre!("batch-sell run requires --key-file"))?;
            let origin = if input.via == batch_sell::Via::Intent { crate::routes::app_origin() } else { &cli.url };
            let payment_signer = crate::startup::signer_from_file(cli.x402_key_file.as_deref())?.unwrap_or_else(|| signer.clone());
            let client = Client::new(origin, cli.api_key.clone().or_else(crate::credentials::load_api_key)).with_x402(
                crate::x402::Config { enabled: cli.x402, prefer_x402: cli.prefer_x402, chain_id: cli.x402_chain_id,
                    max_amount: cli.x402_max_amount.clone(), asset: cli.x402_asset.clone() }, Some(payment_signer));
            let output = batch_sell::run(&client, signer, input.clone(), record, cli.allow_trade, cli.trade_max_amount.as_deref(), Wait::CLI).await?;
            if cli.json { println!("{}", serde_json::to_string_pretty(&output)?); }
            else { for row in &output.tokens { println!("{}", serde_json::to_string(row)?); } }
            output.check()
        }
        BatchSellCommands::Report(input) => {
            let output = batch_sell::report(input).await?;
            if cli.json { println!("{}", serde_json::to_string_pretty(&output)?); }
            else { print!("{}", batch_sell::report_text(&output)); }
            Ok(())
        }
    }
}
