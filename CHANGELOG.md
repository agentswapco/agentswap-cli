# Changelog

## 0.4.0 — unreleased

The CLI now speaks the V6 protocol generation: `UserProxyV6`, `IntentSettlerV3`, and
`IntentLensV3`, including the exclusive-window pricing and protocol fee in intent reads.

## 0.3.0 — 2026-09-05

The CLI is now a client for the protocol on Base, Arbitrum, BNB Smart Chain and Robinhood Chain.

### Added

- `intent place / list / status`: an authorised agent signs an `IntentAuthorization` on the owner's
  User Proxy domain, wraps it in the authorization envelope and announces the resting Dutch intent
  itself (`--self-submit`) or through the gasless relay (`--relay`). `list` and `status` read
  `IntentAnnounced` logs and `IntentLensV2`, attributing each order to its owner or agent.
- `policy`: an agent's policy (expiry, epoch, action mask, generation) and per-token budgets;
  `--token` reads specific budgets when the cap events are older than the log lookback.
- `mcp`: an MCP server over stdio exposing quote, batch_quote, tokens, pools, trade, intent_place,
  intent_list, intent_status and policy, under the same safety rails as the CLI.
- x402 paid retries for protected API calls when no API key is configured.
- `AGENTSWAP_RPC_URL` / `AGENTSWAP_RPC_URL_<chainId>` and `--lookback-blocks`; BSC defaults to a
  9,000-block lookback on the built-in public RPC, with an actionable error when a node refuses
  a log range.

### Changed

- `trade` signs the V5 `AgentOrder` (nine fields, `generation` bound) on domain version 5, and
  `--self-submit` now broadcasts `executeAsAgent` instead of returning calldata.
- Every digest is cross-checked against the deployed contracts before signing; a dry run still
  signs but never sends. `--allow-trade` is required to send; `--max-amount` bounds `amountIn`
  before signing, and a malformed cap is an error rather than no cap.
- The service URL variable is `AGENTSWAP_URL` (the README previously named `SR_SERVICE_URL`).

### Removed

- The V3/Permit2-era design document and the "self-submit is deferred" path.
