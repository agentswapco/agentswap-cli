#!/usr/bin/env bash
# Remote Tier 3 sweep verification, serialized with four compiler jobs.
# Uses aid diagnostics and the caller's existing shared Cargo target directory.
set -euo pipefail
export CARGO_BUILD_JOBS=4
aid build check -p agentswap
aid test -p agentswap
cargo build -p agentswap -j4
BIN="${CARGO_TARGET_DIR:-target}/debug/agentswap" bash tests/e2e/sweep_anvil.sh
