#!/usr/bin/env bash
# Monthly kill-switch test (spec I4: "Tested monthly"). Run it with the main
# runtime DOWN and Postgres optionally stopped — the switch must not care.
#
# Usage: scripts/killswitch-test.sh [journal-path]
set -euo pipefail
JOURNAL="${1:-/tmp/fortuna-killswitch-test-$(date +%Y%m%d).jsonl}"
echo "[killswitch-test] building and running self-test (journal: $JOURNAL)"
# DATABASE_URL deliberately unset: the switch must never need it.
env -u DATABASE_URL cargo run -q -p fortuna-killswitch -- self-test --journal "$JOURNAL"
echo "[killswitch-test] PASS — record this run in the ops log / audit."
