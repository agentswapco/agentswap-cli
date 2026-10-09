#!/usr/bin/env bash
# Tier 3 sweep verification through the supplied remote Cargo shim.
# Run on the dispatch host; aid parses remote output and the fork runs via rbox.
set -euo pipefail
export AID_BUILD_BOX=${AID_BUILD_BOX_OVERRIDE:-grok-bot-kestrel}
export CARGO_BUILD_JOBS=4
aid build check -p agentswap
aid test -p agentswap
cargo build -p agentswap -j4
REMOTE_DIR="~/.rbox/work/cli-sweep-$(git rev-parse --short HEAD)"
rbox ensure "$AID_BUILD_BOX"
rbox sync --to "$REMOTE_DIR" --lock-timeout 300 "$AID_BUILD_BOX" .
JOB=$(rbox run "$AID_BUILD_BOX" --dir "$REMOTE_DIR" --lock-timeout 300 -- bash -lc \
  'BIN="${CARGO_TARGET_DIR:?}/debug/agentswap" bash tests/e2e/sweep_anvil.sh')
status=0
rbox wait "$AID_BUILD_BOX" "$JOB" || status=$?
rbox log "$AID_BUILD_BOX" "$JOB"
exit "$status"
