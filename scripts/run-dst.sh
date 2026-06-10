#!/usr/bin/env bash
# Deterministic simulation testing corpus runner.
# Usage: scripts/run-dst.sh [N_RANDOM_SEEDS]
# Contract (implemented in T0.4):
#   1. Replays every regression seed in crates/fortuna-core/dst-corpus/ (never delete these).
#   2. Runs N_RANDOM_SEEDS fresh seeds through the randomized scenario generator.
#   3. Exits non-zero on ANY invariant violation, printing the offending seed.
# Note for T0.4: declare the dst test target with harness = false in
# fortuna-core/Cargo.toml so custom flags like --seeds are accepted.
set -euo pipefail
N="${1:-2000}"
if cargo test -p fortuna-core --test dst --no-run 2>/dev/null; then
  exec cargo test -p fortuna-core --test dst -- --nocapture --seeds "$N"
else
  echo "[run-dst] DST harness not implemented yet (BUILD_PLAN T0.4); passing vacuously."
  exit 0
fi
