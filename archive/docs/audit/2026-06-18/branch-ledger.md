# Branch Ledger (no work lost — all archive-tagged)

Audited: 2026-06-18  
Trunk: `feature/paper-on-live-data`  
Method: content-first — `git log trunk..branch` for unique commits, then direct
blob diffs (`git diff trunk:file branch:file`) to distinguish "branch has unique
content" from "trunk moved past the merge-base."  
Archive tags: all 15 branches are already tagged `archive/<branch-name>` — no work
can be lost by a future deletion.

---

## Summary

15 branches total (including trunk).

| Classification | Count | Branches |
|---|---|---|
| Absorbed | 11 | `build/phase-0`, `main`, `persona-live-integration`, `track-b`, `track-c`, `track-e`, `track-e-aeolus`, `track-e-bucket-matching`, `track-e-f10-e5`, `track-e-weather-resolve`, `track-a` |
| Stranded | 2 | `track-d`, `track-e-docs-freshen` |
| Redundant | 1 | `track-c-pre-rebase-3b6278c` |
| Trunk (excluded) | 1 | `feature/paper-on-live-data` |

---

## Ledger

| Branch | commits-ahead-of-trunk | unique files (if stranded) | classification | evidence | recommended action (Phase B) |
|---|---|---|---|---|---|
| `build/phase-0` | 0 | — | absorbed | IS ancestor of trunk (`git merge-base --is-ancestor` YES); 0-line diffstat | Delete |
| `main` | 0 | — | absorbed | IS ancestor of trunk; 0-line diffstat | Delete |
| `persona-live-integration` | 0 | — | absorbed | IS ancestor of trunk; 0-line diffstat | Delete |
| `track-b` | 1 | — | absorbed | NOT an ancestor (cherry-pick), but `git diff trunk:docs/design/rota-observability.md branch:file` = 0 lines — content is byte-identical in trunk | Delete |
| `track-c` | 0 | — | absorbed | IS ancestor of trunk; 0-line diffstat | Delete |
| `track-e` | 0 | — | absorbed | IS ancestor of trunk; 0-line diffstat | Delete |
| `track-e-aeolus` | 0 | — | absorbed | IS ancestor of trunk; 0-line diffstat | Delete |
| `track-e-bucket-matching` | 0 | — | absorbed | IS ancestor of trunk; 0-line diffstat | Delete |
| `track-e-f10-e5` | 0 | — | absorbed | IS ancestor of trunk; 0-line diffstat | Delete |
| `track-e-weather-resolve` | 0 | — | absorbed | IS ancestor of trunk; 0-line diffstat | Delete |
| `track-a` | 5 | — | absorbed | NOT an ancestor, but trunk is a **strict superset**: `resolve_kalshi_demo_creds`, `build_kalshi_demo_transport` (IO-free), `redact_secrets`, `flatten-perps` runbook section, and all doc freshening are ALL present in trunk. Trunk's `daemon.rs` (4554 lines vs branch 4282) and `main.rs` (983 vs 830) have moved further. Branch's `CHANGELOG.md` / `BUILD_PLAN.md` / `GAPS.md` are older (missing C-next-1b/2 completion, paper-on-live phase entries). | Delete |
| `track-d` | 1 | `GAPS.md` | **stranded** | Commit `7094e40`: marks weather-resolver `drive()` wiring as `DONE (Track A, 349881d — GATE ACCEPT, merged)`. Trunk's `GAPS.md` (5858 lines) still says this is "PENDING / coordinate with Track A" — the branch (5696 lines) has the accurate status. `track-d-ingestion-subsystem.md` is byte-identical in trunk. | Merge `GAPS.md` update into trunk, then delete branch |
| `track-e-docs-freshen` | 1 | `GAPS.md`, `BUILD_PLAN.md` | **stranded** | Commit `500b03a`: removes stale "RALPH STOP / NOT yet wired" caveat from `GAPS.md` and checks the `drive()` wiring box in `BUILD_PLAN.md`. Trunk's `GAPS.md` still says weather-resolver drive() wiring pending; branch correctly records it DONE (`0ad3f3f`/`349881d`). `track-e-aeolus-changelog.md` is byte-identical in trunk. | Merge `GAPS.md` + `BUILD_PLAN.md` updates into trunk, then delete branch |
| `track-c-pre-rebase-3b6278c` | 5 | — | redundant | A pre-rebase snapshot of `track-c` (commit `3b6278c`). Its code (`FundingWindow`, `funding_window.rs` test) is byte-identical in trunk. Its design doc (`perp-strategies-and-scalar-claims.md`) is a 445-line subset of trunk's 629-line version. Its `GAPS.md`/`BUILD_PLAN.md`/`ASSUMPTIONS.md` are all older than trunk. Preserved as `archive/track-c-pre-rebase-3b6278c` for historical reference. | Delete |

