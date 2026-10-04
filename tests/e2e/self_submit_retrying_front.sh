#!/usr/bin/env bash
# `intent place --self-submit` behind an RPC front that answers eth_sendRawTransaction with an
# error after the node took the transaction (retrying_rpc_front.py), on a local anvil fork of Base.
# The agent follows the exit status: it places again only on exit 1. Exactly one intent must be
# announced per placement, in two modes: the transaction waits in the pool (automine off), or it is
# mined at once (automine on). Needs anvil, cast and python3; BIN is the agentswap binary.
set -u
BIN=${BIN:?set BIN to the agentswap binary}
FORK_URL=${FORK_URL:-https://mainnet.base.org}
PORT=${PORT:-28556}; FRONT_PORT=${FRONT_PORT:-28557}
RPC=http://127.0.0.1:$PORT; FRONT=http://127.0.0.1:$FRONT_PORT
SRC=$(cd "$(dirname "$0")/../.." && pwd); WORK=$(mktemp -d)
USDC=0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913; WETH=0x4200000000000000000000000000000000000006
address_of() { grep -o "$1: Address = alloy::primitives::address!(\"0x[0-9a-fA-F]*\")" "$SRC/src/evm.rs" | grep -o '0x[0-9a-fA-F]*'; }
FACTORY=$(address_of FACTORY); SETTLER=$(address_of SETTLER)
APID=; FPID=; FAILED=0
trap '[ -n "$APID" ] && kill $APID 2>/dev/null; [ -n "$FPID" ] && kill $FPID 2>/dev/null; rm -rf "$WORK"' EXIT
for port in "$PORT" "$FRONT_PORT"; do
  if cast chain-id --rpc-url "http://127.0.0.1:$port" > /dev/null 2>&1; then echo "port $port is already in use"; exit 2; fi
done
anvil --fork-url "$FORK_URL" --port "$PORT" > "$WORK/anvil.log" 2>&1 & APID=$!
for _ in $(seq 1 120); do cast chain-id --rpc-url "$RPC" > /dev/null 2>&1 && break; sleep 1; done
python3 "$SRC/tests/e2e/retrying_rpc_front.py" "$FRONT_PORT" "$RPC" 2> "$WORK/front.log" & FPID=$!
for _ in $(seq 1 20); do cast chain-id --rpc-url "$FRONT" > /dev/null 2>&1 && break; sleep 0.5; done

new_wallet() { local key; key=0x$(python3 -c 'import secrets; print(secrets.token_hex(32))'); echo "$key $(cast wallet address --private-key "$key")"; }
read -r OKEY OWNER <<< "$(new_wallet)"; read -r AKEY AGENT <<< "$(new_wallet)"
for who in "$OWNER" "$AGENT"; do cast rpc anvil_setBalance "$who" 0x56BC75E2D63100000 --rpc-url "$RPC" > /dev/null; done
cast send --private-key "$OKEY" "$FACTORY" "deploy(address)" "$OWNER" --rpc-url "$RPC" > /dev/null || exit 2
PROXY=$(cast call "$FACTORY" "proxyOf(address)(address)" "$OWNER" --rpc-url "$RPC")
NOW=$(cast block latest -f timestamp --rpc-url "$RPC")
cast send --private-key "$OKEY" "$PROXY" "grantAgent(address,uint64,uint32,uint8,address[],uint256[])" \
  "$AGENT" $((NOW + 86400)) 86400 4 "[$USDC,$WETH]" "[100000000,0]" --rpc-url "$RPC" > /dev/null || exit 2
printf '%s' "${AKEY#0x}" > "$WORK/agent.key"
FROM_BLOCK=$(cast block-number --rpc-url "$RPC")

place() {  # $1 RPC URL, $2 output prefix
  AGENTSWAP_RPC_URL_8453=$1 HOME=$WORK "$BIN" --allow-trade -j intent place --chainid 8453 --proxy-owner "$OWNER" \
    --from USDC --to WETH --amount 1000000 --start-out 400000000000000 --end-out 300000000000000 --self-submit \
    --key-file "$WORK/agent.key" > "$2.json" 2> "$2.err"
  echo $? > "$2.code"
}
announced() { cast logs --from-block "$FROM_BLOCK" --address "$SETTLER" --rpc-url "$RPC" --json 2>/dev/null | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))'; }
wait_for_pending() {
  for _ in $(seq 1 240); do
    case "$(cast rpc txpool_status --rpc-url "$RPC")" in *'"pending":"0x0"'*) sleep 0.5;; *) return 0;; esac
  done
}
expect() { if [ "$2" = "$3" ]; then echo "PASS $1: $2"; else echo "FAIL $1: got '$2', expected '$3'"; FAILED=1; fi; }

for mode in pool mined; do
  echo "=== mode=$mode"
  if [ "$mode" = pool ]; then cast rpc evm_setAutomine false --rpc-url "$RPC" > /dev/null; fi
  before=$(announced)
  place "$FRONT" "$WORK/$mode" & CLI=$!
  if [ "$mode" = pool ]; then wait_for_pending; cast rpc evm_mine --rpc-url "$RPC" > /dev/null; fi
  wait $CLI
  echo "exit=$(cat "$WORK/$mode.code") stdout_bytes=$(wc -c < "$WORK/$mode.json") stderr: $(tr '\n' ' ' < "$WORK/$mode.err" | head -c 300)"
  cast rpc evm_setAutomine true --rpc-url "$RPC" > /dev/null
  if [ "$(cat "$WORK/$mode.code")" = 1 ]; then
    place "$RPC" "$WORK/$mode-retry"
    echo "exit 1 means nothing was sent, so the agent placed again: exit=$(cat "$WORK/$mode-retry.code")"
  fi
  expect "$mode: intents announced for one placement" "$(( $(announced) - before ))" 1
  expect "$mode: exit status" "$(cat "$WORK/$mode.code")" 0
done
echo "--- front answers"; cut -c1-160 "$WORK/front.log"
exit $FAILED
