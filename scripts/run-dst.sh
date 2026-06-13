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
#   5b. Runs the funding_forecast belief-producer DST (PerpTick chaos: time
#      gaps, window rolls, estimate oscillation, ±clamp extremes — per-arm
#      accounted; zero-proposals + every-draft-validates + determinism,
#      crates/fortuna-runner/tests/funding_forecast_dst.rs) with the same seed
#      count (T5.B7 slice 2b).
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
# T5.B7 slice 2b: the funding_forecast belief-producer under PerpTick chaos.
FUNDING_FORECAST_DST_SCENARIOS="$N" cargo test -p fortuna-runner --test funding_forecast_dst -- --nocapture
# Track E E.3c: the persona runner under the cost budget + chaos mind (budget
# throttle, signal absence, schema-invalid findings, coalesced re-triggers).
PERSONA_DST_SCENARIOS="$N" cargo test -p fortuna-cognition --test persona_dst -- --nocapture
# Track E (persona live-loop brain): the run_due_personas orchestrator under a
# seeded tick — signal fan-out across stations/dates/read+unread kinds, random
# (possibly pre-exhausted) budget, random cadence/debounce. Asserts no panic,
# at-most-one-run-per-(persona,region) coalescing, no phantom regions, budget
# throttle, and byte-identical determinism on replay (crates/fortuna-cognition/
# tests/persona_orchestrator_dst.rs).
PERSONA_ORCH_DST_SCENARIOS="$N" cargo test -p fortuna-cognition --test persona_orchestrator_dst -- --nocapture
# T4.1 req 10: the daemon-composition smoke (boot -> ticks -> stop signal
# -> graceful shutdown, deterministic under SimClock, vs the example config).
cargo test -p fortuna-live --test daemon_smoke -- --nocapture
# D9: the ingestion-scheduler DST — the five enumerated failure scenarios
# (timeout, 429 storm, crash+rebuild, burst/volume-cap, quarantine+rearm),
# each deterministic under SimClock, with the Layer-1 validator on the live
# refuse-and-quarantine path (crates/fortuna-sources/tests/ingest_dst.rs).
cargo test -p fortuna-sources --test ingest_dst -- --nocapture
