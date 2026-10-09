# agentswap

AgentSwap CLI for requesting quotes, inspecting tokens and pools, managing intents, and registering an API key against the AgentSwap service.

## Install

```bash
cargo install agentswap
# Or install the prebuilt binary for this platform.
curl -fsSL https://agentswap.co/install.sh | sh
```

The installer downloads release assets named
`agentswap-v<version>-<target>.tar.gz` for the supported targets.

## Usage

```bash
agentswap --help
agentswap health
agentswap chains
agentswap tokens --chainid 8453
agentswap quote --chainid 8453 --from USDC --to WETH --amount 1000000
agentswap batch-quote --chainid 42161 --amount 1000000 USDC/WETH WETH/ARB
agentswap trade --chainid 8453 --from USDC --to WETH --amount 1000000 \
  --proxy 0xYourProxy --key-file ./private-key.txt --dry-run
agentswap route-explain --hash 0xYourQuoteHash
```

Replace `0xYourProxy` with your User Proxy address and `./private-key.txt` with the agent's signing
key file; `--dry-run` returns an unsigned preview and never signs or broadcasts.

Use `--chainid` with a numeric chain ID such as `8453`; known aliases such as `base`, `arb`,
`bsc`, `robinhood` and `ethereum` are also accepted. Quotes, tokens and pools resolve any of
them. Intents, policy and trade are deployed on Base (8453), Arbitrum One (42161), BNB Smart
Chain (56) and Robinhood Chain (4663) only, and an alias that resolves to another chain is
refused by those commands.

## Portfolio and grant links

`portfolio` and `grant-link` are read-only on Base (8453), Arbitrum One (42161), BNB Smart
Chain (56) and Robinhood Chain (4663). They do not sign transactions or x402 payments.

```bash
agentswap portfolio --chainid 8453 --owner "$OWNER" --max-usd 10 --json
agentswap grant-link --chainid 8453 --owner "$OWNER" --agent "$AGENT" \
  --token "$TOKEN" --receive USDC --one-shot --json
agentswap grant-link --chainid 42161 --owner "$OWNER" --agent "$AGENT" \
  --token "$TOKEN:1000000" --receive USDC --epoch 1w --expiry 30d
```

Portfolio combines the app's `/api/wallet-tokens` with repeatable `--token` addresses, without
scanning logs; `--lookback-blocks` is not accepted. Indexed tokens use the endpoint's balance,
decimals, symbol and name without per-token RPC reads. Explicit tokens absent from that response
are read on chain. When indexing
is unavailable, discovery falls back to the live service catalog filtered by chain and registry
tokens, reading their balances on chain. Every row reports discovery sources; zero balances are omitted.
`wallet_tokens` is `indexed`, `unindexed` or `unavailable`; failed sources contribute no candidates.
Catalog failures are reported in `catalog_error`. Discovery may be incomplete;
use `--token` addresses to extend it. Failed balance reads carry
a row status; the command fails if none of the candidate balances can be read.
App `/api/prices` prices and USD values use decimal integer arithmetic. When an app price is absent,
the wallet-token `priceUsd` is shown with source `alchemy` and `floor_eligible: false`; it is never
a sale-floor source. Tokens without either price are unpriced.
Rows carry price `source`, `confidence`, `basis`, `observed`, `source_count` and `floor_eligible`.
DefiLlama and 1inch prices qualify for a floor; oracle prices qualify only when observed and either
supported by at least two sources or based on `manual_pin`, `stablecoin_par` or `onchain_pool`.
`--max-usd` is a USD decimal filter, not a token amount. It quotes the full balance of priced tokens
at or below the threshold and unpriced tokens, into `--quote-token` (default USDC). `dust` is true
only for priced tokens at or below the threshold with a route. Quote failures produce `no_route`;
quotes and indicative USD prices are not sale floors. Where no built-in USDC entry exists, supply
the receive address through `--quote-token`. A missing quote symbol produces a warning: its
`no_route` status does not establish that no economic route exists.

