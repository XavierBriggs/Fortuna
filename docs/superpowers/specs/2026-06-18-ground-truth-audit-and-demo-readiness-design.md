# Ground-Truth Audit → Demo-Paper-Ready (v1) — Design

**Date:** 2026-06-18 · **Status:** approved (brainstorm) · **Next:** implementation plan (writing-plans)

## 1. Why this exists

FORTUNA was built breadth-first across many parallel agents/tracks (Cursor, Codex, track-a…e). The result is not a missing system — it's an **illegible, fragmented** one: ~127k LOC, giant files, overlapping/competing models (two execution-mode systems, two live DBs, mockups vs ROTA), parked threads from abandoned tracks, 15 git branches, 100+ docs of unknown authority, and a loop whose last feedback wires are open. The system *feels* broken because you can't see it, not because the pieces are absent.

This initiative establishes **ground truth**, cuts the **bloat**, and closes the loop into a **demo-paper-ready v1** that accrues data to learn and create edges — on the existing safe engine, **not a rewrite**.

## 2. North Star & target state

**North star:** a safe, replayable, venue-connected system that closes the loop from live market data → risk-gated paper execution → auditable PnL → scored validation, on a path toward **$50k validated PnL a month**.

**Concrete target (definition of done for this initiative):** one clean command —

```
fortuna start paper-demo
```

— boots the full closed loop on **live Kalshi data**, with **no order-mutation path constructible**, running **all strategies in paper**, where every decision flows the spine (live book → event → belief → mapping → proposal → gate → paper fill → settlement → score) and **one view shows the whole chain**. State is append-only; data accrues so strategies can be honed. One config, one trunk, no dual models, no dead code.

This deliberately delivers the *MVP experience* (clean CLI + visible, safe closed loop) on the *existing engine*. It is **v1**, not a throwaway MVP: all strategies plug into the one closed spine.

## 3. Approach: A → B → C


| Phase                  | Goal                                                          | Output                                                                                              | Code changes     |
| ---------------------- | ------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | ---------------- |
| **A — Ground Truth**   | See the whole system; know the exact distance to `paper-demo` | the **atlas** + **MVP closure plan** + branch ledger + doc triage                                   | none (read-only) |
| **B — Consolidate**    | Lean, single-pathed, legible — kill the bloat                 | promoted trunk, archived branches/docs, deleted dead code, merged dual models, targeted file splits | yes              |
| **C — Close the loop** | Demo-paper-ready; gathering data                              | closed spine (F0/F1), the `paper-demo` CLI, the chain-view                                          | yes              |


**Trunk anchor:** `feature/paper-on-live-data` is the de-facto truth (the running system that produced all analyzed data). Confirmed by content-diff in Phase A, then promoted to `main` in Phase B.

## 4. Phase A — the audit (mechanism)

**Brief:** the installed `deep-codebase-audit` skill (`.claude/skills/deep-codebase-audit/`) — its protocol, the `fortuna_trading_system_profile.md`, and its `audit_report.md` + `mvp_closure_plan.md` templates are authoritative.

**Depth:** full ensemble — each area gets a paired **Auditor + Adversary**; a **Verifier** independently checks every `file:line` citation; the **Lead** synthesizes. Read-first, evidence-based, P0–P3 severity. No code edits during the audit.

**Cross-cutting Readiness lens:** every finding is scored against the target state —
`BLOCKS demo-paper-ready` · `SERVES it` · `BLOAT — cut` · `out-of-scope — park`.
The Lead synthesizes a dedicated **Demo-Paper-Ready Readiness Scorecard** = the exact distance to `fortuna start paper-demo`.

### 4.1 The 9 audit areas

