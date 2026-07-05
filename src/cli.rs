// Clap argument definitions for the AgentSwap binary.
// Exports: Cli and Commands.
// Deps: clap derive macros.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agentswap",
    version,
    about = "AgentSwap CLI — AI-agent-optimized DEX aggregator"
)]
pub struct Cli {
    /// Output raw JSON instead of formatted tables
    #[arg(short, long, global = true)]
    pub json: bool,

    /// AgentSwap service URL
    #[arg(
        short,
        long,
        global = true,
        env = "AGENTSWAP_URL",
        default_value = "https://api.agentswap.co"
    )]
    pub url: String,

    /// API key for authenticated endpoints
    #[arg(short = 'k', long, global = true, env = "SR_API_KEY")]
    pub api_key: Option<String>,

    /// Local signer key file for trade/MCP signing
    #[arg(long, global = true, env = "AGENTSWAP_KEY_FILE")]
    pub key_file: Option<String>,

    /// Enable x402 paid retry flow
    #[arg(long, global = true, env = "AGENTSWAP_X402")]
    pub x402: bool,

    /// Prefer x402 even when an API key is configured
    #[arg(long, global = true)]
    pub prefer_x402: bool,

    /// Local key file used for x402 EIP-3009 signatures
    #[arg(long, global = true, env = "AGENTSWAP_X402_KEY_FILE")]
    pub x402_key_file: Option<String>,

    /// x402 chain ID
    #[arg(long, global = true, env = "AGENTSWAP_X402_CHAIN", default_value_t = 8453)]
    pub x402_chain: u64,

    /// Maximum x402 payment amount in raw token units
    #[arg(long, global = true, env = "AGENTSWAP_X402_MAX_AMOUNT", default_value = "0")]
    pub x402_max_amount: String,

    /// x402 asset symbol or address selector
    #[arg(long, global = true, env = "AGENTSWAP_X402_ASSET", default_value = "USDC")]
    pub x402_asset: String,

    /// Gateway URL for relayed trade submission
    #[arg(long, global = true, env = "AGENTSWAP_GATEWAY_URL")]
    pub gateway_url: Option<String>,

    /// Allow trade execution; otherwise trade is forced to dry-run
    #[arg(long, global = true)]
    pub allow_trade: bool,

    /// Per-trade notional cap on amountIn (raw token units); refuses to sign/relay above it. Also bounds MCP trades.
    #[arg(long = "max-amount", global = true, env = "AGENTSWAP_TRADE_MAX_AMOUNT")]
    pub trade_max_amount: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Get quotes for multiple token pairs at once
    BatchQuote {
        #[arg(short, long)]
        chain: String,
        #[arg(required = true)]
        pairs: Vec<String>,
        #[arg(short, long)]
        amount: String,
    },
    /// List supported chains and DEX info
    Chains,
    /// Show wallet call instructions for buying API quota
    BuyQuota {
        #[arg(short, long)]
        chain: String,
        #[arg(short, long)]
        token: String,
        #[arg(short, long)]
        amount: String,
    },
    /// Get a swap quote
    Quote {
        #[arg(short, long)]
        chain: String,
        #[arg(short, long)]
        from: String,
        #[arg(short, long)]
        to: String,
        #[arg(short, long)]
        amount: String,
        #[arg(short, long)]
        slippage: Option<u16>,
        #[arg(long)]
        verify: bool,
    },
    /// Check service health
    Health,
    /// Show API key status, quota, and usage
    KeyInfo,
    /// List supported tokens
    Tokens {
        #[arg(short, long)]
        chain: Option<String>,
    },
    /// Inspect a specific pool
    Pools {
        #[arg(short, long)]
        chain: String,
        #[arg(short, long)]
        address: String,
    },
    /// Register a new API key via wallet signature
    Register {
        #[arg(long)]
        address: String,
        #[arg(long)]
        private_key: Option<String>,
        #[arg(long)]
        key_file: Option<String>,
    },
    /// Show quote pricing + quota purchase contract mapping
    Pricing,
    /// Claim purchased API quote quota using an on-chain tx hash
    QuotaClaim {
        #[arg(short, long)]
        chain: String,
        #[arg(long)]
        tx_hash: String,
    },
    /// Explain a saved quote route by hash
    RouteExplain {
        #[arg(long)]
        hash: String,
    },
    /// Run an MCP server over stdio
    Mcp,
    /// Quote, sign, and optionally relay an agent order
    Trade {
        #[arg(short, long)]
        chain: String,
        #[arg(short, long)]
        from: String,
        #[arg(short, long)]
        to: String,
        #[arg(short, long)]
        amount: String,
        #[arg(short, long)]
        slippage: Option<u16>,
        #[arg(long)]
        min_out: Option<String>,
        #[arg(long, default_value = "agent-order")]
        mode: String,
        #[arg(long, env = "AGENTSWAP_PROXY")]
        proxy: String,
        #[arg(long)]
        nonce: Option<String>,
        #[arg(long, default_value_t = 120)]
        deadline_secs: u64,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        relay: bool,
        #[arg(long)]
        self_submit: bool,
        #[arg(long)]
        key_file: Option<String>,
    },
}
