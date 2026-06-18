#!/usr/bin/env bash
# Launch the Kalshi paper-on-live-data demo as managed daemon + recorder.
#
# Usage:
#   scripts/demo-launch.sh [--clear-killswitch] [--no-caffeinate] [--skip-market-refresh]
#
# The kill-switch clear is an operator action. Without --clear-killswitch this
# script prompts before clearing the revocation sentinel.
set -euo pipefail

usage() {
  cat <<'EOF'
Launch the Kalshi paper-on-live-data demo as managed daemon + recorder.

Usage:
  scripts/demo-launch.sh [--clear-killswitch] [--no-caffeinate] [--skip-market-refresh]

Options:
  --clear-killswitch  Clear the revocation sentinel without prompting.
  --no-caffeinate     Do not start macOS caffeinate for the daemon lifetime.
  --skip-market-refresh
                      Do not update static strategy ticker seeds before boot.

The kill-switch clear is an operator action. Without --clear-killswitch this
script prompts before clearing the revocation sentinel.
EOF
}

clear_killswitch=prompt
use_caffeinate=1
refresh_markets=1

while [ "$#" -gt 0 ]; do
  case "$1" in
    --clear-killswitch)
      clear_killswitch=yes
      ;;
    --no-caffeinate)
      use_caffeinate=0
      ;;
    --skip-market-refresh)
      refresh_markets=0
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CONFIG_PATH="${FORTUNA_DEMO_CONFIG:-$ROOT/config/fortuna.toml}"
RUNTIME_DIR="${FORTUNA_RUNTIME_DIR:-$ROOT/data/runtime}"
JOURNAL="${FORTUNA_KILLSWITCH_JOURNAL:-$ROOT/data/runtime/killswitch/ks.log}"
SENTINEL="$ROOT/data/runtime/killswitch/KILLSWITCH_REVOKED"
FORTUNA="$ROOT/target/release/fortuna"
KILLSWITCH="$ROOT/target/release/fortuna-killswitch"

need_file() {
  if [ ! -f "$1" ]; then
    echo "missing required file: $1" >&2
    exit 1
  fi
}

need_exec() {
  if [ ! -x "$1" ]; then
    echo "missing executable: $1" >&2
    echo "build first: cargo build --release -p fortuna-live -p fortuna-cli -p fortuna-recorder -p fortuna-killswitch" >&2
    exit 1
  fi
}

need_file "$CONFIG_PATH"
need_file "$ROOT/.env"
need_exec "$FORTUNA"
need_exec "$ROOT/target/release/fortuna-live"
need_exec "$ROOT/target/release/fortuna-recorder"
need_exec "$KILLSWITCH"

set -a
# shellcheck disable=SC1091
source "$ROOT/.env"
set +a

if [ -z "${DATABASE_URL:-}" ]; then
  echo ".env did not set DATABASE_URL" >&2
  exit 1
fi

if [ -n "${FORTUNA_DEMO_DATABASE_URL:-}" ]; then
  export DATABASE_URL="$FORTUNA_DEMO_DATABASE_URL"
else
  case "$DATABASE_URL" in
    */fortuna)
      export DATABASE_URL="${DATABASE_URL%/fortuna}/fortuna_demo"
      ;;
    */fortuna_demo)
      export DATABASE_URL="$DATABASE_URL"
      ;;
    *)
      echo "DATABASE_URL is not pointed at fortuna or fortuna_demo; set FORTUNA_DEMO_DATABASE_URL explicitly" >&2
      exit 1
      ;;
  esac
fi
db_label="${DATABASE_URL##*/}"

echo "[demo] repo: $ROOT"
echo "[demo] config: $CONFIG_PATH"
echo "[demo] database: $db_label"

if [ "$refresh_markets" -eq 1 ]; then
  "$ROOT/scripts/refresh-demo-markets.sh" "$CONFIG_PATH"
else
  echo "[demo] market refresh skipped"
fi

"$FORTUNA" config check --config-path "$CONFIG_PATH"

if command -v sqlx >/dev/null 2>&1; then
  echo "[demo] applying ledger migrations"
  sqlx migrate run --source "$ROOT/crates/fortuna-ledger/migrations" >/dev/null
else
  echo "[demo] sqlx not found; relying on daemon boot migration"
fi

if [ -f "$SENTINEL" ]; then
  if [ "$clear_killswitch" = prompt ]; then
    echo
    echo "Kill-switch revocation sentinel is present:"
    echo "  $SENTINEL"
    echo "Clearing it re-arms order placement for the Kalshi paper demo after start."
    printf 'Type CLEAR KILLSWITCH AND START to continue: '
    IFS= read -r answer
    if [ "$answer" != "CLEAR KILLSWITCH AND START" ]; then
      echo "aborted; sentinel left in place"
      exit 1
    fi
  fi
  "$KILLSWITCH" clear-revocation --journal "$JOURNAL"
else
  echo "[demo] kill-switch sentinel already clear"
fi

echo "[demo] starting managed daemon + recorder"
"$FORTUNA" start --config-path "$CONFIG_PATH"

daemon_pid_file="$RUNTIME_DIR/daemon.pid"
if [ ! -f "$daemon_pid_file" ]; then
  echo "daemon pidfile missing after start: $daemon_pid_file" >&2
  "$FORTUNA" status || true
  exit 1
fi
daemon_pid="$(sed -n '1p' "$daemon_pid_file")"

if [ "$use_caffeinate" -eq 1 ] && command -v caffeinate >/dev/null 2>&1; then
  nohup caffeinate -dimsu -w "$daemon_pid" >/dev/null 2>&1 &
  caffeinate_pid="$!"
  echo "$caffeinate_pid" > "$RUNTIME_DIR/caffeinate.pid"
  echo "[demo] caffeinate active while daemon pid $daemon_pid runs (pid $caffeinate_pid)"
else
  echo "[demo] caffeinate skipped; keep the Mac awake manually"
fi

echo "[demo] waiting for process health"
sleep 5
"$FORTUNA" status

if ! ps -p "$daemon_pid" >/dev/null 2>&1; then
  echo "daemon pid $daemon_pid is not running after start; recent daemon log:" >&2
  tail -n 80 "$RUNTIME_DIR/logs/daemon.log" >&2 || true
  exit 1
fi

echo
echo "ROTA: http://127.0.0.1:9187/rota"
echo "Daemon log:   $FORTUNA logs daemon -f"
echo "Recorder log: $FORTUNA logs recorder -f"
echo "Stop cleanly: $FORTUNA stop"
echo "Emergency:    cd $ROOT && set -a && source .env && set +a && $KILLSWITCH freeze --journal $JOURNAL --venue kalshi"
