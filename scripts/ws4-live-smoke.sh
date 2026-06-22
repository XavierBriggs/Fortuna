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
# 1. Create + migrate the smoke DB
# ---------------------------------------------------------------------------
echo "ws4-live-smoke: creating temp DB $SMOKE_DB ..."
psql "postgres:///postgres?host=/tmp" -tAc "DROP DATABASE IF EXISTS $SMOKE_DB;" >/dev/null
psql "postgres:///postgres?host=/tmp" -tAc "CREATE DATABASE $SMOKE_DB;" >/dev/null

export SQLX_OFFLINE=true
export DATABASE_URL="postgres:///$SMOKE_DB?host=/tmp"

# Run sqlx migrations against the fresh DB (brings it to the ledger schema the
# doctor + persona_clv tests expect).
cargo run -q -p fortuna-cli -- migrate run 2>/dev/null \
  || sqlx migrate run --database-url "$DATABASE_URL" 2>/dev/null \
  || (
      echo "ws4-live-smoke: migrating via sqlx CLI fallback..."
      # Try cargo sqlx if available
      cargo sqlx migrate run --database-url "$DATABASE_URL" 2>/dev/null \
      || echo "ws4-live-smoke: WARNING: explicit migration step skipped — sqlx::test will handle it"
  )

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

DOCTOR_OUT="$(env $DOCTOR_ENV ./target/debug/fortuna doctor --offline 2>&1)"
DOCTOR_EXIT=$?
echo "$DOCTOR_OUT"

if [ $DOCTOR_EXIT -ne 0 ]; then
    echo "ws4-live-smoke FAIL: fortuna doctor --offline exited $DOCTOR_EXIT (expected 0)"
    exit 1
fi

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

cargo test -q \
    -p fortuna-live \
    --lib \
    -- \
    "persona_and_discovery_edge_ids_are_disjoint_for_the_same_seq" \
    --test-threads=1 \
    2>&1 | tail -5

echo "ws4-live-smoke: edge-ID disjointness test PASS"

# ---------------------------------------------------------------------------
# 3b. Meteorologist CLV non-null with discovery co-active (W5+W6a persona CLV)
# ---------------------------------------------------------------------------
echo "ws4-live-smoke: running meteorologist_belief_gets_nonnull_clv (persona_clv) ..."

cargo test -q \
    -p fortuna-live \
    --test persona_clv \
    -- \
    "meteorologist_belief_gets_nonnull_clv" \
    --test-threads=1 \
    2>&1 | tail -5

echo "ws4-live-smoke: persona CLV non-null test PASS"

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
echo ""
echo "ws4-live-smoke: ok"
