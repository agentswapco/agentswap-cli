// Clap arguments for the `intent` subcommands: place, list and status.
// Exports: IntentCommands.
// Deps: clap derive macros, crate::tokens for the published chain-selector copy.

use crate::tokens::{CHAIN_ID_HELP, V6_CHAINS_NOTE};
use clap::Subcommand;

#[derive(Subcommand)]
pub enum IntentCommands {
    /// Preview an unsigned intent and authorization, or sign and announce with --allow-trade.
    #[command(after_help = V6_CHAINS_NOTE)]
    Place {
        #[arg(short, long = "chainid", help = CHAIN_ID_HELP)]
        chain_id: String,
        /// Owner wallet whose User Proxy authorizes the agent to place the intent
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
        /// gas, and wait a bounded time for the receipt. Mutually exclusive with --relay. Exits 3
        /// when the announce reverts and 4 when it was sent but no receipt was read; both print
        /// its hash and status.
        #[arg(long)]
        self_submit: bool,
        /// Return the unsigned intent, authorization and digest after on-chain hash parity checks.
        /// Never sign, create an envelope, relay or broadcast. Signature-based authorization
        /// validation runs only on the live path. Forced on unless --allow-trade is set.
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
