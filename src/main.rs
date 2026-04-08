// AgentSwap CLI entrypoint for the standalone published crate.
// Exports: binary command parsing and dispatch.
// Deps: clap, tokio, crate::commands, crate::{client, credentials}

mod commands;
mod client;
mod credentials;
mod display;
mod routes;
mod tokens;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agentswap",
    version,
    about = "AgentSwap CLI — AI-agent-optimized DEX aggregator"
)]
struct Cli {
    /// Output raw JSON instead of formatted tables
    #[arg(short, long, global = true)]
    json: bool,

    /// AgentSwap service URL
    #[arg(
        short,
        long,
        global = true,
        env = "AGENTSWAP_URL",
        default_value = "https://api.agentswap.co"
    )]
    url: String,

    /// API key for authenticated endpoints
    #[arg(short = 'k', long, global = true, env = "SR_API_KEY")]
    api_key: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Get quotes for multiple token pairs at once
    BatchQuote {
        /// Chain name or ID
        #[arg(short, long)]
        chain: String,
        /// Token pairs as FROM/TO (e.g. USDC/WETH WETH/DAI)
        #[arg(required = true)]
        pairs: Vec<String>,
        /// Amount in human-readable units (applied to each pair)
        #[arg(short, long)]
        amount: String,
    },
    /// List supported chains and DEX info
    Chains,
    /// Show wallet call instructions for buying API quota
    BuyQuota {
        /// Chain name or ID
        #[arg(short, long)]
        chain: String,
        /// Payment token (symbol or address)
        #[arg(short, long)]
        token: String,
        /// Amount in human-readable units
        #[arg(short, long)]
        amount: String,
    },
    /// Get a swap quote
    Quote {
        /// Chain name or ID (ethereum, base, arbitrum, 1, 8453, 42161)
        #[arg(short, long)]
        chain: String,
        /// Input token (symbol or address): USDC, WETH, 0x833589f...
        #[arg(short, long)]
        from: String,
        /// Output token (symbol or address)
        #[arg(short, long)]
        to: String,
        /// Amount in human-readable units (e.g. "1000") or raw with "raw:" prefix
        #[arg(short, long)]
        amount: String,
        /// Slippage tolerance in basis points (default: 50 = 0.5%)
        #[arg(short, long)]
        slippage: Option<u16>,
        /// Verify quote on-chain via eth_call dry-run
        #[arg(long)]
        verify: bool,
    },
    /// Check service health
    Health,
    /// Show API key status, quota, and usage
    KeyInfo,
    /// List supported tokens
    Tokens {
        /// Filter by chain name or ID
        #[arg(short, long)]
        chain: Option<String>,
    },
    /// Inspect a specific pool
    Pools {
        /// Chain name or ID
        #[arg(short, long)]
        chain: String,
        /// Pool contract address
        #[arg(short, long)]
        address: String,
    },
    /// Register a new API key via wallet signature (EIP-191)
    Register {
        /// Wallet address (0x...)
        #[arg(long)]
        address: String,
        /// Private key for automated signing (omit for interactive mode) (deprecated: use --key-file instead)
        #[arg(long)]
        private_key: Option<String>,
        /// Path to file containing private key (safer than --private-key)
        #[arg(long)]
        key_file: Option<String>,
    },
    /// Show quote pricing + quota purchase contract mapping
    Pricing,
    /// Claim purchased API quote quota using an on-chain tx hash
    QuotaClaim {
        /// Chain name or ID where the quota purchase tx was sent
        #[arg(short, long)]
        chain: String,
        /// Purchase transaction hash (0x...)
        #[arg(long)]
        tx_hash: String,
    },
    /// Explain a saved quote route by hash
    RouteExplain {
        /// Quote hash (0x...)
        #[arg(long)]
        hash: String,
    },
}

#[tokio::main]
async fn main() {
    let _ = client::Client::status;
    let _ = (routes::X402_VERIFY, routes::X402_SETTLE, routes::X402_DISCOVERY);

    let cli = Cli::parse();
    let api_key = cli
        .api_key
        .filter(|k| !k.is_empty())
        .or_else(credentials::load_api_key);
    let client = client::Client::new(&cli.url, api_key);

    let result = match cli.command {
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
    };

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
