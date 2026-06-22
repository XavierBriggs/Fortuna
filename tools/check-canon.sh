#!/usr/bin/env bash
# check-canon.sh — the canon drift guardrail (advisory).
#
# Fails (non-zero) when either:
#   (1) CLOSED SET: a repo-root *.md exists that is not listed in canon.manifest
#       (canonical or permitted-noncanonical) and is not under an ignore-root.
#   (2) INVARIANT COVERAGE: a CONSTITUTION invariant has no covering test — i.e.
#       a `present` map row names a test that does not exist, or an invariant ID
#       has zero `present` covering tests. `todo` rows are reported, not failed.
#
# Env:
#   CANON_DIR  where canon.manifest and CONSTITUTION.md live   (default: script parent)
#   REPO_ROOT  where crates/ and the root *.md live            (default: CANON_DIR)
# Usage:  tools/check-canon.sh
set -uo pipefail

CANON_DIR="${CANON_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
REPO_ROOT="${REPO_ROOT:-$CANON_DIR}"
MANIFEST="$CANON_DIR/canon.manifest"
CONSTITUTION="$CANON_DIR/CONSTITUTION.md"
fail=0

echo "== canon check =="
echo "canon-dir: $CANON_DIR"
echo "repo-root: $REPO_ROOT"

[ -f "$MANIFEST" ]     || { echo "FAIL: no canon.manifest at $MANIFEST"; exit 2; }
[ -f "$CONSTITUTION" ] || { echo "FAIL: no CONSTITUTION.md at $CONSTITUTION"; exit 2; }

# --- (1) closed set ----------------------------------------------------------
# Allowlist = lines under [canonical] and [permitted-noncanonical] that look like
# a root-level filename (strip directory entries and ** globs; those are subdir/dir rules).
allow="$(awk '
  /^\[canonical\]/        {sec=1; next}
  /^\[permitted-noncanonical\]/ {sec=1; next}
  /^\[/                   {sec=0; next}
  sec==1 && $0 !~ /^#/ && NF>0 {print $1}
' "$MANIFEST" | grep -vE '/\*\*$' | grep -vE '/' )"

echo
echo "-- closed-set check (repo-root *.md) --"
while IFS= read -r f; do
  base="$(basename "$f")"
  if echo "$allow" | grep -qxF "$base"; then
    :
  else
    echo "  UNLISTED top-level doc: $base (add to canon.manifest, archive it, or move under scratch/docs/archive)"
    fail=1
  fi
done < <(find "$REPO_ROOT" -maxdepth 1 -name '*.md' -type f | sort)
[ "$fail" = 0 ] && echo "  ok: every repo-root *.md is on the manifest"

# --- (2) invariant coverage --------------------------------------------------
echo
echo "-- invariant-coverage check (CONSTITUTION invariant-to-test map) --"
map="$(awk '/INVARIANT-MAP-BEGIN/{f=1;next} /INVARIANT-MAP-END/{f=0} f && /\|/{print}' "$CONSTITUTION" | grep -vE '^\s*```')"
[ -n "$map" ] || { echo "  FAIL: no invariant map found in CONSTITUTION.md"; exit 2; }

declare -A id_has_present
missing=0; todo=0
while IFS= read -r line; do
  id="$(echo "$line"   | awk -F'|' '{gsub(/ /,"",$1); print $1}')"
  path="$(echo "$line" | awk -F'|' '{gsub(/^[ \t]+|[ \t]+$/,"",$2); print $2}')"
  fn="$(echo "$line"   | awk -F'|' '{gsub(/^[ \t]+|[ \t]+$/,"",$3); print $3}')"
  st="$(echo "$line"   | awk -F'|' '{gsub(/ /,"",$4); print $4}')"
  [ -z "$id" ] && continue
  if [ "$st" = "todo" ]; then
    echo "  TODO ($id): test '$fn' not yet written ($path)"
    todo=$((todo+1)); continue
  fi
  # present row: the named test (or compile_fail anchor) must exist in the cited path
  target="$REPO_ROOT/$path"
  if [ ! -f "$target" ]; then
    echo "  MISSING ($id): cited file not found: $path"; missing=$((missing+1)); fail=1; continue
  fi
  if [[ "$fn" == compile_fail* ]]; then
    if grep -q "compile_fail" "$target"; then id_has_present[$id]=1
    else echo "  MISSING ($id): no compile_fail anchor in $path"; missing=$((missing+1)); fail=1; fi
  else
    if grep -qE "fn[[:space:]]+$fn\b" "$target"; then id_has_present[$id]=1
    else echo "  MISSING ($id): test fn '$fn' not found in $path"; missing=$((missing+1)); fail=1; fi
  fi
done <<< "$map"

# every invariant ID must have at least one present covering test
all_ids="$(echo "$map" | awk -F'|' '{gsub(/ /,"",$1); print $1}' | sort -u | grep -v '^$')"
while IFS= read -r id; do
  [ -z "$id" ] && continue
  if [ -z "${id_has_present[$id]:-}" ]; then
    echo "  UNCOVERED invariant: $id has no present covering test"; fail=1
  fi
done <<< "$all_ids"

echo
echo "summary: missing-tests=$missing todo=$todo closed-set-violations=$([ "$fail" = 1 ] && echo present || echo none)"
if [ "$fail" = 0 ]; then echo "CANON OK"; else echo "CANON DRIFT (advisory: see lines above)"; fi
exit "$fail"
