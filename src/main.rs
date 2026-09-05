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
            chain_id: cli.x402_chain,
            max_amount: cli.x402_max_amount.clone(),
            asset: cli.x402_asset.clone(),
        },
        x402_signer.clone(),
    );
    let gateway_url = cli.gateway_url.clone().unwrap_or_else(|| cli.url.clone());
    let relay_client = client::Client::new(&gateway_url, cli.api_key.clone()).with_x402(
        x402::Config {
            enabled: cli.x402,
            prefer_x402: cli.prefer_x402,
            chain_id: cli.x402_chain,
            max_amount: cli.x402_max_amount.clone(),
            asset: cli.x402_asset.clone(),
        },
        signer.clone(),
    );
    let intent_gateway_url = cli
        .gateway_url
        .clone()
        .unwrap_or_else(|| "https://app.agentswap.co".to_string());
    let intent_relay_client = client::Client::new(&intent_gateway_url, api_key).with_x402(
        x402::Config {
            enabled: cli.x402,
            prefer_x402: cli.prefer_x402,
            chain_id: cli.x402_chain,
            max_amount: cli.x402_max_amount.clone(),
            asset: cli.x402_asset.clone(),
        },
        x402_signer,
    );

    match cli.command {
        Commands::BatchQuote {
            chain,
            pairs,
            amount,
        } => {
            commands::batch_quote::run(
                &client,
                commands::batch_quote::Args {
                    chain,
                    pairs,
                    amount,
                    json: cli.json,
                },
            )
            .await
        }
        Commands::Chains => commands::chains::run(&client, cli.json).await,
        Commands::BuyQuota {
            chain,
            token,
            amount,
        } => {
            commands::buy_quota::run(
                &client,
                commands::buy_quota::Args {
                    chain,
                    token,
                    amount,
                    json: cli.json,
                },
            )
            .await
        }
        Commands::Quote {
            chain,
            from,
            to,
            amount,
            slippage,
            verify,
        } => {
            commands::quote::run(
                &client,
                commands::quote::Args {
                    chain,
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
        Commands::Tokens { chain } => {
            commands::tokens::run(&client, cli.json, chain.as_deref()).await
        }
        Commands::Pools { chain, address } => {
            commands::pools::run(
                &client,
                commands::pools::Args {
                    chain,
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
        Commands::QuotaClaim { chain, tx_hash } => {
            commands::quota_claim::run(
                &client,
                commands::quota_claim::Args {
                    chain,
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
                chain, proxy_owner, from, to, amount, start_out, end_out,
                decay_secs, duration_secs, deadline_secs, relay, self_submit, dry_run,
            } => {
                let signer = signer.ok_or_else(|| eyre::eyre!("intent place requires --key-file or AGENTSWAP_KEY_FILE"))?;
                commands::intent::run_place(
                    &intent_relay_client,
                    service::intent::PlaceInput {
                        chain, proxy_owner, from, to, amount, start_out, end_out,
                        decay_secs, duration_secs, deadline_secs, relay, self_submit, dry_run,
                        max_amount: cli.trade_max_amount.clone(),
                    }, signer, cli.allow_trade, cli.json,
                ).await
            }
            IntentCommands::List { chain, owner, agent } => {
                commands::intent::run_list(service::intent::ListInput { chain, owner, agent }, cli.json).await
            }
            IntentCommands::Status { chain, id } => {
                commands::intent::run_status(service::intent::StatusInput { chain, id }, cli.json).await
            }
        },
        Commands::Policy { chain, owner, agent } => {
            commands::intent::run_policy(service::intent::PolicyInput { chain, owner, agent }, cli.json).await
        }
        Commands::Trade {
            chain,
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
            relay,
            self_submit,
            key_file,
        } => {
            let signer = signer_from_file(key_file.as_deref())?
                .or(signer)
                .ok_or_else(|| eyre::eyre!("trade requires --key-file or AGENTSWAP_KEY_FILE"))?;
            commands::trade::run(
                &client,
                &relay_client,
                signer,
                commands::trade::Args {
                    chain,
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
                    relay,
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
