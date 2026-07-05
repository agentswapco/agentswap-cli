# Agent-Facing CLI Design

## Goal

Extend `agentswap` from a human CLI into an agent-facing execution surface without
forking the product logic. The three priority moves compose as:

1. An agent discovers and calls AgentSwap through `agentswap mcp` tools over stdio.
2. The shared HTTP transport pays protected API calls through x402 when no API key is set.
3. The `trade` tool/command quotes, signs the canonical on-chain order digest, then relays
   through the gateway or self-submits on-chain.

This document is a plan only. It does not implement the changes.

## Current Shape

The CLI is a single Rust binary using `clap`, `reqwest`, and `alloy` local signers. Commands
dispatch from `src/main.rs` into thin `src/commands/*::run` handlers, and
`src/client/transport.rs` is the only HTTP request path. `src/routes.rs` already contains x402
constants for `/x402/verify`, `/x402/settle`, and `/.well-known/x402`.

Gateway references read for this design: `agentswap-gateway/src/domain/order.rs`,
`src/eth/abi.rs`, `src/eth/sig.rs`, and `src/api/intents.rs`. Contract references read:
`UserProxyV3.hashAgentOrder`, `IntentFactory.createIntent`, and `IntentClone`'s pinned
Permit2 `IntentWitness`.

## Architecture

Add a small tool/service layer under the existing command handlers:

```text
CLI args \
MCP tool  +-> service/tool handlers -> Client -> x402-aware transport -> API/gateway
          |                         -> Signer -> AgentOrder / IntentOrder signatures
          +-> display/json formatting stays in command modules
```

The existing command modules should stop being the only place that builds request bodies.
Move reusable request construction and response DTOs into feature modules, then keep CLI
`run()` functions as parsing/display wrappers. MCP tools call the same feature functions and
return JSON content.

## P0: `agentswap mcp`

Use `rmcp` 2.1, the official Rust SDK from the Model Context Protocol Rust repository. It
already provides server macros, schema integration, and stdio transport. Avoid a handwritten
JSON-RPC loop unless `rmcp` blocks a required MCP client, because protocol drift would be
high-risk and unrelated to swap logic.

Command:

```bash
agentswap mcp [--url <url>] [--api-key <key>] [--x402] [--key-file <path>]
```

Tools: `quote`, `batch_quote`, `tokens`, `pools`, and `trade`. `trade` returns the order
digest, signature mode, relay/self-submit status, and tx hash or gateway intent id.

Implementation plan: add `Commands::Mcp` in `src/main.rs`, `src/mcp.rs` for rmcp stdio
bootstrap, and `src/service/` for reusable typed handlers. Keep schemas explicit and small;
do not expose arbitrary URL or raw calldata tools in P0. The server owns one `Client`, one
optional `Signer`, and shared rmcp service state.

Security defaults: MCP `trade` requires explicit signer config, starts as `dry_run = true`
until `--allow-trade` or a tool argument opts into execution, and returns request IDs/digests
so agents can audit what they signed.

## P1: x402 Client

Add a 402-aware transport at `src/client/transport.rs`, not inside individual commands. Quote,
trade, and future paid endpoints then pay transparently.

Flow: send the original request with `x-api-key` if configured. On `402 Payment Required`,
parse the body and/or `WWW-Authenticate` for `accepts`, choose a supported EVM `exact`
USDC/EIP-3009 requirement, sign
`TransferWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)`
with alloy typed data, attach `X-PAYMENT`, and retry the same request once.

Module plan:

- `src/x402/mod.rs`: public config and `PaymentClient`.
- `src/x402/types.rs`: `PaymentRequired`, `Accept`, `PaymentPayload`, EIP-3009 typed data.
- `src/x402/eip3009.rs`: domain construction and signing.
- `src/x402/select.rs`: deterministic requirement selection.
- `src/client/transport.rs`: call `maybe_pay_and_retry(...)` on 402.
- `src/routes.rs`: add helper for x402 discovery only if needed; existing constants stay.

Config: `--x402`, `AGENTSWAP_X402=1`, `AGENTSWAP_X402_KEY_FILE`,
`AGENTSWAP_X402_CHAIN`, `AGENTSWAP_X402_MAX_AMOUNT`, and
`AGENTSWAP_X402_ASSET` defaulting to `USDC`.

Failure rules:

- If both API key and x402 are configured, API key wins unless `--prefer-x402` is set.
- If no acceptable payment option matches the cap/chain/scheme, return the full server
  payment summary in the error.
