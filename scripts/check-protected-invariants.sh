#!/usr/bin/env bash
# Protected-invariant guard (CLAUDE.md "Protected directory").
#
# crates/fortuna-invariants/ tests are ADDITIONS-ONLY: you may ADD tests/files,
# never weaken, delete, rename, or modify the assertion logic of an EXISTING test.
# This script fails if the diff vs <base> removes or changes any line in an existing
# test file under crates/fortuna-invariants/tests/ — a modification shows up as a
# '-' (removed) line; a brand-new file is all '+' lines, so it passes; pure
# appends to an existing file are all '+' lines, so they pass too.
#
# Usage:  scripts/check-protected-invariants.sh [base-ref]   (default: main)
# CI runs it against the PR base; run it locally before every commit that touches
# the invariants crate. A legitimate reformat that trips this is an explicit
# operator override, not a silent allowance.
set -euo pipefail

BASE="${1:-main}"
DIR="crates/fortuna-invariants/tests"

if ! git rev-parse --verify --quiet "$BASE" >/dev/null; then
  echo "check-protected-invariants: base ref '$BASE' not found; pass an existing ref" >&2
  exit 2
fi

# Removed/changed lines in EXISTING test files (exclude the '---' diff header).
removed="$(git diff "$BASE" -- "$DIR" | grep -E '^-' | grep -vE '^---' || true)"

if [ -n "$removed" ]; then
  echo "PROTECTED-INVARIANT VIOLATION (CLAUDE.md): existing test(s) under $DIR were"
  echo "modified or deleted. The invariants crate is ADDITIONS-ONLY — add tests/files,"
  echo "never change an existing assertion. Offending removed/changed lines vs $BASE:"
  echo "----------------------------------------------------------------------"
  echo "$removed"
  echo "----------------------------------------------------------------------"
  echo "If an invariant test is genuinely defective: STOP, record it in GAPS.md under"
  echo "'Disputed invariant tests', and leave the test untouched for operator review."
  exit 1
fi

echo "OK: $DIR is additions-only vs $BASE (no existing assertion changed)."
