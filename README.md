# agentswap

AgentSwap CLI for requesting quotes, inspecting tokens and pools, managing V6 intents, and registering an API key against the AgentSwap service.

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
agentswap tokens --chain base
agentswap quote --chain base --from USDC --to WETH --amount 1000000
agentswap batch-quote --chain arbitrum --amount 1000000 USDC/WETH WETH/ARB
agentswap trade --chain base --from USDC --to WETH --amount 1000000 --dry-run
```

## Intents and policy

V6 open intents are signed against the owner proxy and can be inspected before they are
announced. Use `intent place` with `--dry-run`, `--relay`, or `--self-submit`, then use
`intent list --owner <address>` and `intent status --id <bytes32>` to inspect them. Raw token
addresses are accepted on supported chains, and every amount is given in the token's smallest
unit.

```bash
agentswap intent place --chain base --proxy-owner 0xOwner --from USDC --to WETH \
  --amount 1000000 --start-out 1000000000000000000 --end-out 900000000000000000 --dry-run
agentswap intent list --chain base --owner 0xOwner
agentswap intent status --chain base --id 0xIntentId
agentswap policy --chain base --owner 0xOwner --agent 0xAgent
```

`trade` and intent announce/relay/self-submit operations are forced to dry-run unless
`--allow-trade` is set. Every monetary input, including `--max-amount`, is an unsigned decimal
integer in the asset's smallest unit; `--max-amount` bounds the raw input amount before signing
or sending. Human-readable values may appear as supplementary display values only.
Trade dry-runs require a reachable RPC and deployed V6 proxy to verify policy and order hashes before signing. Dry-runs may sign locally but never broadcast or relay. `agentswap mcp` exposes quote, trade,
intent place/list/status, and policy tools with the same safety model; intent listing requires
an owner or agent filter.

Intent and policy reads inspect the latest 200,000 blocks on Base, Arbitrum, and Robinhood Chain.
BSC uses 9,000 blocks with the built-in public RPC; set `AGENTSWAP_RPC_URL_56` or
`AGENTSWAP_RPC_URL` for a keyed endpoint to use the full lookback, or pass `--lookback-blocks`.

## Authenticated Flows

```bash
agentswap register --address 0xYourWallet --key-file ./private-key.txt
agentswap key-info
agentswap pricing
agentswap quota-claim --chain base --tx-hash 0xYourPurchaseTx
```

Set `AGENTSWAP_URL` to target a non-default service endpoint and `SR_API_KEY` to override the cached API key.
Set `AGENTSWAP_RPC_URL_<chainId>` or `AGENTSWAP_RPC_URL` to override the V6 RPC endpoint.
