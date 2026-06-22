#!/usr/bin/env bash
# WS4 live smoke — the boundary live gate for the WS4 Demo Surface.
#
# Verifies the demo's end-to-end behavior in CI-deterministic form:
#
#   1. Creates + migrates a temp demo Postgres DB (superuser socket at /tmp,
#      the ledger-gate-DB recipe — no Postgres user/password needed).
#   2. Runs `fortuna doctor --offline` against the migrated DB → asserts exit 0
#      with "READY" or all-green indicator (CI-deterministic: no network calls).
#   3. Exercises the head-to-head with discovery co-active (the W6a edge_id fix,
#      end-to-end) via the focused Rust integration tests:
#        a. `persona_and_discovery_edge_ids_are_disjoint_for_the_same_seq`
#           (daemon.rs inline test) — asserts 01EDP vs 01EDG prefix disjointness
#           (the fix that prevents persona CLV from being silently dropped when
#           discovery + personas co-run in one drive()).
#        b. `meteorologist_belief_gets_nonnull_clv` (fortuna-live/tests/persona_clv.rs)
#           — asserts the meteorologist belief gets a non-null clv_bps EQUAL to
#           Aeolus's after the W5 threshold-match edge join (market-level honesty).
#
# Why tests, not a live daemon boot:
# A deterministic Rust test is more reliable than booting the full daemon with
# real Kalshi credentials (unavailable in CI). The tests use a real DB (sqlx::test
# auto-creates + migrates + drops), exercise the exact code path the daemon uses,
# and fire on any regression. The briefed preferred form ("deterministic test
# invocation over a flaky server boot").
#
# Requires: a superuser Postgres socket at /tmp (e.g., `initdb`, `pg_ctl start`
#           with a unix socket at /tmp). The SQLX_OFFLINE=true flag lets the
#           compile phase run without a live query-time connection; the test
#           runtime connects to the /tmp socket for the sqlx::test DB.
#
# Print "ws4-live-smoke: ok" on success; exit non-zero on any failure.
# NEVER fakes green: every assertion must pass, or the script exits immediately.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

SMOKE_DB="fortuna_ws4_livegate"

# ---------------------------------------------------------------------------
# 1. Create the smoke DB (no explicit migrate step needed)
# ---------------------------------------------------------------------------
# fortuna_ledger::connect() auto-runs embedded migrations on first connect, and
# `fortuna doctor --offline` exercises that path — its `migrations_applied` check
# then verifies the schema is current. sqlx::test DB handles this independently
# for the Rust integration tests. An explicit migrate step here would require a
# fortuna-cli binary or sqlx-cli to be present and is dead theater.
echo "ws4-live-smoke: creating temp DB $SMOKE_DB ..."
psql "postgres:///postgres?host=/tmp" -tAc "DROP DATABASE IF EXISTS $SMOKE_DB;" >/dev/null
psql "postgres:///postgres?host=/tmp" -tAc "CREATE DATABASE $SMOKE_DB;" >/dev/null

export SQLX_OFFLINE=true
export DATABASE_URL="postgres:///$SMOKE_DB?host=/tmp"

# ---------------------------------------------------------------------------
# 2. fortuna doctor --offline → assert all-green
# ---------------------------------------------------------------------------
echo "ws4-live-smoke: running fortuna doctor --offline ..."

# Build the doctor binary (dev profile for speed)
cargo build -q -p fortuna-cli 2>&1 | tail -2

# Supply minimal required env vars for the env_creds check (presence check only;
# values are sentinel strings — no real secrets needed).
DOCTOR_ENV="DATABASE_URL=$DATABASE_URL"
DOCTOR_ENV="$DOCTOR_ENV FORTUNA_SLACK_BOT_TOKEN=xoxb-ws4-smoke-sentinel"
DOCTOR_ENV="$DOCTOR_ENV FORTUNA_DEADMAN_URL=https://hc-ping.example.com/ws4-smoke"
DOCTOR_ENV="$DOCTOR_ENV FORTUNA_SLACK_CHANNEL_TRADING=CTRADING0001"
DOCTOR_ENV="$DOCTOR_ENV FORTUNA_SLACK_CHANNEL_ALERTS=CALERTS0001"
DOCTOR_ENV="$DOCTOR_ENV FORTUNA_SLACK_CHANNEL_REVIEW=CREVIEW0001"
DOCTOR_ENV="$DOCTOR_ENV FORTUNA_SLACK_CHANNEL_DIGEST=CDIGEST0001"
DOCTOR_ENV="$DOCTOR_ENV FORTUNA_SLACK_CHANNEL_OPS=COPS000001"

