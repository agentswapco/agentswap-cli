# Changelog

## 0.5.0 — 2026-09-07

### Breaking

- All CLI, MCP, and environment monetary inputs are now unsigned decimal integers in the asset's
  smallest unit: `1000000` is one USDC, `1000000000000000000` is one WETH. Human-unit parsing,
  token-decimal scaling and the `raw:` prefix were removed, so `--amount 1000000` on a 6-decimal
  token now means one token where 0.4.x meant a million. A value that is not digits — a fraction,
  an exponent, a sign, a separator, an empty string — is refused before any quote, RPC call,
  signature, relay or broadcast, and no amount passes through a float. Human-readable values are
  printed beside the raw one and are never read back.
- Quote requests no longer carry the floating-point `amount_usd` field.

### Fixed

- Intent relay requests include the V6 generation selected with the chain configuration.
- Removed the unavailable trade relay endpoint; trade still supports local signing and
  self-submission.

## 0.4.0 — 2026-09-06

The CLI is now a client for the V6 protocol generation, live since 2026-09-06 at one address set
on Base, Arbitrum, BNB Smart Chain and Robinhood Chain: `UserProxyFactoryV6`
`0xc1660e4BbC825f8367dA92b60dccc17E4E10bc26`, `IntentSettlerV3`
`0x2dd81c4fD1FC38b009Ab10D5C9b1f01Ca51cE462`, `IntentLensV3`
`0x3AFfAafAF3Ec0A8A535723CBf0891482680F30C3`. The contracts 0.3.0 spoke to are not the V6
set; every proxy is new under V6, so an owner re-onboards (new proxy, new `approve` per
token) before an agent can place or trade for them.

### Changed

- `intent place`, `trade` and `policy` resolve the owner's proxy through `UserProxyFactoryV6` and
  announce on `IntentSettlerV3`. The `Order` struct and the agent's EIP-712 domain
  (`AgentSwap UserProxy`, version `5`, the V6 proxy as verifying contract) are unchanged; digest
  parity against the deployed settler and a live V6 proxy was re-run on all four chains before
  release.
- `intent list` and `intent status` read `IntentLensV3`'s nineteen-word preview and refuse to
  decode unless the lens reports `PREVIEW_LAYOUT() == 3`. Each record now carries
  `exclusive_window`, `floor_now`, `fee_now`, `required_now`, `floor_for_outsider` and
  `required_for_outsider`, all raw token units. Inside the exclusive window (before `startTime`)
  the status reason says that only the system filler fills at the floor and an outsider pays
  floor + 25 bps; `required_*` amounts include the 3 bps protocol fee the filler pays on top.
  An order is `expired` when the lens reports it outside its window, not by comparing timestamps
  locally.
- `IntentFilled` decoding carries the new `fee` field; `requiredOut` is the gross the filler owes
  (floor + fee), never the recipient's proceeds.
- Revert diagnostics are regenerated from the V6 proxy sources.

### Notes

- No V6 fill is possible on chain until the solver in open-intent-filler ships and is allowlisted
  on the system filler gate. `list` and `status` were validated against the published lens
  captures and the live lens, not against a filled V6 order.

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