---

## Stranded work needing review (detail)

### `track-d` — commit `7094e40`

**What it does:** de-stales Track-D's own docs after the weather loop closed
cross-track. Two files changed:

1. **`GAPS.md`** (13 net insertions): The section covering the weather-resolver
   `drive()` wiring handoff changed from:
   > `STILL OPEN (Track A): the one-line drive() call wiring the resolver into the
   > resolution tick ... coordinate with Track A`

   to:
   > `DONE (Track A, 349881d — GATE ACCEPT, merged): the drive() daily-boundary call
   > now fires BOTH resolvers (weather + funding), pool-gated, idempotent, fail-soft
   > ... THE WEATHER LOOP IS NOW LIVE END-TO-END`

2. **`docs/design/track-d-ingestion-subsystem.md`**: byte-identical to trunk (the
   branch's version of this file was already absorbed).

**Why it matters:** trunk's `GAPS.md` currently misrepresents commit `349881d` as
pending work. The branch has the accurate status. This is a one-section doc
correction, not a code change.

**Merge risk:** Low. The GAPS section is additive (changes one status block).
Trunk has 162 additional lines vs branch in `GAPS.md` overall, but the specific
section is contiguous and non-conflicting.

---

### `track-e-docs-freshen` — commit `500b03a`

**What it does:** freshens Track-E's docs after Track-A wired the weather
resolver into `drive()`.

1. **`GAPS.md`** (73 lines changed, net -4): Removes the RALPH STOP `drive()`
   wiring item ("STILL OPEN (Track A)" → "DONE, `0ad3f3f`/`349881d`") and
   trims the open-handoffs list. Trunk still has the stale RALPH STOP text
   calling the wiring "NOT yet on the live tick."

2. **`BUILD_PLAN.md`** (10 lines changed, net 0): Moves the `drive()` wiring
   sub-item from open to done (rewrites one bullet from "NOT yet wired into
   `drive()` — that one-line additive call is Track A's" to "wired live by
   Track A, loop CLOSED").

3. **`docs/design/track-e-aeolus-changelog.md`**: byte-identical to trunk.

**Why it matters:** trunk's `GAPS.md` and `BUILD_PLAN.md` contain stale
"pending" entries for work that commit `349881d` (in trunk) completed. New
readers following the GAPS queue may act on a false open item.

**Merge risk:** Low. Both files are doc-only. The GAPS change is a targeted
status update; the BUILD_PLAN change rewrites a single sub-bullet. Trunk has
moved ~46 lines further in GAPS.md on unrelated sections — standard merge
resolution.

---

## Notes for Phase B

- **Deletion order does not matter** — all branches are archive-tagged; `git tag -l
  'archive/*'` lists them. Tags are permanent refs; deleting the branch pointer
  does not lose the commits.
- **Stranded merges first:** merge `track-d` and `track-e-docs-freshen` doc
  corrections into trunk before deleting — these are the only two branches with
  content that trunk does not yet have.
- **track-c-pre-rebase-3b6278c** is a historical snapshot; once the operator
  confirms the perp-strategies design doc in trunk is the canonical version, it
  can be deleted (the `archive/` tag preserves it).
- **trunk itself** is tagged `archive/feature/paper-on-live-data` — that tag should
  be updated or the policy clarified if trunk continues to advance (archive tags
  are point-in-time snapshots, not rolling refs).
