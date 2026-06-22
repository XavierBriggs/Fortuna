# FORTUNA Ground-Truth Audit — 2026-06-18

Phase A of the ground-truth → demo-paper-ready initiative
(spec: `docs/superpowers/specs/2026-06-18-ground-truth-audit-and-demo-readiness-design.md`,
plan: `docs/superpowers/plans/2026-06-18-phase-a-ground-truth-audit.md`).

## Start here
- **[AUDIT.md](AUDIT.md)** — the atlas: exec summary, architecture, critical-path status, risk register, refactor roadmap (Phase B), readiness scorecard.
- **[MVP-CLOSURE-PLAN.md](MVP-CLOSURE-PLAN.md)** — Phase C: the verified gaps + phased plan to `fortuna start paper-demo`.

## Evidence
- `verification.md` — independent citation/dispute gate (38 findings checked, 35 upheld).
- `branch-ledger.md` — 14 branches classified (no work lost; all `archive/*` tagged).
- `doc-triage.md` — doc inventory (authoritative / stale / archive).
- `area-1-spine.md` … `area-9-discovery-seeding.md` — per-area findings with `file:line` evidence.
- `PROGRESS.md` — execution ledger.

## Bottom line
Safe, well-tested engine; **not** rubble. **Not yet demo-paper-ready** — the loop runs but doesn't close. One dominant root cause: **"computed but never persisted"** (Settlements/Calibration/Fills repos + bus recording all have `insert` methods with zero production callers). Fix, not rebuild.
