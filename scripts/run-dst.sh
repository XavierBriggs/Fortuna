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
#   5. Runs the perp margin/funding/liquidation DST (funding-tick chaos,
#      liquidation under ack-delay/api-error, system-fill ingestion,
#      margin-call sequences, demo-divergence — per-arm accounted,
#      crates/fortuna-state/tests/perp_dst.rs) with the same seed count (T5.B6).
#   6. Exits non-zero on ANY invariant violation OR build failure, printing the
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
# T5.B6: the perp margin/funding/liquidation plane under seeded chaos.
PERP_DST_SCENARIOS="$N" cargo test -p fortuna-state --test perp_dst -- --nocapture
# Track E E.3c: the persona runner under the cost budget + chaos mind (budget
# throttle, signal absence, schema-invalid findings, coalesced re-triggers).
PERSONA_DST_SCENARIOS="$N" cargo test -p fortuna-cognition --test persona_dst -- --nocapture
# T4.1 req 10: the daemon-composition smoke (boot -> ticks -> stop signal
# -> graceful shutdown, deterministic under SimClock, vs the example config).
cargo test -p fortuna-live --test daemon_smoke -- --nocapture
