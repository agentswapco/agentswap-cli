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
key file; `--dry-run` signs locally and never broadcasts.

Use `--chainid` with a numeric chain ID such as `8453`; known aliases such as `base`, `arb`,
`bsc`, `robinhood` and `ethereum` are also accepted. Quotes, tokens and pools resolve any of
them. Intents, policy and trade are deployed on Base (8453), Arbitrum One (42161), BNB Smart
Chain (56) and Robinhood Chain (4663) only, and an alias that resolves to another chain is
refused by those commands.

## Intents and policy

V6 open intents are signed against the owner's V6 proxy and can be inspected before they are
announced. A live `intent place` needs exactly one submission mode: `--relay` hands the signed
intent to the AgentSwap relay, `--self-submit` broadcasts it from the `--key-file` wallet.
`--dry-run` signs and verifies against the chain, then stops. Use `intent list --owner <address>`
and `intent status --id <bytes32>` to inspect them. Raw token addresses are accepted on supported
chains, and every amount is given in the token's smallest unit.

```bash
agentswap intent place --chainid 8453 --proxy-owner 0xOwner --from USDC --to WETH \
  --amount 1000000 --start-out 1000000000000000000 --end-out 900000000000000000 --dry-run
agentswap intent list --chainid 8453 --owner 0xOwner
agentswap intent status --chainid 8453 --id 0xIntentId
agentswap policy --chainid 8453 --owner 0xOwner --agent 0xAgent
```

`trade` and `intent place` are forced to dry-run unless `--allow-trade` is set, and the same flag
gates the MCP `trade` and `intent_place` tools. A live trade also needs `--min-out`: without an
explicit floor the trade is refused, because the quote server's output is not trusted as the
protection floor. Every monetary input is an unsigned decimal integer in the asset's smallest
unit; `--max-amount` bounds the raw input amount before signing or sending. A trade dry-run
verifies policy and order hashes against the chain before signing, and may sign locally but never
broadcasts or relays.

`agentswap mcp` exposes nine tools: quote, batch_quote, tokens, pools, trade, intent_place,
intent_list, intent_status and policy. `--allow-trade` permits live execution by trade and
intent_place; without it those two run as dry-runs. Intent listing requires an owner or agent filter. MCP `trade` additionally defaults `dry_run` to true, while the
CLI defaults to a live trade once `--allow-trade` is set.

Intent and policy reads inspect the latest 200,000 blocks on Base, Arbitrum One, BNB Smart Chain
and Robinhood Chain. BNB Smart Chain uses 9,000 blocks with the built-in public RPC; set
`AGENTSWAP_RPC_URL_56` or `AGENTSWAP_RPC_URL` for a keyed endpoint to use the full lookback, or
pass `--lookback-blocks`.

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
