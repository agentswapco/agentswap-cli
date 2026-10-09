#!/usr/bin/env bash
# Grant-bounded sweep on a Base fork with fresh wallets and exact approvals.
# Real app prices and quote routes; all storage changes and transactions target local anvil.
set -euo pipefail
BIN=${BIN:?set BIN to the agentswap binary}
FORK_URL=${FORK_URL:-https://mainnet.base.org}
PORT=${PORT:-28549}
RPC=http://127.0.0.1:$PORT
SRC=$(cd "$(dirname "$0")/../.." && pwd)
WORK=$(mktemp -d)
APID=
trap 'if [ -n "$APID" ]; then kill "$APID" 2>/dev/null || true; fi; rm -rf "$WORK"' EXIT
read -r WETH CBETH USDC FACTORY < <(python3 - "$SRC" <<'PY'
import pathlib,re,sys
root=pathlib.Path(sys.argv[1])
s=(root/'src/tokens/registry/base.rs').read_text()
a={symbol:address for address,symbol in re.findall(r'address: "([^"]+)",\s*symbol: "([^"]+)"',s)}
f=re.search(r'const FACTORY:.*address!\("([^"]+)"', (root/'src/evm.rs').read_text())[1]
print(a['WETH'],a['cbETH'],a['USDC'],f)
PY
)
if cast chain-id --rpc-url "$RPC" >/dev/null 2>&1; then echo 'FAIL fork port occupied'; exit 1; fi
anvil --fork-url "$FORK_URL" --port "$PORT" >"$WORK/anvil.log" 2>&1 & APID=$!
for _ in $(seq 1 120); do
  if cast chain-id --rpc-url "$RPC" >/dev/null 2>&1; then break; fi
  if ! kill -0 "$APID" 2>/dev/null; then cat "$WORK/anvil.log"; exit 1; fi
  sleep 1
done
cast chain-id --rpc-url "$RPC"
OKEY=$(cast wallet new --json | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["private_key"])')
AKEY=$(cast wallet new --json | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["private_key"])')
OWNER=$(cast wallet address --private-key "$OKEY")
AGENT=$(cast wallet address --private-key "$AKEY")
printf '%s' "$AKEY" > "$WORK/agent.key"
for who in "$OWNER" "$AGENT"; do cast rpc anvil_setBalance "$who" "$(cast to-hex 100000000000000000000)" --rpc-url "$RPC" >/dev/null; done
cast send --private-key "$OKEY" "$FACTORY" 'deploy(address)' "$OWNER" --rpc-url "$RPC" >/dev/null
PROXY=$(cast call "$FACTORY" 'proxyOf(address)(address)' "$OWNER" --rpc-url "$RPC")
# Locate the cbETH balance mapping on the fork, restoring every non-matching slot.
CB_SLOT=
for slot in $(seq 0 100); do
  key=$(cast index address "$OWNER" "$slot")
  previous=$(cast storage "$CBETH" "$key" --rpc-url "$RPC")
  cast rpc anvil_setStorageAt "$CBETH" "$key" "$(cast to-uint256 123456789)" --rpc-url "$RPC" >/dev/null
  balance=$(cast call "$CBETH" 'balanceOf(address)(uint256)' "$OWNER" --rpc-url "$RPC" | awk '{print $1}')
  cast rpc anvil_setStorageAt "$CBETH" "$key" "$previous" --rpc-url "$RPC" >/dev/null
  if [ "$balance" = 123456789 ]; then CB_SLOT=$key; break; fi
done
[ -n "$CB_SLOT" ] || { echo 'FAIL cbETH balance mapping unavailable'; exit 1; }
CAP=1000000000000000
cast send --private-key "$OKEY" "$WETH" 'deposit()' --value "$CAP" --rpc-url "$RPC" >/dev/null
cast rpc anvil_setStorageAt "$CBETH" "$CB_SLOT" "$(cast to-uint256 "$CAP")" --rpc-url "$RPC" >/dev/null
NOW=$(cast block latest -f timestamp --rpc-url "$RPC")
cast send --private-key "$OKEY" "$PROXY" 'grantAgent(address,uint64,uint32,uint8,address[],uint256[])' \
  "$AGENT" "$((NOW + 86400))" 604800 1 "[$WETH,$CBETH,$USDC]" "[$CAP,$CAP,0]" --rpc-url "$RPC" >/dev/null
for token in "$WETH" "$CBETH"; do
  cast send --private-key "$OKEY" "$token" 'approve(address,uint256)' "$PROXY" "$CAP" --rpc-url "$RPC" >/dev/null
done
export AGENTSWAP_RPC_URL_8453=$RPC
BEFORE=$(cast call "$USDC" 'balanceOf(address)(uint256)' "$OWNER" --rpc-url "$RPC" | awk '{print $1}')
"$BIN" --allow-trade sweep --chainid 8453 --proxy "$PROXY" --key-file "$WORK/agent.key" \
  --receive USDC --max-usd 100 --max-loss-bps 500 --self-submit --lookback-blocks 100 --json > "$WORK/first.json"
python3 - "$WORK/first.json" <<'PY'
import json,sys
rows=json.load(open(sys.argv[1]))['tokens']
assert len(rows)==3,rows
sold=[r for r in rows if r['outcome']=='sold']
assert len(sold)==2,rows
assert all(r['tx_status']=='confirmed' and r['tx_hash'] for r in sold),rows
print('PASS sweep sells two tokens with confirmed receipts')
PY
AFTER=$(cast call "$USDC" 'balanceOf(address)(uint256)' "$OWNER" --rpc-url "$RPC" | awk '{print $1}')
[ "$AFTER" -gt "$BEFORE" ]
echo 'PASS USDC arrived at owner'
for token in "$WETH" "$CBETH"; do
  allowance=$(cast call "$token" 'allowance(address,address)(uint256)' "$OWNER" "$PROXY" --rpc-url "$RPC" | awk '{print $1}')
  [ "$allowance" = 0 ]
done
echo 'PASS exact allowances consumed'
"$BIN" --allow-trade sweep --chainid 8453 --proxy "$PROXY" --key-file "$WORK/agent.key" \
  --receive USDC --max-usd 100 --max-loss-bps 500 --self-submit --lookback-blocks 100 --json > "$WORK/second.json"
python3 - "$WORK/second.json" <<'PY'
import json,sys
rows=json.load(open(sys.argv[1]))['tokens']
assert len(rows)==3 and all(r['outcome']=='skipped' and r['reason']=='zero' for r in rows),rows
print('PASS second sweep sells nothing')
PY
# A large cbETH holding probes price impact on thinner liquidity at a zero-loss floor.
THIN_CAP=100000000000000000000
cast rpc anvil_setStorageAt "$CBETH" "$CB_SLOT" "$(cast to-uint256 "$THIN_CAP")" --rpc-url "$RPC" >/dev/null
NOW=$(cast block latest -f timestamp --rpc-url "$RPC")
cast send --private-key "$OKEY" "$PROXY" 'grantAgent(address,uint64,uint32,uint8,address[],uint256[])' \
  "$AGENT" "$((NOW + 86400))" 604800 1 "[$CBETH,$USDC]" "[$THIN_CAP,0]" --rpc-url "$RPC" >/dev/null
cast send --private-key "$OKEY" "$CBETH" 'approve(address,uint256)' "$PROXY" "$THIN_CAP" --rpc-url "$RPC" >/dev/null
"$BIN" sweep --chainid 8453 --proxy "$PROXY" --key-file "$WORK/agent.key" \
  --receive USDC --max-usd 10000000 --max-loss-bps 0 --dry-run --lookback-blocks 100 --json > "$WORK/thin.json"
python3 - "$WORK/thin.json" <<'PY'
import json,sys
rows=json.load(open(sys.argv[1]))['tokens']
assert any(r['reason']=='below_floor' for r in rows),rows
assert all(r['tx_hash'] is None for r in rows),rows
print('PASS thin-token zero-loss quote is below_floor without broadcast')
PY
