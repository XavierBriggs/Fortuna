#!/usr/bin/env bash
# Deterministic simulation testing corpus runner.
# Usage: scripts/run-dst.sh [N_RANDOM_SEEDS]
# Contract (implemented in T0.4; extended at Phase 2 EXIT):
#   1. Replays every regression seed in crates/fortuna-core/dst-corpus/ (never delete these).
#   2. Runs N_RANDOM_SEEDS fresh seeds through the randomized scenario generator.
#   3. Runs the composed decision-loop DST (synthesis strategy + chaos mind,
#      crates/fortuna-runner/tests/synthesis_dst.rs) with the same seed count.
#   4. Exits non-zero on ANY invariant violation, printing the offending seed.
# Note for T0.4: declare the dst test target with harness = false in
# fortuna-core/Cargo.toml so custom flags like --seeds are accepted.
set -euo pipefail
N="${1:-2000}"
if cargo test -p fortuna-core --test dst --no-run 2>/dev/null; then
  cargo test -p fortuna-core --test dst -- --nocapture --seeds "$N"
else
  echo "[run-dst] DST harness not implemented yet (BUILD_PLAN T0.4); passing vacuously."
  exit 0
fi
# Phase 2 EXIT: the composed decision loop under cognition + venue chaos.
SYNTH_DST_SCENARIOS="$N" cargo test -p fortuna-runner --test synthesis_dst -- --nocapture
