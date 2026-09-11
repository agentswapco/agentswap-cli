// Clap argument definitions for the AgentSwap binary.
// Exports: Cli and Commands.
// Deps: clap derive macros, crate::tokens for the published chain-selector copy.

use crate::tokens::{CHAIN_ID_HELP, V6_CHAINS_NOTE};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agentswap",
    version,
    about = "AgentSwap CLI for requesting quotes, inspecting tokens and pools, managing V6 intents, and registering an API key against the AgentSwap service."
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

    /// Local signer key file for `trade`, `intent place` and the MCP signing tools; also the
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

    /// Allow live execution of `trade` and `intent place`; without it both are forced to dry-run,
    /// and the MCP `trade` and `intent_place` tools cannot execute.
    #[arg(long, global = true)]
    pub allow_trade: bool,

    /// Cap on amountIn for `trade` and `intent place` as unsigned decimal digits in raw token
    /// units; refuses to sign or submit above it. Also bounds the MCP `trade` and `intent_place`
    /// tools.
    #[arg(long = "max-amount", global = true, env = "AGENTSWAP_TRADE_MAX_AMOUNT")]
    pub trade_max_amount: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
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
        /// Input token symbol or address on that chain
        #[arg(short, long)]
        from: String,
        /// Output token symbol or address on that chain
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
        /// File holding the private key that signs the registration challenge. This is
        /// register's own flag: it does not read AGENTSWAP_KEY_FILE.
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
    /// Run an MCP server over stdio exposing nine tools: quote, batch_quote, tokens, pools,
    /// trade, intent_place, intent_list, intent_status and policy
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
    /// Quote and sign an AgentOrder, optionally self-submitting it; a dry-run requires a
    /// reachable RPC and deployed V6 proxy to verify policy and order hashes before signing
    #[command(after_help = V6_CHAINS_NOTE)]
    Trade {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Input token symbol or address on that chain
        #[arg(short, long)]
        from: String,
        /// Output token symbol or address on that chain
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
        /// Sign and verify against the chain, then stop: no self-submission. Forced on unless
        /// --allow-trade is set.
        #[arg(long)]
        dry_run: bool,
        /// Broadcast executeAsAgent to the proxy from the --key-file wallet, which pays the gas.
        /// Needs --allow-trade; a dry run never sends.
        #[arg(long)]
        self_submit: bool,
        /// Local signer key file for this trade; falls back to the global --key-file
        #[arg(long)]
        key_file: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum IntentCommands {
    /// Sign and announce an agent-placed open intent
    #[command(after_help = V6_CHAINS_NOTE)]
    Place {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Owner wallet whose V6 proxy signs the intent
        #[arg(long)]
        proxy_owner: String,
        /// Input token symbol or raw address; a raw address must answer decimals() on that chain
        #[arg(short, long)]
        from: String,
        /// Output token symbol or raw address; a raw address must answer decimals() on that chain
        #[arg(short, long)]
        to: String,
        /// Unsigned decimal input amount in the token's smallest unit.
        #[arg(short, long)]
        amount: String,
        /// Unsigned decimal starting output in the token's smallest unit.
        #[arg(long)]
        start_out: String,
        /// Unsigned decimal ending output in the token's smallest unit.
        #[arg(long)]
        end_out: String,
        /// Seconds over which the output decays from --start-out to --end-out; defaults to the
        /// window and is capped at it. Must be greater than zero.
        #[arg(long)]
        decay_secs: Option<u64>,
        /// Intent window in seconds from now; default 600.
        #[arg(long)]
        duration_secs: Option<u64>,
        /// Seconds from now for the agent authorization deadline. Defaults to the end of the
        /// intent window; a value that lands before the window closes is refused with both
        /// numbers named.
        #[arg(long)]
        deadline_secs: Option<u64>,
        /// Have the AgentSwap relay announce the signed intent; it always posts to
        /// https://app.agentswap.co, not to --url. Mutually exclusive with --self-submit, and a
        /// live placement needs exactly one of the two.
        #[arg(long)]
        relay: bool,
        /// Broadcast IntentSettlerV3.announce yourself from the --key-file wallet, which pays the
        /// gas. Mutually exclusive with --relay.
        #[arg(long)]
        self_submit: bool,
        /// Sign and verify against the chain, then stop: no relay, no broadcast. Forced on unless
        /// --allow-trade is set.
        #[arg(long)]
        dry_run: bool,
    },
    /// List announced intents by owner or agent
    #[command(after_help = V6_CHAINS_NOTE)]
    List {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Owner wallet whose intents to list; required unless --agent is given
        #[arg(long, conflicts_with = "agent", required_unless_present = "agent")]
        owner: Option<String>,
        /// Agent wallet that placed the intents; required unless --owner is given
        #[arg(long, conflicts_with = "owner", required_unless_present = "owner")]
        agent: Option<String>,
        /// Blocks scanned for IntentAnnounced events; defaults to 200,000, or 9,000 on BNB Smart
        /// Chain with the built-in public RPC
        #[arg(long)]
        lookback_blocks: Option<u64>,
    },
    /// Inspect an announced intent by bytes32 id
    #[command(after_help = V6_CHAINS_NOTE)]
    Status {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Intent id (bytes32) as announced
        #[arg(long)]
        id: String,
        /// Blocks scanned for the IntentAnnounced event; defaults to 200,000, or 9,000 on BNB
        /// Smart Chain with the built-in public RPC
        #[arg(long)]
        lookback_blocks: Option<u64>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chainid_is_the_only_chain_selector_flag() {
        let cli = Cli::try_parse_from([
            "agentswap", "quote", "--chainid", "8453", "--from", "USDC", "--to", "WETH",
            "--amount", "1",
        ])
        .expect("--chainid should parse");
        let Commands::Quote { chain_id, .. } = cli.command else {
            panic!("expected quote command");
        };
        assert_eq!(chain_id, "8453");
        assert!(Cli::try_parse_from([
            "agentswap", "quote", "--chain", "base", "--from", "USDC", "--to", "WETH",
            "--amount", "1",
        ])
        .is_err());
    }

    #[test]
    fn no_subcommand_still_takes_the_old_chain_flag() {
        // One rename that is only half done would leave the old flag alive on a command nobody
        // exercises, so every subcommand that selects a chain is probed here.
        let cases: [&[&str]; 6] = [
            &["agentswap", "tokens", "--chain", "base"],
            &["agentswap", "pools", "--chain", "base", "--address", "0x1"],
            &["agentswap", "batch-quote", "--chain", "base", "--amount", "1", "USDC/WETH"],
            &["agentswap", "policy", "--chain", "base", "--owner", "0x1", "--agent", "0x2"],
            &["agentswap", "intent", "list", "--chain", "base", "--owner", "0x1"],
            &["agentswap", "quota-claim", "--chain", "base", "--tx-hash", "0x1"],
        ];
        for case in cases {
            assert!(
                Cli::try_parse_from(case).is_err(),
                "--chain still parses for {:?}",
                case[1]
            );
        }
    }

    #[test]
    fn the_x402_payment_chain_is_selected_by_chainid() {
        let cli = Cli::try_parse_from([
            "agentswap", "--x402-chainid", "42161", "tokens", "--chainid", "8453",
        ])
        .expect("--x402-chainid should parse");
        assert_eq!(cli.x402_chain_id, 42161);
        assert!(Cli::try_parse_from([
            "agentswap", "--x402-chain", "42161", "tokens", "--chainid", "8453",
        ])
        .is_err());
    }
}
