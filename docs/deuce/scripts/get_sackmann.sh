#!/usr/bin/env bash
# Clone Jeff Sackmann's match databases (free, CC-BY-NC-SA — research only).
# NEXT PHASE: serve/return features for the point model; not needed for Phase A.
set -euo pipefail

TOUR="${1:-atp}"   # atp | wta
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
DEST="$ROOT/data/deuce/sackmann"
mkdir -p "$DEST"

repo="https://github.com/JeffSackmann/tennis_${TOUR}.git"
target="$DEST/tennis_${TOUR}"
if [ -d "$target/.git" ]; then
  echo "Updating $target"; git -C "$target" pull --ff-only
else
  echo "Cloning $repo -> $target"; git clone --depth 1 "$repo" "$target"
fi
echo "Done. Also consider tennis_MatchChartingProject for point-by-point data."
