#!/usr/bin/env bash
# Replay verification entrypoint.
#
# Usage:
#   scripts/replay.sh <recording.jsonl>   # structural verify of a recorded stream (T0.2)
#   scripts/replay.sh --seed <N>          # re-run a DST seed deterministically (T0.4)
#
# Semantic replay of derived events (same handlers, byte-compare) is exposed
# in-library as fortuna_core::bus::replay_verify and exercised by the DST
# harness; live-decision replay from audit manifests arrives with the ledger.
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: scripts/replay.sh <recording.jsonl> | --seed <N>" >&2
  exit 2
fi

case "$1" in
  --seed)
    if [[ $# -ne 2 ]]; then
      echo "usage: scripts/replay.sh --seed <N>" >&2
      exit 2
    fi
    if cargo test -p fortuna-core --test dst --no-run >/dev/null 2>&1; then
      exec cargo test -p fortuna-core --test dst -- --nocapture --replay-seed "$2"
    fi
    echo "[replay] DST harness not implemented yet (BUILD_PLAN T0.4)." >&2
    exit 1
    ;;
  *)
    exec cargo run -q -p fortuna-core --bin replay-verify -- "$1"
    ;;
esac
