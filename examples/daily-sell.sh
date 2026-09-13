#!/usr/bin/env bash
# Preview a fraction-of-balance sale through a decaying intent, or relay with --live.
# Usage: daily-sell.sh --owner ADDRESS --agent ADDRESS --token TOKEN --quote-token TOKEN.
# Deps: agentswap, bash, curl, bc, jq; AGENTSWAP_KEY_FILE selects the agent key.
set -euo pipefail
export LC_ALL=C
trap 'printf "daily-sell: failed at line %s (exit %s)\n" "$LINENO" "$?" >&2' ERR

fail() {
    printf 'daily-sell: %s\n' "$*" >&2
    exit 1
}

calculate() {
    local result
    result=$(BC_LINE_LENGTH=0 bc <<< "scale=0; $1" 2>&1) || fail "integer calculation failed: $result"
    result=${result//$'\\\n'/}
    [[ $result =~ ^-?[0-9]+$ ]] || fail "invalid integer calculation: $result"
    printf '%s\n' "$result"
}

resolve_token() {
    jq -er --arg token "$1" --arg chain "$chainid" '
        to_entries | map(select(
            (.value.chain_id | tostring) == $chain and
            ((.key | ascii_downcase) == ($token | ascii_downcase) or
             (.value.symbol | ascii_downcase) == ($token | ascii_downcase)))) |
        if length != 1 then error("token must resolve uniquely on the selected chain")
        else .[0] end |
        if (.key | test("^0x[0-9a-f]{40}$")) and
           (.value.decimals | type == "number" and . >= 0 and . <= 255 and floor == .)
        then [.key, .value.decimals, .value.symbol] | @tsv
        else error("invalid token address or decimals") end
    ' <<< "$tokens"
}

assert_place() {
    jq -e --arg agent "$agent" --arg amount "$amount" \
        --arg start "$start_out" --arg end "$end_out" --argjson live "$live" '
        (.digest | test("^0x[0-9a-f]{64}$")) and
        (.id | test("^0x[0-9a-f]{64}$")) and
        ((.authorization.agent | ascii_downcase) == ($agent | ascii_downcase)) and
        .order.amountIn == $amount and .order.startAmountOut == $start and
        .order.endAmountOut == $end and
        (if $live then
            .dry_run == false and
            (.signature | test("^0x[0-9a-fA-F]{130}$")) and
            (.envelope | test("^0x([0-9a-fA-F]{2})+$")) and .relay != null
         else .dry_run == true and (has("signature") | not) and (has("envelope") | not)
         end)
    ' <<< "$placement" >/dev/null || fail 'intent JSON assertions failed'
}

chainid=8453
owner='' agent='' token='' quote_token=''
fraction_bps=1000 start_bps=100 end_bps=-20 decay_secs=300 duration_secs=600
live=false
while (($#)); do
    case "$1" in
        --live) live=true; shift; continue ;;
        --help|-h)
            cat <<'USAGE'
Usage: daily-sell.sh --owner ADDRESS --agent ADDRESS --token TOKEN --quote-token TOKEN
Example parameters: --chainid 8453 --fraction-bps 1000 --start-bps 100 --end-bps -20
                    --decay-secs 300 --duration-secs 600
TOKEN is a registry symbol or address. Amounts use unsigned integer raw token units.
Fraction must be 1..10000 bps; output multipliers and durations must be positive.
AGENTSWAP_KEY_FILE is required; --agent must be its address. AGENTSWAP_BIN defaults
to agentswap. Set AGENTSWAP_RPC_URL_<chainid> or AGENTSWAP_RPC_URL; only chain 8453
has a script fallback (https://mainnet.base.org). Requires bash, curl, bc and jq.
Default: unsigned dry-run ending at the authorization digest. --live passes
--allow-trade --relay and requires sufficient budget and on-chain authorization.
USAGE
            exit 0 ;;
        --chainid|--owner|--agent|--token|--quote-token|--fraction-bps|--start-bps|--end-bps|--decay-secs|--duration-secs)
            (($# >= 2)) || fail "missing value for $1"
            [[ -n $2 && $2 != --* ]] || fail "missing value for $1" ;;
        *) fail "unknown argument: $1" ;;
    esac
    case "$1" in
        --chainid) chainid=$2 ;;
        --owner) owner=$2 ;;
        --agent) agent=$2 ;;
        --token) token=$2 ;;
        --quote-token) quote_token=$2 ;;
        --fraction-bps) fraction_bps=$2 ;;
        --start-bps) start_bps=$2 ;;
        --end-bps) end_bps=$2 ;;
        --decay-secs) decay_secs=$2 ;;
        --duration-secs) duration_secs=$2 ;;
    esac
    shift 2
done

for dependency in curl bc jq "${AGENTSWAP_BIN:-agentswap}"; do
    command -v "$dependency" >/dev/null || fail "missing executable: $dependency"
done
[[ -n ${AGENTSWAP_KEY_FILE:-} && -r ${AGENTSWAP_KEY_FILE:-} ]] ||
    fail 'AGENTSWAP_KEY_FILE must name a readable key file'
[[ $owner =~ ^0x[0-9a-fA-F]{40}$ ]] || fail '--owner must be an address'
[[ $agent =~ ^0x[0-9a-fA-F]{40}$ ]] || fail '--agent must be an address'
[[ -n $token && -n $quote_token ]] || fail '--token and --quote-token are required'
for parameter in chainid fraction_bps decay_secs duration_secs; do
    [[ ${!parameter} =~ ^[1-9][0-9]*$ ]] || fail "$parameter must be a positive integer"
