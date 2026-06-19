#!/usr/bin/env bash
# Download tennis-data.co.uk yearly result+odds files (free).
# Source: http://www.tennis-data.co.uk/alldata.php  (ATP 2000+, WTA 2007+).
# Files carry results, surface/series, set scores AND near-close Pinnacle odds.
# NOTE: verify the URL pattern at the source if a year 404s — the site layout
# occasionally changes; the Kaggle mirror is a fallback.
set -euo pipefail

TOUR="${1:-atp}"                 # atp | wta
START="${2:-2000}"
END="${3:-2024}"

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"   # repo root
DEST="$ROOT/data/deuce/tennisdata/$TOUR"
mkdir -p "$DEST"

base="http://www.tennis-data.co.uk"
echo "Downloading $TOUR $START-$END into $DEST"
for y in $(seq "$START" "$END"); do
  if [ "$TOUR" = "wta" ]; then
    url="$base/${y}w/${y}.xlsx"
  else
    url="$base/${y}/${y}.xlsx"
  fi
  out="$DEST/${y}.xlsx"
  if curl -fsSL "$url" -o "$out"; then
    echo "  ok   $y"
  else
    echo "  skip $y ($url not found)"
    rm -f "$out"
  fi
done
echo "Done. Files in $DEST"
