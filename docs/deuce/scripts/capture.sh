#!/usr/bin/env bash
# DEUCE three-way capture — cron-safe.
#
# Spends ~0 API credits when no ATP matches are live (the CLI returns before it
# queries the Odds API), and ~10 credits per run when matches are open. Appends
# each snapshot to data/deuce/live/atp_three_way.csv and logs to capture.log.
#
# Install (every 30 min, all day — idempotent):
#   ( crontab -l 2>/dev/null | grep -v 'deuce/scripts/capture.sh' ; \
#     echo "*/30 * * * * $HOME/fortuna/docs/deuce/scripts/capture.sh" ) | crontab -
# Remove:
#   crontab -l | grep -v 'deuce/scripts/capture.sh' | crontab -
# Watch:
#   tail -f $HOME/fortuna/data/deuce/live/capture.log
set -uo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"          # docs/deuce
LOG="$DIR/../../data/deuce/live/capture.log"
mkdir -p "$(dirname "$LOG")"

{
  echo "=== $(date -u +%Y-%m-%dT%H:%M:%SZ) ==="
  "$DIR/.venv/bin/deuce" live-capture
  echo
} >> "$LOG" 2>&1