- If signing succeeds but the retry returns 402 again, report both payment ID and response body.

## P2: `agentswap trade`

Command:

```bash
agentswap trade \
  --chain base \
  --from USDC \
  --to WETH \
  --amount 100 \
  --slippage 50 \
  --mode agent-order|open-intent \
  --key-file ./agent.key \
  --relay|--self-submit \
  [--proxy 0x...] [--gateway-url https://...] [--intent-factory 0x...] [--rpc-url ...]
```

Agent-order path: reuse quote construction to get router, spender, route data, `amountIn`,
and expected output; build `UserProxyV3.AgentOrder` with `agent`, `router`, `tokenIn`,
`amountIn`, `tokenOut`, `minOut`, `nonce`, `deadline`; sign the EIP-712 digest bound to
`chainId` and the user's proxy; then `--relay` posts gateway `/intents` with
`{ kind: "swap", order, agentSig }`, while `--self-submit` sends
`executeAsAgent(order, agentSig, spender, routerData)`.

Open-intent path: build `IntentFactory.IntentOrder` with owner, token, curve, timing, and
nonce fields; build Permit2 `PermitTransferFrom` with `permitted = (tokenIn, amountIn)` and
`deadline = endTime`; sign Permit2 `permitWitnessTransferFrom` with the exact `IntentWitness`
used by `IntentClone`; then `--relay` posts `{ kind: "limit", order, permit2Sig }`, while
`--self-submit` sends permissionless `IntentFactory.createIntent(order, permit2Sig)`.

Flags:

- `--dry-run`: default for first release; prints quote, order, digest, and typed data.
- `--deadline-secs`: default 120 for agent orders.
- `--nonce`: explicit nonce; otherwise use a nonce allocator.
- `--min-out` / `--slippage`: choose exactly one.
- `--owner`: required for open intent when signer address is not the owner.
- `--start-time`, `--end-time`, `--start-out`, `--end-out`: open Dutch/limit controls.
- `--limit-out`: shortcut setting `startAmountOut == endAmountOut`.
- `--permit2-nonce-domain open-intent`: reserves a disjoint nonce range from legacy paths.

Env: `AGENTSWAP_KEY_FILE`, `AGENTSWAP_RPC_URL_<CHAIN_ID>` or `AGENTSWAP_RPC_URL`,
`AGENTSWAP_GATEWAY_URL`, `AGENTSWAP_PROXY_<CHAIN_ID>`, and
`AGENTSWAP_INTENT_FACTORY_<CHAIN_ID>`.

## Digest Parity Requirement

The CLI must never hand-roll EIP-712 strings independently from the gateway/contracts.
Digest parity is a release blocker:

- CLI `AgentOrder` digest must equal `UserProxyV3.hashAgentOrder(order)` for the same proxy
  and chain.
- CLI open-intent Permit2 witness must match `IntentClone.INTENT_WITNESS_TYPEHASH` and
  `INTENT_WITNESS_TYPE_STRING` byte-for-byte.
- `IntentFactory` order identity must use the same `keccak256(abi.encode(o))` and salt input
  `keccak256(abi.encode("AgentSwap.Intent.v1", orderHash))`.

Preferred strategy:

1. Extract the gateway's `eth::abi`, `eth::sig`, and order DTO conversions into a new shared
   crate, `agentswap-order-types`.
2. Depend on that crate from both `agentswap-cli` and `agentswap-gateway`.
3. Generate alloy `sol!` structs once in the shared crate so the Rust field order and type
   hashes are identical wherever signatures are produced or verified.
4. Add parity tests:
   - Rust unit test vector for `AGENT_ORDER_TYPEHASH`.
   - Fork/anvil test calling `hashAgentOrder` and comparing to Rust `signing_hash`.
   - Foundry or fork dry-run for Permit2 `permitWitnessTransferFrom` using the Rust-produced
     `permit2Sig`.

Fallback strategy:

- Vendor the gateway module into `src/order_types/` only for one release, with a source commit
  pin in comments and the same parity tests. Remove the vendored copy once the shared crate is
  published. This is acceptable only if shared-crate publication blocks the CLI release.

## Signer Trait

Ship the trait and `LocalKey` backend first; add one remote backend in the same phase only if
its API is stable enough for CI fixtures.

