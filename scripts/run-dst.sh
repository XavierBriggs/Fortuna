#!/usr/bin/env bash
# Deterministic simulation testing corpus runner.
# Usage: scripts/run-dst.sh [N_RANDOM_SEEDS]
# Contract (implemented in T0.4; extended at Phase 2 EXIT and E4):
#   1. Replays every regression seed in crates/fortuna-core/dst-corpus/ (never delete these).
#   2. Runs N_RANDOM_SEEDS fresh seeds through the randomized scenario generator.
#   3. Runs the composed decision-loop DST (synthesis strategy + chaos mind,
#      crates/fortuna-runner/tests/synthesis_dst.rs) with the same seed count.
#   4. Runs the settlement/watchdog DST (discrepancies, halts, reversals, voids,
#      disputes, overdue, orphans, divergence, audit death — per-arm accounted,
#      crates/fortuna-runner/tests/settlement_dst.rs) with the same seed count.
#   5. Exits non-zero on ANY invariant violation OR build failure, printing the
#      offending seed. A harness that fails to BUILD fails the battery (E5:
#      the old "passing vacuously" escape is gone — the harness exists).
set -euo pipefail
N="${1:-2000}"
cargo test -p fortuna-core --test dst --no-run
cargo test -p fortuna-core --test dst -- --nocapture --seeds "$N"
# Phase 2 EXIT: the composed decision loop under cognition + venue chaos.
SYNTH_DST_SCENARIOS="$N" cargo test -p fortuna-runner --test synthesis_dst -- --nocapture
# E4: the settlement/watchdog plane under seeded chaos.
SETTLE_DST_SCENARIOS="$N" cargo test -p fortuna-runner --test settlement_dst -- --nocapture
