// Clap argument definitions for the AgentSwap binary.
// Exports: Cli, Commands and IntentCommands.
// Deps: clap derive macros, crate::tokens for the published chain-selector copy, the exit statuses.

use crate::service::submit::EXIT_STATUS_HELP;
use crate::tokens::{CHAIN_ID_HELP, V6_CHAINS_NOTE};
use clap::{Parser, Subcommand};

mod intent;
pub use intent::IntentCommands;

#[derive(Parser)]
#[command(
    name = "agentswap",
    version,
    about = "AgentSwap CLI for requesting quotes, inspecting tokens and pools, managing intents, and registering an API key against the AgentSwap service.",
    after_help = EXIT_STATUS_HELP
)]
pub struct Cli {
    /// Output raw JSON instead of formatted tables
    #[arg(short, long, global = true)]
    pub json: bool,

    /// AgentSwap service URL for quotes, tokens, pools, pricing, quota and health. The intent
    /// relay always posts to https://app.agentswap.co and does not follow this setting.
    #[arg(
        short,
        long,
        global = true,
        env = "AGENTSWAP_URL",
        default_value = "https://api.agentswap.co"
    )]
    pub url: String,

    /// AgentSwap API key (SR_API_KEY) for authenticated endpoints; overrides the key cached in
    /// ~/.agentswap/credentials
    #[arg(short = 'k', long, global = true, env = "SR_API_KEY")]
    pub api_key: Option<String>,

    /// Local signer key file for `trade`, `batch-sell run`, `intent place` and the MCP signing tools; also the
    /// fallback x402 signer when --x402-key-file is unset
    #[arg(long, global = true, env = "AGENTSWAP_KEY_FILE")]
    pub key_file: Option<String>,

    /// Pay and retry once when a request answers 402. Needs --x402-max-amount at or above the
    /// amount the service asks for, and a key file (--x402-key-file, else --key-file).
    #[arg(long, global = true, env = "AGENTSWAP_X402")]
    pub x402: bool,

    /// Pay a 402 even when an API key is configured. Requires --x402; a request the service
    /// serves is never paid for.
    #[arg(long, global = true)]
    pub prefer_x402: bool,

    /// Local key file used for x402 EIP-3009 signatures; falls back to --key-file
    #[arg(long, global = true, env = "AGENTSWAP_X402_KEY_FILE")]
    pub x402_key_file: Option<String>,

    /// Chain ID the x402 payment is made on
    #[arg(long = "x402-chainid", global = true, env = "AGENTSWAP_X402_CHAINID", default_value_t = 8453)]
    pub x402_chain_id: u64,

    /// Maximum x402 payment as unsigned decimal digits in raw token units. The default 0 refuses
    /// every payment, so --x402 on its own never pays: set a cap at or above the amount the
    /// service asks for.
    #[arg(long, global = true, env = "AGENTSWAP_X402_MAX_AMOUNT", default_value = "0")]
    pub x402_max_amount: String,

    /// x402 payment asset. Only USDC is supported: the EIP-3009 domain is fixed to USD Coin
    /// version 2, so any other symbol or address is refused before signing.
    #[arg(long, global = true, env = "AGENTSWAP_X402_ASSET", default_value = "USDC")]
    pub x402_asset: String,

    /// Allow signing and live execution of `trade`, `batch-sell run`, `intent place` and their MCP tools. Without it they return unsigned dry-run previews: no intent or
    /// AgentOrder signatures, authorization envelopes, signed calldata, relay or broadcast.
    #[arg(long, global = true)]
    pub allow_trade: bool,

    /// Cap on amountIn for `trade` and `intent place` as unsigned decimal digits in raw token
    /// units; refuses to sign or submit above it. Also bounds `batch-sell run` and the MCP `trade`, `batch_sell_run` and `intent_place`
    /// tools.
    #[arg(long = "max-amount", global = true, env = "AGENTSWAP_TRADE_MAX_AMOUNT")]
    pub trade_max_amount: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Plan a batch sale, share the review URL, then run after owner confirmation.
    #[command(after_help = crate::tokens::HOLDINGS_CHAINS_NOTE)]
    BatchSell { #[command(subcommand)] command: BatchSellCommands },
    /// Discover ERC-20 holdings and optionally quote balances below a USD threshold.
    #[command(after_help = crate::tokens::HOLDINGS_CHAINS_NOTE)]
    Portfolio(crate::service::portfolio::Input),
    /// Create an advisory grant URL; live policies require --replace. No transactions are signed.
    /// Requires unique ERC-20 tokens, a positive spend cap, at most 20 tokens including receive,
    /// and rendered caps of at most 32 characters.
    #[command(after_help = crate::tokens::HOLDINGS_CHAINS_NOTE)]
    GrantLink(crate::service::grant_link::Input),
    /// Get quotes for multiple token pairs at once
    BatchQuote {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Token pairs as FROM/TO, one or more, such as USDC/WETH WETH/ARB
        #[arg(required = true)]
        pairs: Vec<String>,
        /// Unsigned decimal amount in the input token's smallest unit.
        #[arg(short, long)]
        amount: String,
        /// Owner's V6 proxy; required for BNB Smart Chain meta-aggregator quotes.
        #[arg(long)]
        taker: Option<String>,
    },
    /// List supported chains and DEX info
    Chains,
    /// Show wallet call instructions for buying API quota (Base and Arbitrum only)
    BuyQuota {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Token to pay with, symbol or address. USDC buys quota directly; any other token is
        /// quoted to USDC first and bought through the token path.
        #[arg(short, long)]
        token: String,
        /// Unsigned decimal amount in the input token's smallest unit.
        #[arg(short, long)]
        amount: String,
    },
    /// Get a swap quote
    Quote {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Input token symbol or raw address; a raw address outside the built-in registry must
        /// answer decimals() on that chain and is accepted only on the chains `trade` supports
        #[arg(short, long)]
        from: String,
        /// Output token symbol or raw address; a raw address outside the built-in registry must
        /// answer decimals() on that chain and is accepted only on the chains `trade` supports
        #[arg(short, long)]
        to: String,
        /// Unsigned decimal amount in the input token's smallest unit.
        #[arg(short, long)]
        amount: String,
        /// Slippage tolerance in basis points, sent as slippage_bps; omitted from the request
        /// when not given
        #[arg(short, long)]
        slippage: Option<u16>,
        /// Ask the service to verify the quoted output; prints the verified output and deviation
        #[arg(long)]
        verify: bool,
        /// Owner's V6 proxy; required for BNB Smart Chain meta-aggregator quotes.
        #[arg(long)]
        taker: Option<String>,
    },
    /// Check service health
    Health,
    /// Show API key status, quota, and usage
    KeyInfo,
    /// List supported tokens
    Tokens {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: Option<String>,
    },
    /// Inspect a specific pool
    Pools {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Pool address to inspect
        #[arg(short, long)]
        address: String,
    },
    /// Register a new API key via wallet signature
    Register {
        /// Owner wallet address the API key is registered for
        #[arg(long)]
        address: String,
        /// Private key that signs the registration challenge; --key-file takes precedence
        #[arg(long)]
        private_key: Option<String>,
        /// File holding the private key that signs the registration challenge.
        #[arg(long)]
        key_file: Option<String>,
    },
    /// Show quote pricing + quota purchase contract mapping
    Pricing,
    /// Claim purchased API quote quota using an on-chain tx hash
    QuotaClaim {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Transaction hash of the quota purchase
        #[arg(long)]
        tx_hash: String,
    },
    /// Explain a saved quote route by hash
    RouteExplain {
        /// Hash of the saved quote whose route to explain
        #[arg(long)]
        hash: String,
    },
    /// Run an MCP server over stdio exposing thirteen tools: quote, batch_quote, tokens, pools,
    /// trade, intent_place, intent_list, intent_status, policy, portfolio, grant_link, batch_sell_plan and batch_sell_run
    Mcp,
    /// Place, list, or inspect V6 open intents
    Intent {
        #[command(subcommand)]
        command: IntentCommands,
    },
    /// Show the V6 policy and token budgets for an agent
    #[command(after_help = V6_CHAINS_NOTE)]
    Policy {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Owner wallet whose proxy holds the policy
        #[arg(long)]
        owner: String,
        /// Agent wallet the policy authorizes
        #[arg(long)]
        agent: String,
        /// Blocks scanned for cap events; defaults to 200,000, or 9,000 on BNB Smart Chain with
        /// the built-in public RPC
        #[arg(long)]
        lookback_blocks: Option<u64>,
        /// Token address to read a budget for, repeatable; use it when the cap events are older
        /// than the lookback
        #[arg(long = "token")]
        tokens: Vec<String>,
    },
    /// Preview an unsigned AgentOrder or, with --allow-trade, sign and optionally self-submit.
    /// A dry-run returns the quote, unsigned AgentOrder and digest, with no signature or signed
    /// calldata. It needs a reachable RPC and deployed proxy for policy generation and digest
    /// parity checks; those checks do not prove the trade would execute.
    #[command(after_help = V6_CHAINS_NOTE)]
    Trade {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Input token symbol or raw address; a raw address outside the built-in registry must
        /// answer decimals() on that chain
        #[arg(short, long)]
        from: String,
        /// Output token symbol or raw address; a raw address outside the built-in registry must
        /// answer decimals() on that chain
        #[arg(short, long)]
        to: String,
        /// Unsigned decimal amount in the input token's smallest unit.
        #[arg(short, long)]
        amount: String,
        /// Slippage tolerance in basis points, sent as slippage_bps. In a dry run without
        /// --min-out it also derives the protection floor, which falls back to 50 bps when
        /// --slippage is omitted.
        #[arg(short, long)]
        slippage: Option<u16>,
        /// Minimum output as unsigned decimal digits in raw token units. Required for a live
        /// trade: without it the trade is refused, because the quote server's output is not
        /// trusted as the protection floor. Optional in a dry run.
        #[arg(long)]
        min_out: Option<String>,
        /// Order mode. Only `agent-order` is implemented; any other value is refused.
        #[arg(long, default_value = "agent-order")]
        mode: String,
        /// Address of the owner's V6 proxy that authorizes the agent
        #[arg(long, env = "AGENTSWAP_PROXY")]
        proxy: String,
        /// Order nonce; a random one is used when omitted
        #[arg(long)]
        nonce: Option<String>,
        /// Seconds from now until the signed AgentOrder expires; default 120. This is a different
        /// deadline from `intent place --deadline-secs`, which defaults to the intent window.
        #[arg(long, default_value_t = 120)]
        deadline_secs: u64,
        /// Return the quote, unsigned AgentOrder and digest after on-chain hash parity checks.
        /// Never sign, return signed calldata or broadcast. Forced on unless --allow-trade is set.
        #[arg(long)]
        dry_run: bool,
        /// Broadcast executeAsAgent to the proxy from the --key-file wallet, which pays the gas,
        /// and wait a bounded time for the receipt. Needs --allow-trade; a dry run never sends.
        /// Exits 3 when the transaction reverts and 4 when it was sent but no receipt was read;
        /// both print its hash and status.
        #[arg(long)]
        self_submit: bool,
        /// Local signer key file for this trade; falls back to the global --key-file
        #[arg(long)]
        key_file: Option<String>,
    },
}

#[cfg(test)]
mod tests;

#[derive(Subcommand)]
pub enum BatchSellCommands {
    /// Create one unsigned grant request with exact balance caps and caller-selected criteria;
    /// at most 100 tokens including receive, larger selections are refused.
    Plan(crate::service::batch_sell::PlanInput),
    /// Execute a confirmed request. Requires --key-file; live signing requires --allow-trade.
    Run(crate::service::batch_sell::RunInput),
}
