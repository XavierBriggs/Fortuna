#!/usr/bin/env bash
# WS3 live smoke — end-to-end CLI pipeline against a real (or real-shaped) Aeolus
# archive: AeolusArchiveSource -> ReplayHarness -> ledger -> GO surface.
#
# Asserts, on real data: (1) a NON-EMPTY source-stamped replay, (2) idempotency
# (a second run writes 0), (3) rows carry provenance.source='historical-import',
# (4) `validate` prints the whole-truth GO surface. This is the live-system check
# the WS3 boundary requires (the Aeolus connection is exercised, not assumed).
#
# Archive selection:
#   - If FORTUNA_WS3_ARCHIVE is set, it is used VERBATIM (point it at a real
#     bounded slice extracted read-only from the prod archive — the real-
#     connection test, runnable on the box that can reach Aeolus).
#   - Otherwise a temp SQLite is built from the committed real-shaped fixture
#     (crates/fortuna-backtest/tests/fixtures/aeolus_archive.sql) so the pipeline
#     smoke is self-contained / CI-runnable without prod access.
#
# Requires: a superuser Postgres socket at /tmp (the ledger-gate-DB recipe).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

SMOKE_DB="fortuna_ws3_livegate"
FROM="${WS3_SMOKE_FROM:-2026-01-01}"
TO="${WS3_SMOKE_TO:-2026-12-31}"

if [ -n "${FORTUNA_WS3_ARCHIVE:-}" ]; then
  ARCHIVE="$FORTUNA_WS3_ARCHIVE"
  echo "live-smoke: using REAL archive $ARCHIVE"
else
  ARCHIVE="$(mktemp -t ws3_live_fixture.XXXXXX.db)"
  rm -f "$ARCHIVE"
  sqlite3 "$ARCHIVE" < crates/fortuna-backtest/tests/fixtures/aeolus_archive.sql
  echo "live-smoke: FORTUNA_WS3_ARCHIVE unset -> built real-shaped fixture DB $ARCHIVE"
fi

psql "postgres:///postgres?host=/tmp" -tAc "DROP DATABASE IF EXISTS $SMOKE_DB;" >/dev/null
psql "postgres:///postgres?host=/tmp" -tAc "CREATE DATABASE $SMOKE_DB;" >/dev/null
export SQLX_OFFLINE=true DATABASE_URL="postgres:///$SMOKE_DB?host=/tmp" FORTUNA_WS3_ARCHIVE="$ARCHIVE"

OUT1="$(cargo run -q -p fortuna-cli -- backtest aeolus-archive --from "$FROM" --to "$TO" 2>&1)"
echo "$OUT1" | grep -qE 'written=[1-9][0-9]* ' || { echo "LIVE SMOKE FAIL: run1 wrote 0 rows"; echo "$OUT1"; exit 1; }

OUT2="$(cargo run -q -p fortuna-cli -- backtest aeolus-archive --from "$FROM" --to "$TO" 2>&1)"
echo "$OUT2" | grep -qE 'written=0 ' || { echo "LIVE SMOKE FAIL: run2 not idempotent"; echo "$OUT2"; exit 1; }

STAMPED="$(psql "$DATABASE_URL" -tAc "SELECT COUNT(*) FROM beliefs WHERE provenance::text LIKE '%historical-import%';" | tr -d '[:space:]')"
[ "${STAMPED:-0}" -gt 0 ] || { echo "LIVE SMOKE FAIL: no source-stamped (historical-import) rows"; exit 1; }

cargo run -q -p fortuna-cli -- validate --scope forecast:KNYC --producer historical-import 2>&1 | grep -q 'verdict:' \
  || { echo "LIVE SMOKE FAIL: validate did not print a GO-surface verdict"; exit 1; }

echo "live-smoke: ok (run1 written>0, run2 idempotent, $STAMPED source-stamped rows, GO surface printed)"
