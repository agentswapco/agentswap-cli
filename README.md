# agentswap

AgentSwap CLI for requesting quotes, inspecting tokens and pools, managing V5 intents, and registering an API key against the AgentSwap service.

## Install

```bash
cargo install agentswap
```

## Usage

```bash
agentswap --help
agentswap health
agentswap chains
agentswap tokens --chain base
agentswap quote --chain base --from USDC --to WETH --amount 1000
agentswap batch-quote --chain arbitrum --amount 1000 USDC/WETH WETH/ARB
agentswap trade --chain base --from USDC --to WETH --amount 100 --dry-run
```

## Intents and policy

V5 open intents are signed against the owner proxy and can be inspected before they are
announced. Use `intent place` with `--dry-run`, `--relay`, or `--self-submit`, then use
`intent list --owner <address>` and `intent status --id <bytes32>` to inspect them. Raw token
addresses are accepted on supported chains; their decimals are read from the token contract.

```bash
agentswap intent place --chain base --proxy-owner 0xOwner --from USDC --to WETH \
  --amount 100 --start-out 99 --end-out 90 --dry-run
agentswap intent list --chain base --owner 0xOwner
agentswap intent status --chain base --id 0xIntentId
agentswap policy --chain base --owner 0xOwner --agent 0xAgent
```

`trade` and intent announce/relay/self-submit operations are forced to dry-run unless
`--allow-trade` is set. `--max-amount` bounds the raw input amount before signing or sending.
Dry-runs may sign locally but never broadcast or relay. `agentswap mcp` exposes quote, trade,
intent place/list/status, and policy tools with the same safety model; intent listing requires
an owner or agent filter.

## Authenticated Flows

```bash
agentswap register --address 0xYourWallet --key-file ./private-key.txt
agentswap key-info
agentswap pricing
agentswap quota-claim --chain base --tx-hash 0xYourPurchaseTx
```

Set `AGENTSWAP_URL` to target a non-default service endpoint and `SR_API_KEY` to override the cached API key.
Set `AGENTSWAP_RPC_URL_<chainId>` or `AGENTSWAP_RPC_URL` to override the V5 RPC endpoint.