Grant spend inputs are addresses with optional `:raw-cap`. One-shot omitted caps use current owner
balances, with a weekly epoch and UTC expiry in 24 hours. Recurring grants require explicit raw caps,
`--epoch 1h|1d|1w` and `--expiry 7d|30d|90d|ISO`; ISO timestamps must be future whole seconds in UTC
ending `Z`. Caps render using on-chain decimals, up to 32 characters. The receive token is appended
with cap zero. Native tokens, duplicate tokens, more than 20 tokens including receive, and baskets
without a positive cap are refused. A live agent policy requires `--replace`; output identifies the
replaced policy and warns that its entire basket is replaced. The app re-reads metadata and the
owner reviews the advisory link before approving. `--label` and `--note` add optional text.
MCP `portfolio` and `grant_link` expose the same inputs using snake_case names and `tokens` arrays.

## Intents and policy

Run [examples/daily-sell.sh](examples/daily-sell.sh) for one fraction-of-balance sale preview:
`AGENTSWAP_KEY_FILE=/path/to/key examples/daily-sell.sh --owner "$OWNER" --agent "$AGENT" --token WETH --quote-token USDC`.
It requires Bash, curl, bc and jq, checks the token budget and unsigned intent, and stops at the
authorization digest by default. Its example parameters select chain 8453, a 1000 bps fraction,
outputs at +100/-20 bps from the quote, a 300-second decay and a 600-second duration;
`--live` enables signing and relaying and requires sufficient budget and on-chain authorization.

V6 open intents are signed against the owner's V6 proxy and can be inspected before they are
announced. A live `intent place` needs exactly one submission mode: `--relay` hands the signed
intent to the AgentSwap relay, `--self-submit` broadcasts it from the `--key-file` wallet.
`--dry-run` returns the unsigned intent, authorization and digest after on-chain hash parity
checks, without signing, creating an envelope, relaying or broadcasting. Signature-based
authorization validation runs only on the live path. Use `intent list --owner <address>`
and `intent status --id <bytes32>` to inspect them. Raw token addresses are accepted on supported
chains, and every amount is given in the token's smallest unit.

```bash
agentswap intent place --chainid 8453 --proxy-owner 0xOwner --from USDC --to WETH \
  --amount 1000000 --start-out 1000000000000000000 --end-out 900000000000000000 --dry-run
agentswap intent list --chainid 8453 --owner 0xOwner
agentswap intent status --chainid 8453 --id 0xIntentId
agentswap policy --chainid 8453 --owner 0xOwner --agent 0xAgent
```

`trade`, `sweep` and `intent place` are forced to dry-run unless `--allow-trade` is set, and the same flag
gates the MCP `trade`, `sweep` and `intent_place` tools. A live trade also needs `--min-out`: without an
explicit floor the trade is refused, because the quote server's output is not trusted as the
protection floor. Every token amount input is an unsigned decimal integer in the asset's smallest
unit; `--max-amount` bounds the raw input amount before signing or sending. A trade dry-run
reads policy generation and verifies the AgentOrder digest against the chain, then returns the quote,
unsigned AgentOrder and digest. Dry-run outputs omit signatures, authorization envelopes and
signed calldata; these commands do not sign, broadcast or relay in dry-run.

A token is a symbol from the built-in registry or a raw address. A raw address outside the
registry is accepted by `quote`, `trade` and `buy-quota` on the chains `trade` supports, where it
must answer `decimals()`; it is read on chain before any quote or order, and displays label it
with its shortened address.

`agentswap mcp` exposes twelve tools: quote, batch_quote, tokens, pools, trade, intent_place,
intent_list, intent_status, policy, portfolio, grant_link and sweep. `--allow-trade` permits live execution by trade, sweep and
intent_place; without it those three run as dry-runs. Intent listing requires an owner or agent filter. MCP `trade` and `sweep` additionally defaults `dry_run` to true, while the
CLI defaults to a live trade once `--allow-trade` is set.

Intent and policy reads inspect the latest 200,000 blocks on Base, Arbitrum One, BNB Smart Chain
and Robinhood Chain. BNB Smart Chain uses 9,000 blocks with the built-in public RPC; set
`AGENTSWAP_RPC_URL_56` or `AGENTSWAP_RPC_URL` for a keyed endpoint to use the full lookback, or
pass `--lookback-blocks`.

## Sweep

```sh
agentswap --allow-trade sweep --chainid 8453 --proxy "$PROXY" --key-file agent.key \
  --token "$WETH" --token "$CBETH" --receive USDC --max-usd 5 --max-loss-bps 100 --self-submit --json
```