done
for parameter in start_bps end_bps; do
    [[ ${!parameter} =~ ^-?(0|[1-9][0-9]*)$ ]] || fail "$parameter must be integer bps"
    [[ $(calculate "10000 + ${!parameter} > 0") == 1 ]] ||
        fail "$parameter must be greater than -10000"
done
[[ $(calculate "$fraction_bps <= 10000") == 1 ]] || fail 'fraction-bps must be at most 10000'

rpc_variable=AGENTSWAP_RPC_URL_$chainid
rpc_url=${!rpc_variable:-${AGENTSWAP_RPC_URL:-}}
if [[ -z $rpc_url ]]; then
    [[ $chainid == 8453 ]] || fail "set $rpc_variable or AGENTSWAP_RPC_URL for the balance read"
    rpc_url=https://mainnet.base.org
fi
export "$rpc_variable=$rpc_url"
cli=("${AGENTSWAP_BIN:-agentswap}" -j --key-file "$AGENTSWAP_KEY_FILE")

tokens=$("${cli[@]}" tokens --chainid "$chainid")
resolved=$(resolve_token "$token")
read -r token_address token_decimals token_symbol <<< "$resolved"
resolved=$(resolve_token "$quote_token")
read -r quote_address quote_decimals quote_symbol <<< "$resolved"
printf 'Token: %s %s (decimals %s)\n' "$token_symbol" "$token_address" "$token_decimals"
printf 'Quote token: %s %s (decimals %s)\n' "$quote_symbol" "$quote_address" "$quote_decimals"

calldata=$(printf '0x70a08231%064s' "${owner:2}" | tr ' ' 0)
request=$(jq -nc --arg token "$token_address" --arg data "$calldata" \
    '{jsonrpc:"2.0",id:1,method:"eth_call",params:[{to:$token,data:$data},"latest"]}')
response=$(curl --fail-with-body --silent --show-error --connect-timeout 15 --max-time 60 \
    -H 'Content-Type: application/json' --data "$request" "$rpc_url")
balance_hex=$(jq -er '
    if .error != null then error("balance RPC error: " + (.error | tojson))
    elif .jsonrpc == "2.0" and .id == 1 and (.result | test("^0x[0-9a-fA-F]{64}$"))
    then .result else error("invalid balance RPC result") end
' <<< "$response")
balance_hex=$(printf '%s' "${balance_hex:2}" | tr '[:lower:]' '[:upper:]')
balance=$(calculate "ibase=16; $balance_hex")
amount=$(calculate "$balance * $fraction_bps / 10000")
[[ $amount != 0 ]] || fail 'fraction of balance rounds down to zero'

policy=$("${cli[@]}" policy --chainid "$chainid" --owner "$owner" --agent "$agent" \
    --token "$token_address" --lookback-blocks 1800)
budget=$(jq -er --arg token "$token_address" '
    .tokens | map(select((.token | ascii_downcase) == $token)) |
    if length != 1 then error("expected one token budget") else .[0] end |
    if (.cap | type == "string" and test("^[0-9]+$")) and
       (.used | type == "string" and test("^[0-9]+$"))
    then [.cap, .used] | @tsv else error("invalid cap or used amount") end
' <<< "$policy")
read -r cap used <<< "$budget"
remaining=$(calculate "$cap - $used")
printf 'Budget: cap=%s used=%s remaining=%s\n' "$cap" "$used" "$remaining"
if [[ $(calculate "$remaining < $amount") == 1 ]]; then
    [[ $live == false ]] || fail "remaining budget $remaining is below amount $amount; refusing live placement"
    printf 'Dry-run: intent would not be authorized live; insufficient budget. Previewing the fraction amount.\n'
fi

quote=$("${cli[@]}" quote --chainid "$chainid" --from "$token_address" \
    --to "$quote_address" --amount "$amount")
amount_out=$(jq -er '.output | if type == "string" and test("^[0-9]+$")
    then . else error("quote output must be an unsigned integer string") end' <<< "$quote")
start_out=$(calculate "$amount_out * (10000 + $start_bps) / 10000")
end_out=$(calculate "$amount_out * (10000 + $end_bps) / 10000")
[[ $end_out != 0 ]] || fail 'end output rounds down to zero'
[[ $(calculate "$start_out >= $end_out") == 1 ]] || fail 'start output is below end output'

place_flags=(--dry-run)
if [[ $live == true ]]; then
    place_flags=(--allow-trade --relay)
fi
placement=$("${cli[@]}" intent place --chainid "$chainid" --proxy-owner "$owner" \
    --from "$token_address" --to "$quote_address" --amount "$amount" \
    --start-out "$start_out" --end-out "$end_out" --decay-secs "$decay_secs" \
    --duration-secs "$duration_secs" "${place_flags[@]}")
assert_place
intent_id=$(jq -er '.id' <<< "$placement")
digest=$(jq -er '.digest' <<< "$placement")
printf 'Balance: %s\nAmount: %s\nQuote: %s\nStart output: %s\nEnd output: %s\n' \
    "$balance" "$amount" "$amount_out" "$start_out" "$end_out"
printf 'Intent id: %s\nDigest: %s\n' "$intent_id" "$digest"
