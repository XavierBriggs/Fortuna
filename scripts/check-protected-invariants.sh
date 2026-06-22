#!/usr/bin/env bash
# Protected-invariant guard (CLAUDE.md "Protected directory").
#
# crates/fortuna-invariants/ assertions are ADDITIONS-ONLY: you may ADD tests/files,
# never weaken, delete, rename, or modify the assertion logic of an EXISTING test.
# This script fails if the diff vs <base> removes or changes any line in an existing
# protected file — the runtime tests under crates/fortuna-invariants/tests/ AND the
# I1/perp-I1 compile_fail doc-tests in crates/fortuna-invariants/src/lib.rs. A
# modification shows up as a '-' (removed) line; a brand-new file is all '+' lines,
# so it passes; pure appends to an existing file are all '+' lines, so they pass too.
#
# Usage:  scripts/check-protected-invariants.sh [base-ref]   (default: main)
# CI runs it against the PR base; run it locally before every commit that touches
# the invariants crate. A legitimate reformat that trips this is an explicit
# operator override, not a silent allowance.
set -euo pipefail

BASE="${1:-main}"
# Both protected scopes: the runtime invariant tests AND the compile_fail doc-tests
# in src/lib.rs (the I1 / perp-I1 type-level seals live there, not under tests/).
PROTECTED=(
  "crates/fortuna-invariants/tests"
  "crates/fortuna-invariants/src/lib.rs"
)

if ! git rev-parse --verify --quiet "$BASE" >/dev/null; then
  echo "check-protected-invariants: base ref '$BASE' not found; pass an existing ref" >&2
  exit 2
fi

# Removed/changed lines in EXISTING protected files (exclude the '---' diff header).
removed="$(git diff "$BASE" -- "${PROTECTED[@]}" | grep -E '^-' | grep -vE '^---' || true)"

if [ -n "$removed" ]; then
  echo "PROTECTED-INVARIANT VIOLATION (CLAUDE.md): an existing assertion under"
  echo "${PROTECTED[*]} was modified or deleted. The invariants crate is ADDITIONS-ONLY"
  echo "— add tests/files, never change an existing assertion. Offending lines vs $BASE:"
  echo "----------------------------------------------------------------------"
  echo "$removed"
  echo "----------------------------------------------------------------------"
  echo "If an invariant test is genuinely defective: STOP, record it in GAPS.md under"
  echo "'Disputed invariant tests', and leave the test untouched for operator review."
  exit 1
fi

echo "OK: ${PROTECTED[*]} are additions-only vs $BASE (no existing assertion changed)."
