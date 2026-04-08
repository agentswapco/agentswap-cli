# agentswap

AgentSwap CLI for requesting quotes, inspecting tokens and pools, and managing API-key registration against the AgentSwap service.

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
```

## Authenticated Flows

```bash
agentswap register --address 0xYourWallet --key-file ./private-key.txt
agentswap key-info
agentswap pricing
agentswap quota-claim --chain base --tx-hash 0xYourPurchaseTx
```

Set `SR_SERVICE_URL` to target a non-default service endpoint and `SR_API_KEY` to override the cached API key.
