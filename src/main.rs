// AgentSwap CLI entrypoint for the standalone published crate.
// Exports: binary command parsing and dispatch.
// Deps: clap, tokio, crate::commands, crate::{client, credentials}

mod commands;
mod client;
mod cli;
mod credentials;
mod display;
mod evm;
mod mcp;
mod order_types;
mod routes;
mod service;
mod signer;
mod tokens;
mod x402;

use clap::Parser;
use cli::{Cli, Commands, IntentCommands};
use eyre::Result;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let _ = client::Client::status;

    let cli = Cli::parse();
    let result = run_cli(cli).await;

    if let Err(e) = result {
        let msg = format!("{e}");
        eprintln!("Error: {msg}");
        if msg.contains("401") {
            eprintln!();
            eprintln!("No API key found. To get started:");
            eprintln!("  agentswap register --address <YOUR_WALLET> --key-file <PRIVATE_KEY_FILE>");
            eprintln!("  (or set SR_API_KEY if you already have a key)");
        }
        std::process::exit(1);
    }
}

async fn run_cli(cli: Cli) -> Result<()> {
    let _ = (routes::X402_VERIFY, routes::X402_SETTLE, routes::X402_DISCOVERY);
    if let Some(max_amount) = &cli.trade_max_amount {
        order_types::parse_raw_amount("trade max-amount", max_amount)?;
    }
    order_types::parse_raw_amount("x402 payment cap", &cli.x402_max_amount)?;
    let api_key = cli
        .api_key
        .clone()
        .filter(|k| !k.is_empty())
        .or_else(credentials::load_api_key);
    let signer = signer_from_file(cli.key_file.as_deref())?;
    let x402_signer = signer_from_file(cli.x402_key_file.as_deref().or(cli.key_file.as_deref()))?;
    let client = client::Client::new(&cli.url, api_key.clone()).with_x402(
        x402::Config {
            enabled: cli.x402,
            prefer_x402: cli.prefer_x402,
            chain_id: cli.x402_chain_id,
            max_amount: cli.x402_max_amount.clone(),
            asset: cli.x402_asset.clone(),
        },
        x402_signer.clone(),
    );
    let intent_relay_client = client::Client::new("https://app.agentswap.co", api_key).with_x402(
        x402::Config {
            enabled: cli.x402,
            prefer_x402: cli.prefer_x402,
            chain_id: cli.x402_chain_id,
            max_amount: cli.x402_max_amount.clone(),
            asset: cli.x402_asset.clone(),
        },
        x402_signer,
    );

    match cli.command {
        Commands::BatchQuote {
            chain_id,
            pairs,
            amount,
        } => {
            commands::batch_quote::run(
                &client,
                commands::batch_quote::Args {
                    chain_id,
                    pairs,
                    amount,
                    json: cli.json,
                },
            )
            .await
        }
        Commands::Chains => commands::chains::run(&client, cli.json).await,
        Commands::BuyQuota {
            chain_id,
            token,
            amount,
        } => {
            commands::buy_quota::run(
                &client,
                commands::buy_quota::Args {
                    chain_id,
                    token,
                    amount,
                    json: cli.json,
                },
            )
            .await
        }
        Commands::Quote {
            chain_id,
            from,
            to,
            amount,
            slippage,
            verify,
        } => {
            commands::quote::run(
                &client,
                commands::quote::Args {
                    chain_id,
                    from,
                    to,
                    amount,
                    slippage,
                    verify,
                    json: cli.json,
                },
            )
            .await
        }
        Commands::Health => commands::health::run(&client, cli.json).await,
        Commands::KeyInfo => commands::key_info::run(&client, cli.json).await,
        Commands::Tokens { chain_id } => {
            commands::tokens::run(&client, cli.json, chain_id.as_deref()).await
        }
        Commands::Pools { chain_id, address } => {
            commands::pools::run(
                &client,
                commands::pools::Args {
                    chain_id,
                    address,
                    json: cli.json,
                },
            )
            .await
        }
        Commands::Register {
            address,
            private_key,
            key_file,
        } => {
            commands::register::run(
                &client,
                commands::register::Args {
                    address,
                    private_key,
                    key_file,
                    json: cli.json,
                },
            )
            .await
        }
        Commands::Pricing => commands::pricing::run(&client, cli.json).await,
        Commands::QuotaClaim { chain_id, tx_hash } => {
            commands::quota_claim::run(
                &client,
                commands::quota_claim::Args {
                    chain_id,
                    tx_hash,
                    json: cli.json,
                },
            )
            .await
        }
        Commands::RouteExplain { hash } => {
            commands::route_explain::run(
                &client,
                commands::route_explain::Args {
                    hash,
                    json: cli.json,
                },
            )
            .await
        }
        Commands::Mcp => {
            mcp::serve_stdio(mcp::Config {
                client,
                intent_client: intent_relay_client,
                signer,
                allow_trade: cli.allow_trade,
                trade_max_amount: cli.trade_max_amount.clone(),
            })
            .await
        }
        Commands::Intent { command } => match command {
            IntentCommands::Place {
                chain_id, proxy_owner, from, to, amount, start_out, end_out,
                decay_secs, duration_secs, deadline_secs, relay, self_submit, dry_run,
            } => {
                let signer = signer.ok_or_else(|| eyre::eyre!("intent place requires --key-file or AGENTSWAP_KEY_FILE"))?;
                commands::intent::run_place(
                    &intent_relay_client,
                    service::intent::PlaceInput {
                        chain_id, proxy_owner, from, to, amount, start_out, end_out,
                        decay_secs, duration_secs, deadline_secs, relay, self_submit, dry_run,
                        max_amount: cli.trade_max_amount.clone(),
                    }, signer, cli.allow_trade, cli.json,
                ).await
            }
            IntentCommands::List { chain_id, owner, agent, lookback_blocks } => {
                commands::intent::run_list(service::intent::ListInput { chain_id, owner, agent, lookback_blocks }, cli.json).await
            }
            IntentCommands::Status { chain_id, id, lookback_blocks } => {
                commands::intent::run_status(service::intent::StatusInput { chain_id, id, lookback_blocks }, cli.json).await
            }
        },
        Commands::Policy { chain_id, owner, agent, lookback_blocks, tokens } => {
            commands::intent::run_policy(service::intent::PolicyInput { chain_id, owner, agent, lookback_blocks, tokens }, cli.json).await
        }
        Commands::Trade {
            chain_id,
            from,
            to,
            amount,
            slippage,
            min_out,
            mode,
            proxy,
            nonce,
            deadline_secs,
            dry_run,
            self_submit,
            key_file,
        } => {
            let signer = signer_from_file(key_file.as_deref())?
                .or(signer)
                .ok_or_else(|| eyre::eyre!("trade requires --key-file or AGENTSWAP_KEY_FILE"))?;
            commands::trade::run(
                &client,
                signer,
                commands::trade::Args {
                    chain_id,
                    from,
                    to,
                    amount,
                    slippage,
                    min_out,
                    mode,
                    proxy,
                    nonce,
                    deadline_secs: Some(deadline_secs),
                    dry_run: dry_run || !cli.allow_trade,
                    self_submit,
                    json: cli.json,
                    max_amount: cli.trade_max_amount.clone(),
                },
                cli.allow_trade,
            )
            .await
        }
    }
}

fn signer_from_file(path: Option<&str>) -> Result<Option<Arc<dyn signer::Signer>>> {
    match path {
        Some(path) => {
            let key = signer::local::LocalKey::from_key_file(path)?;
            Ok(Some(Arc::new(key)))
        }
        None => Ok(None),
    }
}
