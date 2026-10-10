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

BNB Smart Chain quotes come from the meta-aggregator and need the owner's V6 proxy as taker.
Pass `--taker <0x>` to `quote` and `batch-quote`, or `taker` to MCP `quote` and `batch_quote`.

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

`trade`, `batch-sell run` and `intent place` are forced to dry-run unless `--allow-trade` is set, and the same flag
gates the MCP `trade`, `batch_sell_run` and `intent_place` tools. A live trade also needs `--min-out`: without an
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

`agentswap mcp` exposes fourteen tools: quote, batch_quote, tokens, pools, trade, intent_place,
intent_list, intent_status, policy, portfolio, grant_link, batch_sell_plan, batch_sell_run and batch_sell_report. `--allow-trade` permits live execution by trade, batch_sell_run and
intent_place; without it those three run as dry-runs. Intent listing requires an owner or agent filter. MCP `trade` defaults `dry_run` to true, while the
CLI defaults to a live trade once `--allow-trade` is set.

`intent status` and `intent list --owner` read orders from the public intent index first
(`broadcast.intentscan.net`), which also holds intents the relay published only to the off-chain
stream, with no IntentAnnounced log; live state comes from IntentLensV3. `intent list --agent`,
and either command when the index does not answer, read IntentAnnounced logs; `intent status`
then falls back to the settler's filled or cancelled state by id. Relay responses are recorded as
returned: `{id, mode: "broadcast"}` for a stream publication or the on-chain announce response.

Intent log and policy reads inspect the latest 200,000 blocks on Base, Arbitrum One, BNB Smart Chain
and Robinhood Chain. BNB Smart Chain uses 9,000 blocks with the built-in public RPC; set
`AGENTSWAP_RPC_URL_56` or `AGENTSWAP_RPC_URL` for a keyed endpoint to use the full lookback, or
pass `--lookback-blocks`.

## Batch sell

```sh
agentswap batch-sell plan --chainid 8453 --owner "$OWNER" --agent "$AGENT" \
  --name "Dust sweeper" --note "Sell small holdings for USDC" \
  --receive USDC --max-loss-bps 500 --json
# Give the returned review URL to the owner. After confirmation:
agentswap --allow-trade batch-sell run --request "$REQUEST_URL" --key-file agent.key --wait 60 --json
# Send the owner the report; again after open intents close:
agentswap batch-sell report --request "$REQUEST_URL"
```

Agents use `plan`, give the owner the returned URL, then use `run` after the owner confirms.
After every run, send the owner the `report` output; when intents were still open, send it again
after they close. MCP `batch_sell_plan`, `batch_sell_run` and `batch_sell_report` expose the same
inputs in snake_case, with `tokens` for repeatable `--token`.

Always pass `--name`, a short agent name the owner will recognise; the review page shows it beside
the agent address. `--note` gives the owner the reason for the sale. The request carries them as
`label` and `note`. A name is at most 32 characters and a note at most 140, counted in UTF-16 code
units; control and invisible characters are refused before any request is sent, and runs of
whitespace collapse to one space.

`plan` discovers the portfolio and accepts optional `--min-usd`, `--max-usd`, repeatable `--token`
and repeatable `--exclude` criteria. There is no default holding-size threshold. Explicit tokens
select only those addresses; exclusions take precedence. Unreadable, unpriced or non-independent
holdings cannot be valued for a sale. `below_gas_floor` excludes a holding whose discount budget
(value × maxLossBps / 10000) cannot cover the estimated fill cost. Missing gas or wrapped-native prices produce a warning.
The required `--max-loss-bps` requests a discount from independent market value; the owner may
confirm it or lower it. Planning never signs a transaction or payment.

Each request has exact balance caps, a receive-only zero-cap entry, both market and intent actions,
a weekly epoch and expiry in 24 hours. One batch creates one request with at most 100 tokens,
including the receive entry. Larger selections are refused; narrow the criteria.
Output includes token count, total USD value,
requested discount and left-out tokens with reasons. `grant-link --purpose batch-sell
--max-loss-bps <n>` remains the no-POST fallback and emits `purpose` and `maxloss` link parameters.

`run` accepts a request ID or app review URL and refuses pending or expired requests. It takes the
sale tokens and receive entry from the request, and the proxy and discount from its confirmation.
The signer must match the requested agent and the live policy generation must match confirmation.
Each sale is bounded by the request cap, owner balance, remaining epoch budget and allowance.
Both input and output prices must be floor-eligible under the portfolio pricing rule.

Intent mode is the default: the agent signs against the owner's V6 proxy and relays each intent;
the solver pays fill gas. `--via market` submits market trades from the agent wallet and needs
native gas. The selected action and receive token must be allowed by the live grant.
Without `--allow-trade`, execution returns unsigned previews. `--wait <secs>` applies to intent
mode and shares a bounded status wait across placements. Each status round makes one read of the
owner's intent history from the intent index, which also holds intents published without an
on-chain announce; intents the index cannot settle, or every intent when the index is
unreachable, are read in one batched `IntentLensV3.previewMany` call. The wait scans no logs and
ends once every intent is filled, expired or cancelled.

Each intent is a 10-minute Dutch auction. It starts `--start-premium-bps` above independent market
value (default 100, that is 1%; 0 to 1000) and its price falls linearly to the owner-confirmed
discount floor at expiry; `--start-premium-bps 0` starts at market value. A solver fills once the
falling price reaches one it can pay, so when the market trades above the independent price the
sale can complete above market value. An intent that is not filled before expiry lapses with
nothing sold or lost. `start_out_raw` reports the start and `floor_raw` the floor. The fill-cost
check behind `below_gas_floor` uses the discount budget, market value minus floor, whatever the
start. MCP `batch_sell_run` takes `start_premium_bps`; market mode ignores it.

Per-token results distinguish `placed` from `sold`, report `received_raw` from the intent index's
fill records or confirmed execution events when known, and retain not-sold reasons. Intent proceeds
exclude the protocol fee.
`floor_below_confirmed_discount` and `unpriced_for_confirmed_discount` relay refusals are reported
per token and execution continues. Unknown received amounts remain null. Placement timeouts do
not imply a fill or trigger resubmission. Reverted or unknown market broadcasts stop later sales;
MCP failures carry all token rows. Market exit status is the worst result (0, 1, 3 or 4).

`report` accepts a request ID or app review URL, refuses requests that were never confirmed, and
needs no key file. Per token it gives the status (`sold`, `partly_sold`, `open`, `expired`,
`not_placed` or `unsold`), the cap, the amount sold (the grant budget spent on the token), net
proceeds from fill records, the sold amount's value at independent prices, the discount versus
market in percent, and the fill transaction and time. The summary gives tokens sold, total
proceeds, the market value of what sold, the average and worst discount, the value still unsold
(capped at the owner's balance), the grant expiry, and next steps: report again after open
intents close, or rerun while the grant is valid. Values and discounts use the independent prices
read with the report, not prices at fill time, and only floor-eligible prices count. The report
reads the request, makes one Multicall3 call for the grant budgets, token metadata and owner
balances, and reads the public intent index: the owner's intent history from
`broadcast.intentscan.net` and fill records from `data.intentscan.net`. It scans no logs. Intents
count when the requested agent signed them under the confirmed grant generation for a request token
and the receive token. Without the index, sold amounts come from the grant budgets and proceeds are
unknown. The human output is a Markdown table; `--json` returns the same fields with raw amounts.

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
