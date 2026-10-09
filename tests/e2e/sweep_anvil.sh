#!/usr/bin/env bash
# Grant-bounded sweep on a Base or BNB Smart Chain fork with fresh wallets and exact approvals.
# Real app prices and quote routes; all storage changes and transactions target local anvil.
set -euo pipefail
BIN=${BIN:?set BIN to the agentswap binary}
CHAIN=${CHAIN:-8453}
case "$CHAIN" in
  8453) FORK_URL=${FORK_URL:-https://mainnet.base.org} ;;
  56) FORK_URL=${FORK_URL:?set FORK_URL to a keyed BNB Smart Chain RPC} ;;
  *) echo 'FAIL CHAIN must be 8453 or 56'; exit 1 ;;
esac
PORT=${PORT:-28549}
RPC=http://127.0.0.1:$PORT
SRC=$(cd "$(dirname "$0")/../.." && pwd)
WORK=$(mktemp -d)
APID=
trap 'if [ -n "$APID" ]; then kill "$APID" 2>/dev/null || true; fi; rm -rf "$WORK"' EXIT
read -r WRAPPED SECOND RECEIVE FACTORY SECOND_SLOT < <(python3 - "$SRC" "$CHAIN" <<'PYTOKENS'
import json,pathlib,re,subprocess,sys
root=pathlib.Path(sys.argv[1])
f=re.search(r'const FACTORY:.*address!\("([^"]+)"', (root/'src/evm.rs').read_text())[1]
if sys.argv[2] == '56':
    tokens=[]
    for key in ['token.wbnb@bsc','token.eth@bsc','token.usdt@bsc']:
        fact=json.loads(subprocess.check_output(['fact','get',key,'--json']))
        assert fact['status']=='confirmed',fact['key']
        tokens.append(fact['value'])
    # crpc balance-slot on Binance-Peg ETH matched the smoke owner's nonzero
    # balance (1943168684619350) to mapping slot 1 on BNB Smart Chain.
    print(*tokens,f,1)
else:
    s=(root/'src/tokens/registry/base.rs').read_text()
    a={symbol:address for address,symbol in re.findall(r'address: "([^"]+)",\s*symbol: "([^"]+)"',s)}
    # crpc balance-slot matched cbETH's nonzero self-balance to mapping slot 51.
    print(a['WETH'],a['cbETH'],a['USDC'],f,51)
PYTOKENS
)
if cast chain-id --rpc-url "$RPC" >/dev/null 2>&1; then echo 'FAIL fork port occupied'; exit 1; fi
anvil --fork-url "$FORK_URL" --port "$PORT" >"$WORK/anvil.log" 2>&1 & APID=$!
for _ in $(seq 1 120); do
  if cast chain-id --rpc-url "$RPC" >/dev/null 2>&1; then break; fi
  if ! kill -0 "$APID" 2>/dev/null; then cat "$WORK/anvil.log"; exit 1; fi
  sleep 1
done
[ "$(cast chain-id --rpc-url "$RPC")" = "$CHAIN" ]
read -r OKEY AKEY <<< "$(python3 -c 'import secrets; print("0x" + secrets.token_hex(32), "0x" + secrets.token_hex(32))')"
OWNER=$(cast wallet address --private-key "$OKEY")
AGENT=$(cast wallet address --private-key "$AKEY")
printf '%s' "$AKEY" > "$WORK/agent.key"
for who in "$OWNER" "$AGENT"; do cast rpc anvil_setBalance "$who" "$(cast to-hex 100000000000000000000)" --rpc-url "$RPC" >/dev/null; done
cast send --private-key "$OKEY" "$FACTORY" 'deploy(address)' "$OWNER" --rpc-url "$RPC" >/dev/null
PROXY=$(cast call "$FACTORY" 'proxyOf(address)(address)' "$OWNER" --rpc-url "$RPC")
BALANCE_SLOT=$(cast index address "$OWNER" "$SECOND_SLOT")
CAP=1000000000000000
cast send --private-key "$OKEY" "$WRAPPED" 'deposit()' --value "$CAP" --rpc-url "$RPC" >/dev/null
cast rpc anvil_setStorageAt "$SECOND" "$BALANCE_SLOT" "$(cast to-uint256 "$CAP")" --rpc-url "$RPC" >/dev/null
[ "$(cast call "$SECOND" 'balanceOf(address)(uint256)' "$OWNER" --rpc-url "$RPC" | awk '{print $1}')" = "$CAP" ]
NOW=$(cast block latest -f timestamp --rpc-url "$RPC")
cast send --private-key "$OKEY" "$PROXY" 'grantAgent(address,uint64,uint32,uint8,address[],uint256[])' \
  "$AGENT" "$((NOW + 86400))" 604800 1 "[$WRAPPED,$SECOND,$RECEIVE]" "[$CAP,$CAP,0]" --rpc-url "$RPC" >/dev/null