1. **Critical paths / the spine** — the 6 golden paths (market-data, decision, risk, execution, accounting/PnL, replay): wired / open / duplicated end-to-end.
2. **Duplication, dead code & integration debt** — the "why 127k LOC / what's cruft"; the consolidation ledger.
3. **Safety & invariant integrity** — I1–I7 + the trading invariants (no live without arming, demo-can't-execute, idempotency, no stale-as-live, no vendor type in core). Enforced by code/tests vs convention.
4. **Module boundaries & legibility** — giant files, tangled ownership, unclear interfaces; refactor targets.
5. **Vendor/venue coupling** — Kalshi/Kinetics decoupling: does vendor shape leak into core / risk / PnL / strategies?
6. **Test & replay posture** — the test-gap report; can replay reproduce decisions (event-sourced truth)?
7. **Operational readiness, demo CLI & observability** — kill switch, WS reconnect, rate limits, logs/metrics, the clean `paper-demo` CLI, the single chain-view.
8. **Cognition: Mind, personas & belief authoring** — Mind trait/tiers/budget/degrade; persona registry/config/runner/orchestrator; `domain_analyses`; how beliefs are authored (deterministic vs model). *Known: today 100% of beliefs are the deterministic `aeolus` path; 0 from the Mind/personas.*
9. **Discovery, seeding & signal→event pipeline** — world-forward + market-back discovery, signal ingestion/normalization, event seeding (operator + auto), event dedup (family-keys + Jaccard), catalog→edge minting, the watch-event dead-end. *This is the recurring "mess"; its goal is a clean, legible seeding + signal→event design.*

### 4.2 Two non-code legs (run alongside)

- **Branch ledger (no work lost):** tag every branch `archive/<name>` (recoverable forever) → content-diff each vs trunk → classify `absorbed` / `stranded (review)` / `redundant`. Verifier sanity-checks the "absorbed" claims. **Nothing deleted until the ledger is reviewed.** Judge by *content*, not branch ancestry.
- **Doc triage:** classify every doc `authoritative` / `stale` / `archive`; collapse toward one source of truth. (Current sprawl: spec.md, GAPS.md ~435KB, ASSUMPTIONS.md ~158KB, CHANGELOG ~124KB, docs/design ×33, docs/reviews ×57, mockups, AMENDMENT files, vision docs.)

### 4.3 Phase-A deliverables

- The **atlas** (`audit_report.md` template): exec summary, what-it-is, north star, architecture map, critical-path table, risk register (P0–P3), vendor-coupling report, test-gap report, security review, ops readiness, **MVP closure plan**, refactor roadmap, next-3-moves.
- The **branch ledger** and **doc triage** tables.
- The **Demo-Paper-Ready Readiness Scorecard**.

## 5. Phase B — Consolidate (act on the atlas)

Driven entirely by Phase-A findings, in risk order:

- Promote trunk → `main`; archive (tagged) + delete redundant branches; surface stranded work for review.
- Collapse docs to the authoritative set; archive the rest.
- Delete dead code; merge the dual models (execution-mode, the two-DB path); the file splits that serve legibility (Area 4).
- Each change is small, safe, test-gated (no behavior change for refactors; DST + invariants green).

## 6. Phase C — Close the loop + the demo

**Close the two known open wires** (already evidenced; the audit will confirm/scope precisely):

- **F1 — paper-live settlement:** drive `settlements_since`/`settle_market` on resolved markets → realized PnL per strategy.
- **F0 — calibration persistence in `stage="paper"`:** persist the weekly review's fitted Platt → the model/synthesis arm wakes up. (Applying calibration in paper is not capital promotion; the I7 boundary is explicit.)
- Plus: confirm weather belief grading; fix funding-score window-dedup; the clean `fortuna start paper-demo` CLI + the one chain-view.

**v1 = all strategies on the one closed spine.** Available immediately: `mech_extremes`, `mech_structural`. Activates on calibration: `synthesis` (weather). Data-only/ramping: `perp_event_basis_v2`, `funding_forecast`.

**The data flywheel (how `paper-demo` learns):**

- *Daily:* ingest → believe → propose → gate → paper-fill on live books; markets resolve → settle → realized PnL + trade scores; beliefs resolve → Brier/CLV → calibration samples; daily PnL/health snapshot.
- *Weekly:* the review fits per-scope calibration (≥50 resolved), computes per-strategy scorecards (PnL after fees, CLV, drawdown, fee ratio), emits advisory GO/NO-GO — the steering wheel for honing strategies.
- *Multi-week:* Wk1 mechanical fills + belief accrual; Wk2–3 calibration fits → synthesis activates, funding has enough windows; Wk4+ validation bundles mature → review-eligible strategies surface.

**Persona strategy (2–3, evidence-picked).** Selection criteria: a working signal source + liquid, cleanly-resolvable Kalshi markets + a plausible edge. Initial read: **weather** (proven — `meteorologist` + Aeolus path); **econ/finance** (half-built — `macro-economist` config exists, BLS/Fed signals flow, needs the Area-9 seeding fix); **politics or crypto** (chosen by a short market-quality research pass). Areas 8/9 + that research pass make the final call.

## 7. Out of scope / non-goals

- No live trading; no production order path; no production credentials; `demo_orders` deferred.
- No rewrite. Prefer small safe migrations.
- No new strategies before the spine is closed and lean.
- The productized UI suite (Event Workbench, Ask FORTUNA) beyond the one chain-view is deferred to post-v1.

## 8. Risks & open questions

- **Ensemble cost:** 9 areas × (auditor+adversary) + verifier + lead ≈ 20 agents. Accepted for thoroughness; Areas 8+9 may merge to halve the cognition slice if desired.
- **F0 / I7 boundary:** auto-persisting calibration in paper must be unambiguously scoped as "not capital promotion."
- **Aeolus feed fragility** (ephemeral tunnel + static dates) must be hardened before a multi-week soak.
- **Stale DB pointer** (`current-demo-db-url`) and dual mode model are confirmed; Phase B resolves them.

## 9. Definition of done (this initiative)

`fortuna start paper-demo` boots a lean, single-trunk system on live Kalshi data with no order path; the closed loop runs and **settles + scores**; all strategies accrue paper data; one view shows the chain; calibration accrues so the model arm trades; and the atlas + MVP closure plan + branch/doc ledgers exist as the durable ground truth.