# FIX: capture output AND handle non-zero exit explicitly so the diagnostic
# message actually prints on a doctor-red (under set -e the old pattern was
# dead — the subshell abort happened before DOCTOR_EXIT was captured).
if ! DOCTOR_OUT="$(env $DOCTOR_ENV ./target/debug/fortuna doctor --offline 2>&1)"; then
    echo "$DOCTOR_OUT"
    echo "ws4-live-smoke FAIL: fortuna doctor --offline exited non-zero (expected 0)"
    exit 1
fi
echo "$DOCTOR_OUT"

# Assert the output contains "READY" or an all-green marker
if echo "$DOCTOR_OUT" | grep -qiE "READY|all.*green|all checks"; then
    echo "ws4-live-smoke: doctor --offline PASS (exit 0, READY)"
else
    echo "ws4-live-smoke FAIL: doctor --offline exit 0 but output did not contain READY/all-green marker"
    echo "$DOCTOR_OUT"
    exit 1
fi

# ---------------------------------------------------------------------------
# 3a. Edge-ID disjointness test (persona + discovery co-run fix, W6a)
# ---------------------------------------------------------------------------
# NOTE: this test lives in the fortuna-live LIB (daemon.rs #[cfg(test)] block),
# not in a separate test file — requires `--lib` to avoid the integration-test
# runner (which would filter it out with 0 matches).
echo "ws4-live-smoke: running persona_and_discovery_edge_ids_are_disjoint_for_the_same_seq ..."

# FIX: capture output and assert exactly "1 passed". cargo test exits 0 even
# when 0 tests match the filter ("0 filtered out"), so piping to tail hid the
# silent-pass-if-test-renamed bug. Use --exact to pin the name, then verify the
# run count from the output.
EDGE_ID_OUT="$(cargo test \
    -p fortuna-live \
    --lib \
    -- \
    --exact "daemon::tests::persona_and_discovery_edge_ids_are_disjoint_for_the_same_seq" \
    --test-threads=1 \
    2>&1)"
echo "$EDGE_ID_OUT" | tail -5
if ! echo "$EDGE_ID_OUT" | grep -qE "^test result: ok\. 1 passed"; then
    echo "ws4-live-smoke FAIL: expected exactly 1 passed for edge-ID disjointness test (test renamed/deleted?)"
    echo "$EDGE_ID_OUT"
    exit 1
fi

echo "ws4-live-smoke: edge-ID disjointness test PASS"

# ---------------------------------------------------------------------------
# 3b. Meteorologist CLV non-null with discovery co-active (W5+W6a persona CLV)
# ---------------------------------------------------------------------------
echo "ws4-live-smoke: running meteorologist_belief_gets_nonnull_clv (persona_clv) ..."

# FIX: same pattern — capture output and assert exactly "1 passed" so that
# a renamed or deleted test name causes a gate failure rather than a silent pass.
CLV_OUT="$(cargo test \
    -p fortuna-live \
    --test persona_clv \
    -- \
    --exact "meteorologist_belief_gets_nonnull_clv" \
    --test-threads=1 \
    2>&1)"
echo "$CLV_OUT" | tail -5
if ! echo "$CLV_OUT" | grep -qE "^test result: ok\. 1 passed"; then
    echo "ws4-live-smoke FAIL: expected exactly 1 passed for persona CLV test (test renamed/deleted?)"
    echo "$CLV_OUT"
    exit 1
fi

echo "ws4-live-smoke: persona CLV non-null test PASS"

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
echo ""
echo "ws4-live-smoke: ok"
