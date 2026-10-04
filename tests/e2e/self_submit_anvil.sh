#!/usr/bin/env bash
# End-to-end outcome reporting of `intent place --self-submit` on a local anvil fork of Base:
# a mined announce exits 0, an announce that reverts on chain exits 3 and reports its hash, and an
# RPC that stops answering after the broadcast exits 4 and reports the hash it broadcast.
# Needs anvil, cast and python3. BIN is the agentswap binary; FORK_URL is a Base RPC to fork from.
# Every write goes to the local fork; fresh keys are generated and funded there.
set -u
BIN=${BIN:?set BIN to the agentswap binary}
FORK_URL=${FORK_URL:-https://mainnet.base.org}
PORT=${PORT:-28546}
RPC=http://127.0.0.1:$PORT
SRC=$(cd "$(dirname "$0")/../.." && pwd)
WORK=$(mktemp -d)
USDC=0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913
WETH=0x4200000000000000000000000000000000000006
FACTORY=$(grep -o 'FACTORY: Address = alloy::primitives::address!("0x[0-9a-fA-F]*")' "$SRC/src/evm.rs" | grep -o '0x[0-9a-fA-F]*')
APID=
FAILED=0
trap '[ -n "$APID" ] && kill $APID 2>/dev/null; rm -rf "$WORK"' EXIT

start_anvil() {
  if cast chain-id --rpc-url "$RPC" > /dev/null 2>&1; then echo "port $PORT is already in use"; exit 2; fi
  anvil --fork-url "$FORK_URL" --port "$PORT" > "$WORK/anvil.log" 2>&1 &
  APID=$!
  for _ in $(seq 1 120); do cast chain-id --rpc-url "$RPC" > /dev/null 2>&1 && return 0; sleep 1; done
  echo "anvil did not start"; tail -5 "$WORK/anvil.log"; exit 2
}

stop_anvil() { kill "$APID" 2>/dev/null; wait "$APID" 2>/dev/null; APID=; }

new_wallet() {  # prints "<key> <address>"
  local key; key=0x$(python3 -c 'import secrets; print(secrets.token_hex(32))')
  echo "$key $(cast wallet address --private-key "$key")"
}

setup_owner_and_agent() {
  read -r OKEY OWNER <<< "$(new_wallet)"; read -r AKEY AGENT <<< "$(new_wallet)"
  for who in "$OWNER" "$AGENT"; do cast rpc anvil_setBalance "$who" 0x56BC75E2D63100000 --rpc-url "$RPC" > /dev/null; done
  cast send --private-key "$OKEY" "$FACTORY" "deploy(address)" "$OWNER" --rpc-url "$RPC" > /dev/null || exit 2
  local proxy now; proxy=$(cast call "$FACTORY" "proxyOf(address)(address)" "$OWNER" --rpc-url "$RPC")
  now=$(cast block latest -f timestamp --rpc-url "$RPC")
  cast send --private-key "$OKEY" "$proxy" "grantAgent(address,uint64,uint32,uint8,address[],uint256[])" \
    "$AGENT" $((now + 86400)) 86400 4 "[$USDC,$WETH]" "[1000000,0]" --rpc-url "$RPC" > /dev/null || exit 2
  printf '%s' "${AKEY#0x}" > "$WORK/agent.key"
  echo "owner=$OWNER agent=$AGENT proxy=$proxy"
}

place() {  # $1 = output prefix; JSON to $1.json, stderr to $1.err, exit code to $1.code
  AGENTSWAP_RPC_URL_8453=$RPC HOME=$WORK "$BIN" --allow-trade -j intent place --chainid 8453 \
    --proxy-owner "$OWNER" --from USDC --to WETH --amount 1000000 --start-out 400000000000000 \
    --end-out 300000000000000 --self-submit --key-file "$WORK/agent.key" > "$1.json" 2> "$1.err"
  echo $? > "$1.code"
}

field() { python3 -c 'import json,sys
try: print(json.load(open(sys.argv[1])).get(sys.argv[2]))
except Exception: print("none")' "$1" "$2"; }

wait_for_pending() {
  for _ in $(seq 1 240); do
    case "$(cast rpc txpool_status --rpc-url "$RPC")" in *'"pending":"0x0"'*) sleep 0.5;; *) return 0;; esac
  done
  echo "no transaction reached the pool"; return 1
}

receipt_status() { cast receipt "$1" --json --rpc-url "$RPC" 2>/dev/null | python3 -c 'import json,sys
try: print(int(json.load(sys.stdin)["status"], 16))
except Exception: print("none")'; }

pending_hash() { cast rpc txpool_content --rpc-url "$RPC" | python3 -c 'import json,sys
print(next(t["hash"] for s in json.load(sys.stdin)["pending"].values() for t in s.values()))'; }

expect() {  # $1 scenario, $2 actual, $3 expected
  if [ "$2" = "$3" ]; then echo "PASS $1: $2"; else echo "FAIL $1: got '$2', expected '$3'"; FAILED=1; fi
}

report() {  # $1 output prefix
  echo "exit=$(cat "$1.code") tx_hash=$(field "$1.json" tx_hash) tx_status=$(field "$1.json" tx_status)"
  echo "stderr: $(head -c 400 "$1.err")"
}

echo "=== mined announce"
start_anvil
echo "fork block $(cast block-number --rpc-url "$RPC") factory $FACTORY"
setup_owner_and_agent
place "$WORK/mined"; report "$WORK/mined"
TX=$(field "$WORK/mined.json" tx_hash)
expect "mined: exit" "$(cat "$WORK/mined.code")" 0
expect "mined: receipt status" "$(receipt_status "$TX")" 1

echo "=== announce mined after the intent window, so it reverts"
cast rpc evm_setAutomine false --rpc-url "$RPC" > /dev/null
place "$WORK/reverted" & CLI=$!
wait_for_pending && POOL_TX=$(pending_hash)
cast rpc evm_increaseTime 3600 --rpc-url "$RPC" > /dev/null
cast rpc evm_mine --rpc-url "$RPC" > /dev/null
wait $CLI; report "$WORK/reverted"
TX=$(field "$WORK/reverted.json" tx_hash)
expect "reverted: exit" "$(cat "$WORK/reverted.code")" 3
expect "reverted: reported hash is the broadcast one" "$TX" "${POOL_TX:-none}"
expect "reverted: receipt status" "$(receipt_status "$TX")" 0
expect "reverted: tx_status" "$(field "$WORK/reverted.json" tx_status)" reverted

echo "=== RPC stops answering after the broadcast (fresh fork: the clock above was advanced)"
stop_anvil; start_anvil; setup_owner_and_agent
cast rpc evm_setAutomine false --rpc-url "$RPC" > /dev/null
place "$WORK/unknown" & CLI=$!
wait_for_pending && POOL_TX=$(pending_hash)
stop_anvil
wait $CLI; report "$WORK/unknown"
expect "unknown: exit" "$(cat "$WORK/unknown.code")" 4
expect "unknown: reported hash is the broadcast one" "$(field "$WORK/unknown.json" tx_hash)" "${POOL_TX:-none}"
expect "unknown: tx_status" "$(field "$WORK/unknown.json" tx_status)" unknown

exit $FAILED