for token in "$WRAPPED" "$SECOND"; do
  cast send --private-key "$OKEY" "$token" 'approve(address,uint256)' "$PROXY" "$CAP" --rpc-url "$RPC" >/dev/null
done
export "AGENTSWAP_RPC_URL_$CHAIN=$RPC"
BEFORE=$(cast call "$RECEIVE" 'balanceOf(address)(uint256)' "$OWNER" --rpc-url "$RPC" | awk '{print $1}')
"$BIN" --allow-trade sweep --chainid "$CHAIN" --proxy "$PROXY" --key-file "$WORK/agent.key" \
  --receive "$RECEIVE" --token "$WRAPPED" --token "$SECOND" --max-usd 100 --max-loss-bps 500 --self-submit --json > "$WORK/first.json" || { code=$?; cat "$WORK/first.json"; exit "$code"; }
python3 - "$WORK/first.json" <<'PY'
import json,sys
rows=json.load(open(sys.argv[1]))['tokens']
assert len(rows)==3,rows
sold=[r for r in rows if r['outcome']=='sold']
assert len(sold)==2,rows
assert all(r['tx_status']=='confirmed' and r['tx_hash'] for r in sold),rows
print('PASS sweep sells two tokens with confirmed receipts')
PY
AFTER=$(cast call "$RECEIVE" 'balanceOf(address)(uint256)' "$OWNER" --rpc-url "$RPC" | awk '{print $1}')
[ "$AFTER" -gt "$BEFORE" ]
echo 'PASS receive token arrived at owner'
for token in "$WRAPPED" "$SECOND"; do
  allowance=$(cast call "$token" 'allowance(address,address)(uint256)' "$OWNER" "$PROXY" --rpc-url "$RPC" | awk '{print $1}')
  [ "$allowance" = 0 ]
done
echo 'PASS exact allowances consumed'
"$BIN" --allow-trade sweep --chainid "$CHAIN" --proxy "$PROXY" --key-file "$WORK/agent.key" \
  --receive "$RECEIVE" --token "$WRAPPED" --token "$SECOND" --max-usd 100 --max-loss-bps 500 --self-submit --json > "$WORK/second.json" || { code=$?; cat "$WORK/second.json"; exit "$code"; }
python3 - "$WORK/second.json" <<'PY'
import json,sys
rows=json.load(open(sys.argv[1]))['tokens']
assert len(rows)==3 and all(r['outcome']=='skipped' and r['reason'] in ('zero','receive_token') for r in rows),rows
print('PASS second sweep sells nothing')
PY
if [ "$CHAIN" = 8453 ]; then
# A large cbETH holding probes price impact on thinner liquidity at a zero-loss floor.
THIN_CAP=100000000000000000000
cast rpc anvil_setStorageAt "$SECOND" "$BALANCE_SLOT" "$(cast to-uint256 "$THIN_CAP")" --rpc-url "$RPC" >/dev/null
NOW=$(cast block latest -f timestamp --rpc-url "$RPC")
cast send --private-key "$OKEY" "$PROXY" 'grantAgent(address,uint64,uint32,uint8,address[],uint256[])' \
  "$AGENT" "$((NOW + 86400))" 604800 1 "[$SECOND,$RECEIVE]" "[$THIN_CAP,0]" --rpc-url "$RPC" >/dev/null
cast send --private-key "$OKEY" "$SECOND" 'approve(address,uint256)' "$PROXY" "$THIN_CAP" --rpc-url "$RPC" >/dev/null
"$BIN" sweep --chainid "$CHAIN" --proxy "$PROXY" --key-file "$WORK/agent.key" \
  --receive "$RECEIVE" --token "$SECOND" --max-usd 10000000 --max-loss-bps 0 --dry-run --json > "$WORK/thin.json" || { code=$?; cat "$WORK/thin.json"; exit "$code"; }
python3 - "$WORK/thin.json" <<'PY'
import json,sys
rows=json.load(open(sys.argv[1]))['tokens']
assert any(r['reason']=='below_floor' for r in rows),rows
assert all(r['tx_hash'] is None for r in rows),rows
print('PASS thin-token zero-loss quote is below_floor without broadcast')
PY
fi