`sweep` and MCP `sweep` require `max_usd` (USD decimal), `max_loss_bps` and at least one basket
token address to sell: repeat `--token` on the CLI or pass a nonempty MCP `tokens` array.
`--max-usd` judges the owner's whole holding, even when the grant or allowance limits the sale.
`--max-loss-bps` must be nonnegative and below 10000; it determines the price floor and is also
sent to the quote service as `slippage_bps`.
The agent takes the spend-token addresses listed in `grant-link` output and passes them to `sweep`
after the owner approves the grant. A live market policy and an allowed receive token are required.
Membership, caps, usage and epoch start are read with `agentTokenInfo`; expiry, action mask and
generation are read with `policyOf`. No logs are scanned. Each sale spends the minimum of owner
balance, remaining epoch budget and owner-to-proxy allowance. Prices are available on
Base, Arbitrum One and BNB Smart Chain; unpriced holdings are skipped on all chains.
Both input and receive prices must be floor-eligible under the portfolio pricing rule above.
Floors use integer arithmetic and independent app prices; zero floors and routes below the floor
are skipped. Quotes are requested without server verification. With `--self-submit`, a sale
that would revert at gas estimation fails before broadcast. A sandwich can fill down to the
floor: the floor is the guaranteed minimum for a successful sale, not the quoted output.

Without `--allow-trade`, execution is forced to dry-run. MCP defaults `dry_run` to true.
`--self-submit` pays gas from the agent key's wallet. Without it, a live sweep returns signed
calldata with outcome `skipped` and reason `not_submitted`; dry runs use reason `dry_run`.
Confirmed sales report `sold`.
Each allowed requested token and the receive token has a result row with raw amount, USD value,
floor, quote output and outcome.
Reasons include `receive_token`, `zero`, `unpriced`, `price_not_independent`, `over_max_usd`,
`no_route`, `below_floor`, `quote_failed`, `dry_run`, `not_submitted` and `sweep_stopped`.
Pre-broadcast failures continue; reverted or unknown broadcasts stop later sales, reported as
`sweep_stopped`. The exit code is the worst result (0, 1, 3 or 4); exit 1 can follow earlier
confirmed sales. MCP failures carry the full result. A sweep can exceed a single-trade timeout.

## Exit status

| Status | Meaning |
|---|---|
| 0 | Success |
| 1 | The command failed; no `--self-submit` transaction was broadcast |
| 2 | Invalid arguments |
| 3 | A `--self-submit` transaction was mined and reverted |
| 4 | A `--self-submit` transaction was sent but its receipt was not read, because the RPC failed or the bounded wait ended; it may still be mined |

`--self-submit` signs the transaction before sending it and prints `sending transaction <hash>`
on stderr first. With 3 and 4 the output, `--json` included, also carries the hash and its status
(`tx_hash` and `tx_status` for `intent place`, `self_submit.txHash` and `self_submit.txStatus` for
`trade`); look the hash up before sending again. An error answer to the broadcast exits 1 only
when it is a refusal a node gives before accepting a transaction, such as insufficient funds;
after any other answer the hash is looked up, and a transaction the RPC does not have is reported
with exit 4. The MCP `trade` and `intent_place` tools wait a shorter time, ending before common
MCP request timeouts, and return a reverted or unknown outcome as a tool error that carries the
same fields. A `--relay` error exits 1 without showing whether the relay
broadcast the announce; check `intent list --agent` before placing again. Error text shows a
URL's scheme, host and port only, never its path or query.

## Authenticated Flows

```bash
agentswap register --address 0xOwnerWallet --key-file ./private-key.txt
agentswap key-info
agentswap pricing
agentswap buy-quota --chainid 8453 --token USDC --amount 10000000
agentswap quota-claim --chainid 8453 --tx-hash 0xYourPurchaseTx
```

`buy-quota` prints the wallet calls for a quota purchase and is available on Base and Arbitrum
only.

Set `AGENTSWAP_URL` to target a non-default service endpoint; the intent relay
(`intent place --relay`) always posts to https://app.agentswap.co and does not follow this
setting. Set `SR_API_KEY`, the AgentSwap API key, to override the cached key, and
`AGENTSWAP_RPC_URL_<chainId>` or `AGENTSWAP_RPC_URL` to override the V6 RPC endpoint.