```rust
#[async_trait::async_trait]
pub trait Signer: Send + Sync {
    fn address(&self) -> alloy::primitives::Address;

    async fn sign_message(&self, message: &[u8]) -> eyre::Result<alloy::primitives::Signature>;
    async fn sign_hash(&self, digest: alloy::primitives::B256) -> eyre::Result<alloy::primitives::Signature>;
    async fn sign_typed_data(&self, typed_data: &agentswap_order_types::TypedDataRequest) -> eyre::Result<alloy::primitives::Signature>;
}
```

Backends:

- `src/signer/local.rs`: wraps `alloy::signers::local::PrivateKeySigner`; supports EIP-191,
  EIP-712 prehash signing, and EIP-3009.
- `src/signer/remote.rs`: trait adapter for remote raw/EIP-712 signing.
- First remote candidate: Coinbase CDP Server Wallet or Turnkey, selected by API maturity and
  ability to sign raw digests plus typed data over REST without exposing private keys.

## Module/File Plan

- `src/main.rs`: add `Mcp` and `Trade` subcommands; wire global x402/signer config.
- `src/client.rs`: store x402 config and signer handle.
- `src/client/transport.rs`: central 402 retry handling.
- `src/commands/trade.rs`: CLI wrapper and display for one-shot trade.
- `src/mcp.rs`: stdio MCP server bootstrap.
- `src/service/quote.rs`: quote request construction shared by CLI/MCP/trade.
- `src/service/trade.rs`: quote-to-order orchestration.
- `src/x402/`: payment parsing, selection, EIP-3009 signing.
- `src/signer/`: signer trait and backends.
- `src/order_types/` or shared `agentswap-order-types`: canonical alloy structs and hashes.
- `src/routes.rs`: add gateway intent route constant and any x402 discovery helper.

Keep each new source file under 300 lines; split DTOs, signing, and transport glue early.

## Dependencies

Add only narrow features:

```toml
rmcp = { version = "2.1", default-features = false, features = ["server", "macros", "transport-io"] }
schemars = { version = "1", features = ["derive"] }
async-trait = "0.1"
base64 = "0.22"
zeroize = { version = "1", features = ["derive"] }
getrandom = "0.3"
```

Expand existing alloy features, still with `default-features = false`:

```toml
alloy = { version = "1.6", default-features = false, features = [
  "std", "sol-types", "contract", "network", "consensus",
  "providers", "provider-http", "reqwest", "rpc-types",
  "signers", "signer-local"
] }
```

Do not use `features = ["full"]`.

## Later Phases

P3 signer backends: add one remote signer after the local trait is stable, store remote config
separately from local key files, and require typed-data fixtures for every backend.

P4 authority standards: model user-to-agent authority as ERC-7715/7710 scoped permissions
once wallet support is mature enough, mapping to token allowlists, per-epoch caps, action
masks, expiry, and chain scope. Register AgentSwap in the ERC-8004 Identity registry using
the existing `/.well-known/agent-registration.json` metadata source.

Explicit exclusion: do not build AP2 or card-rail support in this CLI effort. Track it as a
separate product integration so crypto x402/trade signing does not inherit card compliance
scope.

## Build Sequence

- P0: Extract quote service functions; add `agentswap mcp` with read-only tools.
- P1: Add x402 types and 402 retry transport; enable paid quote from CLI and MCP.
- P2: Create shared `agentswap-order-types`; add digest parity tests before `trade`.
- P3: Add `agentswap trade --dry-run`, then `--relay`, then `--self-submit`.
- P4: Add local signer trait backend and one remote backend.
- P5: Register ERC-8004 metadata and track ERC-7715/7710 authority mapping.

## Verification Plan

- `cargo check -p agentswap` after each implementation phase.
- MCP smoke test with an stdio client: list tools, call `quote`, call `trade --dry-run`.
- x402 integration test using a mock server returning 402 once, then validating `X-PAYMENT`.
- Digest parity test against `UserProxyV3.hashAgentOrder`.
- Permit2 witness dry-run against `IntentClone.fill` or a Foundry helper before enabling
  open-intent execution.

## Open Questions

- Which chains/assets should x402 accept in v1: Base USDC only, or Base plus Arbitrum?
- Does the quote API already return all fields needed for `executeAsAgent` (`router`,
  `spender`, `routerData`), or should `trade` call the gateway route provider directly?
- What is the canonical nonce allocator for agent orders and Permit2 unordered nonces?
- Should `--relay` be default after dry-run, or should execution always require an explicit
  `--execute` flag for MCP hosts?
- Which remote signer backend ships first: Turnkey, Coinbase CDP Server Wallet, or Privy?
