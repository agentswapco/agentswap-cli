# Changelog

## Unreleased

### Added

- Read-only `portfolio` and MCP `portfolio`: ERC-20 discovery with provenance, authoritative
  balances, app wallet-tokens and prices, live chain-filtered catalog, price provenance and floor
  eligibility, best-effort scanned block ranges and full-balance route checks.
- Advisory `grant-link` and MCP `grant_link`: exact decimal caps, one-shot and recurring schedules,
  receive-only tokens, form-encoded app links and explicit replacement of live V6 policies.

### Changed

- `--self-submit` on `trade` and `intent place` reads the receipt status, and the exit status
  tells the outcomes apart: 3 when the transaction was mined and reverted, 4 when it was sent but
  its receipt was not read because the RPC failed or the bounded receipt wait ended, 1 when the
  command failed and no `--self-submit` transaction was broadcast. A reverted transaction used to
  exit 0, and an RPC error after the broadcast used to exit 1 without the hash. `--help` and the
  README list every exit status.
- `--self-submit` signs the transaction before sending it and prints `sending transaction <hash>`
  on stderr first, so a caller that stops waiting still has the hash. With exit status 3 or 4 the
  table and `--json` output also print the hash, with `tx_status` (`confirmed`, `reverted` or
  `unknown`), `tx_error` for `unknown`, and the explorer URL on chains with a known explorer:
  `tx_hash`, `tx_status`, `tx_error` and `tx_explorer_url` for `intent place`;
  `self_submit.txHash`, `txStatus`, `txError` and `txExplorerUrl` for `trade`. A failed receipt
  poll is retried until the wait ends.
- An error answer to the broadcast exits 1 only when it is a refusal a node gives before it
  accepts a transaction: insufficient funds, intrinsic gas too low, a fee cap below the block base
  fee, an invalid sender or chain id, a gas limit above the block's, oversized data or an
  unsupported transaction type. An `already known` or `already imported` answer counts as sent and
  the receipt is awaited. After any other answer, such as `nonce too low` or an internal error from
  an RPC front that retried the send, the hash is looked up once: if the RPC has the transaction
  its receipt is awaited, otherwise the outcome is unknown and the exit status is 4.
- The MCP `trade` and `intent_place` tools return a reverted or unknown outcome as a tool error
  whose text carries the full outcome. Their receipt wait is shorter than the command line's and
  ends before common MCP request timeouts, so the client receives the hash as an unknown outcome
  instead of a timeout.
- `quote`, `trade` and `buy-quota` accept a token address outside the built-in registry on the
  chains `trade` supports. The address must answer `decimals()` on that chain. Displays label it
  with its shortened address, beside its `symbol()` when that is short printable text, so it does
  not read like a registry token. Such an address used to be refused as an unknown token, so
  `trade` could not run on BNB Smart Chain, Robinhood Chain or Arc Testnet.
- The RPC-gated deployed-contract parity test is marked ignored, so a run without its
  environment reports it as ignored instead of passed.
- Error text on stderr, in `--json` output and in MCP tool errors keeps a URL's scheme, host and
  port and replaces its path, query and user info, where RPC keys live, with `[redacted]`.
- The API key remediation is printed for an HTTP 401 answer only, not for any error text that
  contains "401", such as a transaction hash.

## 0.9.0 — 2026-09-16

### Added

- Arc (chain id 5042) joins the V6 chain tables beside Base, Arbitrum One, BNB Smart Chain and
  Robinhood Chain: `--chainid arc` / `arc-mainnet` / `5042`, RPC `https://rpc.mainnet.arc.io`,
  explorer links on `https://explorer.arc.io`, USDC as the gas symbol (the 6-decimal ERC-20 at
  `0x3600000000000000000000000000000000000000` over the 18-decimal native balance) and EURC
  `0xbEf5f6d51CB62b58e6A8f77868681825C6fe21c1` in the built-in token registry. Event scans run in
  5,000-block chunks, inside the 10,000-block `eth_getLogs` limit of that RPC. The V6 registry
  addresses are the ones every other chain uses. Arc Testnet (5042002, `--chainid arc-testnet` /
  `arct`) is available the same way, with `https://testnet.arcscan.app` links. Every command and
  MCP tool description that lists chains names them.
- `examples/daily-sell.sh`: a fraction-of-balance sale previewed through a decaying intent — checks
  the token budget and the unsigned intent and stops at the authorization digest unless `--live`.

### Changed

- Credentials are written with `create_new` and mode `0600` into a random temporary name and
  renamed into place, so a symlink or hard link at the target path is never followed; private key
  material is held in zeroizing buffers. The HTTP client follows no redirects, so an API key or
  payment header is never replayed to another destination. `register` reads the key through the
  same local signer as every other command.
- Releases: a tag push runs the test suite first, every binary build and the crate publish depend
  on it, and the tag must name the `Cargo.toml` version.

## 0.8.0 — 2026-09-13

### Breaking

- A dry-run never signs. `trade` and `intent place` — requested with `--dry-run`, or forced to
  dry-run because `--allow-trade` is not set, on the CLI and through the MCP `trade` and
  `intent_place` tools — now return an unsigned preview: the quote, the unsigned `AgentOrder` or
  intent and authorization, and the digest, after the on-chain hash parity checks. 0.7.x signed in
  a dry-run and returned the signature, the signed `executeAsAgent` calldata or the authorization
  envelope; `executeAsAgent` and `IntentSettlerV3.announce` accept those from any sender, so a
  dry-run output was executable by whoever saw it. In dry-run JSON the `signature`, `envelope` and
  `self_submit` fields are now absent; they are present only on the live path. An intent dry-run
  no longer calls `isIntentAuthorized`, which needs a signature.

### Changed

- `health` reports service status, version and uptime only; the pool, sync-lag, memory and
  capacity rows are gone.
- Help text, MCP tool descriptions and the README state what each command enforces; every flag
  has a help line. Unknown-chain, missing-API-key and quota-estimate messages name the accepted
  inputs and the command that confirms the price.

## 0.7.0 — 2026-09-09

### Breaking

- `intent place` now derives the agent authorization deadline from the order itself: the default
  is the order's `endTime`, and an explicit `--deadline-secs` that lands before `endTime` is
  refused with both numbers named. 0.6.x defaulted to 120 seconds regardless of the order window,
  so a resting order announced through the relay stopped being fillable two minutes in while its
  price curve was still running: `UserProxyV6` refuses the pull once `block.timestamp` passes the
  authorization deadline, and the gas paid to announce it was already spent. Anyone passing a
  `--deadline-secs` shorter than their order window now gets an error instead of a signature.

## 0.6.0 — 2026-09-07

### Breaking

- Every CLI chain selector is now `--chainid`, and numeric chain IDs such as `8453` are the
  documented contract. Known aliases such as `base` remain accepted as a convenience; the old
  `--chain` flag is removed. Matching service and MCP request fields are now named `chain_id`.
- The x402 payment chain flag is `--x402-chainid`, read from `AGENTSWAP_X402_CHAINID`.
- `tokens --chainid` rejects a value that names no chain instead of silently listing every chain.

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

The CLI is now a client for the V6 protocol generation, deployed at one address set
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

- `list` and `status` read the lens; they do not depend on a fill having happened.

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
