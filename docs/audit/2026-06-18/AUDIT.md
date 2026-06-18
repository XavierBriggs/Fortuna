# Deep Codebase Audit — FORTUNA

**Date:** 2026-06-18 · **Branch audited:** `feature/paper-on-live-data` (working tree) · **Live DB:** `fortuna_demo`
**Method:** 9 ensemble area-audits (auditor + self-adversary) + independent Verifier (38 findings checked, 35 upheld) + branch ledger + doc triage. Per-area detail in `docs/audit/2026-06-18/area-*.md`; verdicts in `verification.md`.

---

## 1. Executive Summary

### Bottom line
FORTUNA is **not a broken or unsalvageable system** — it is a **safe, well-tested engine whose persistence and operator surfaces were never wired up**, obscured by multi-agent fragmentation. The safety core is genuinely solid: **all 7 invariants (I1–I7) are code-enforced, 34 invariant tests + 6 doc-tests pass, the DST corpus (15 seeds + 500 random) runs with zero violations, there is no `GatedOrder` bypass, no `unwrap/panic` in money paths, no `f64` money, and no constructible real-order path in paper mode.** This earns **fix, not rebuild**.

### MVP readiness: **NOT YET — but the gap is bounded and single-rooted.**
The loop runs left-to-right (signal → belief → proposal → gate → paper-fill) but **does not close**: outcomes are computed in memory and **never persisted**, so there is no realized PnL, no calibration, and the model arm never wakes. There is no single clean demo command, and the operator dashboard does not show the safety state.

### Biggest risk (the one root cause)
**"Computed but never persisted."** Four ledger repos have working `insert` methods with **zero production callers**:
| Repo | DB table | rows in `fortuna_demo` | production callers |
|---|---|---|---|
| `SettlementsRepo::insert_entry` | `settlement_entries` | **0** | none (P0) |
| `CalibrationParamsRepo::insert` | `calibration_params` | **0** | none (P0) |
| `FillsRepo::insert` | `fills` | **0** | none (P1) |
| bus recording (`recording_jsonl`) | — | not persisted | dropped at shutdown (P1) |

Settlement runs in an in-memory `SettlementLedger` (`runner.rs:1918`); calibration is fit by the weekly review then discarded (`daemon.rs:4222`); fills are audited but the `fills` table stays empty. **Wiring these four `insert` calls is the bulk of "close the loop."**

### Best next move
Phase B consolidation is cheap and safe (branch ledger: only 2 stranded branches, both doc-only; doc triage: clear archive set). Then Phase C is a small, well-defined set of "wire the persistence + enable the surfaces" tasks. **Target: `fortuna start paper-demo` that closes the loop and accrues data.**

---

## 2. What The System Currently Is

A ~130k-LOC Rust workspace (≈51% tests/examples — appropriate for a trading system; **not bloat-by-size**) across 16 crates implementing: live Kalshi/Kinetics market-data ingestion, a cognition stack (Mind trait + tiers + budget, discovery, personas, calibration, scoring), 5 strategies, a deterministic 10-check gate pipeline producing a sealed `GatedOrder`, a paper venue with through-not-touch fills, a Postgres ledger, a ROTA dashboard, and a standalone kill switch. It runs today as a **paper-on-live** daemon (live prod reads, local paper fills, no real order path).

**What runs:** ingestion (27.5k signals), belief formation (108 weather beliefs via the deterministic Aeolus path), edge minting (108 edges), proposals (mech_extremes), the gate pipeline (audited), and paper fills (in the journal). **What doesn't close:** settlement, calibration, fills-table persistence, the model/synthesis arm, personas, world-forward → belief.

## 3. Target System And North Star

`fortuna start paper-demo` → one command boots the closed **paper** loop on **live Kalshi data**, no constructible order-mutation path, all strategies in paper, every decision flowing `signal → belief → mapping → proposal → gate → paper-fill → settlement → score`, one view showing the chain, **data accruing** to hone strategies — on a path toward $50k validated PnL. (Full target in `docs/superpowers/specs/2026-06-18-ground-truth-audit-and-demo-readiness-design.md`.)

## 4. Architecture Map

```
sources(NWS/Aeolus/BLS/Fed/Kalshi/Kinetics) ─► fortuna-sources ─► signals(27.5k)
  ├─ fortuna-cognition: Mind(opus/sonnet/haiku)+budget · discovery(world-fwd/market-back) · personas[OFF]
  │                     · beliefs(108, all aeolus) · calibration[fit-but-not-persisted] · scoring
  ├─ fortuna-runner: 5 strategies · tick()/book-poll · synthesis[calibration-gated→silent]
  ├─ fortuna-gates: 10 checks ─► sealed GatedOrder  (I1, invariant-proven)
  ├─ fortuna-exec / fortuna-paper: IntentJournal · PaperLiveVenue(live reads + LOCAL paper fills)
  │                     · settlement[in-memory only] · fills[not persisted]
  ├─ fortuna-ledger(Postgres): events/signals/beliefs/edges/audit ✓ · settlement/calibration/fills [empty]
  ├─ fortuna-ops: ROTA(~26 sections) [no mode/order-mutation display]
  └─ fortuna-killswitch: standalone, no Postgres (I4) ✓
```
Largest files (legibility targets): `daemon.rs` 4854, `repos.rs` 2479, `rota.rs` 2227, `runner.rs` (large), `drive()` ~20-param signature.

## 5. Critical Path Analysis

| Path | Status | Evidence | Required fix |
|---|---|---|---|
| Market data → snapshot → strategy | ✅ wired (REST-polled) | `runner.rs:827`; WS client exists (`kalshi/ws.rs`) but not on book path | (P2) stream WS into book |
| Decision → proposal | ✅ wired | mech_extremes proposals in `fortuna_demo` | — |
| Risk gate → sealed GatedOrder | ✅ wired + invariant-proven | I1 tests pass; no bypass | — |
| Execution → fill/cancel/ack (paper) | 🟡 fills not persisted | `FillsRepo::insert` 0 callers; `fills`=0 | **P1 wire FillsRepo** |
| Accounting → settlement → PnL | ❌ broken | settlement in-memory `runner.rs:1918`; `settlement_entries`=0; calibration `calibration_params`=0 | **P0 wire Settlements + Calibration** |
| Replay | 🟡 partial | DST replay ✓; live bus recording dropped (`ShutdownReport` has no `recording_jsonl`, `runner.rs:90`) | **P1 persist recording** |

## 6. Highest Risk Findings (verified)

| Sev | Finding | Evidence | Readiness | Fix |
|---|---|---|---|---|
| **P0** | Settlement in-memory only; `settlement_entries`=0 | `runner.rs:1918`; `SettlementsRepo::insert_entry` 0 prod callers | BLOCKS | wire settlement persistence |
| **P0** | Calibration fit then discarded; `calibration_params`=0 → synthesis never sizes | `daemon.rs:4222`; `review.rs:101`; `CalibrationParamsRepo::insert` 0 prod callers | BLOCKS | persist fitted calibration in `paper` stage |
| **P1** | Fills not persisted; `fills`=0 | `FillsRepo::insert` 0 prod callers | BLOCKS | wire FillsRepo on fill-applied |
| **P1** | Live bus recording not persisted → live replay impossible | `runner.rs:90` (ShutdownReport lacks `recording_jsonl`) | BLOCKS | thread recording to durable store |
| **P1** | No `fortuna start paper-demo` command | `main.rs:8-17`; `demo-launch.sh` 190 lines | BLOCKS | add the CLI verb |
| **P1** | ROTA Health omits `execution_mode`/`order_mutation_enabled` | `daemon.rs:1421-1438`; `rota.rs:2107` | BLOCKS | emit + render safety pills |
| **P1** | Persona charter never injected (model gets synthesis charter) | `main.rs:474` `synthesis_mind.clone()`; `persona_runner.rs:213` | BLOCKS (personas) | build per-persona Mind w/ `def.method` |
| **P1** | Personas config-OFF + registry empty (boot fails if enabled) | no `[personas]`; `personas`=0; `domain_analyses`=0 | BLOCKS (personas) | seed registry + enable + budget |
| **P1** | Synthesis calibration-gated (depends on P0 calibration) | `daemon.rs:355-375`; beliefs 100% `aeolus` | BLOCKS (model arm) | follows calibration persistence |
| **P1** | World-forward unscoreable trap: 16/20 watch events, 0 beliefs | `discovery.rs:689` `registry.get(...).unwrap_or(true)` exact-matches machine IDs vs prose resolution_source | BLOCKS (world-fwd) | normalize resolution_source / fuzzy match |
| **P1** | funding INSERT permission denied (since fixed; recurs on fresh DB) | `daemon.log:32`; grant now present | SERVES | add GRANT to demo-launch runbook |
| Minor | clippy `collapsible_if` (new in branch) | `daemon.rs:1681` | BLOCKS (DoD) | trivial fix |

**Root-cause grouping:** (A) *computed-but-never-persisted* → 4 findings (the loop closer). (B) *built-but-not-wired/enabled* → paper-demo CLI, ROTA mode display, personas (charter + registry + enable), world-forward registry trap. (C) *consolidation/legibility* → §12.

## 7. Vendor Coupling Report
Boundary is **structurally sound** — strategies, gates, and execution are clean of vendor types. Leakage is contained to the **F7 weather discovery seam** in `fortuna-live`: `KalshiMarket`/`KalshiMarketStatus` DTOs cross into `aeolus_venue.rs:42`/`daemon.rs:2276,2360`; `WeatherMarketSource` trait lives in the Kalshi namespace; sim hardcodes `fees.get("kalshi")` (`daemon.rs:306`). All P2, none block the (Kalshi-only) demo; they block future multi-venue.

## 8. Test Gap Report
Strong base: DST corpus (verified replays all seeds), invariant suite (34+6 pass), venue contract/fixture tests, mind 19/19. Gaps (all P2): no WS `trade`-frame fixture (paper-fill realism proven via REST only); PnL not rebuildable from Postgres events (in-memory tests only); calibration persistence round-trip untested end-to-end; **stale-book does NOT block at the gate** (only sets a wide-mark flag — no `BookAge` gate check).

## 9. Security And Secrets Review
No `unwrap/expect/panic` in money paths; no `f64` money; no `SystemTime::now` outside clock impls (binaries/recorder excepted per CLAUDE.md); secrets via env, not config; append-only audit DB-enforced (`fortuna_refuse_mutation`). One ops issue: `is_revoked()` uses `path.exists()` → fail-open on FS permission error (`killswitch lib.rs:258`, P2, mitigated by `PgHaltPoller`).

## 10. Operational Readiness
Kill switch ✓ (standalone). Gaps: no clean demo CLI; ROTA hides the safety state; operator `rearm` verb unbuilt; `current-demo-db-url` pointer stale (caused a real mis-analysis); funding GRANT missing on fresh DB; dead-man ping observed failing once (re-armed). WS reconnect backoff exists (`kalshi/dial.rs`).

## 11. MVP Closure Plan
→ **`docs/audit/2026-06-18/MVP-CLOSURE-PLAN.md`** (this is Phase C, authored from the findings above).

## 12. Refactor Roadmap (Phase B — consolidation, authored from findings)
1. **Branch cleanup (safe):** 11 absorbed branches → delete (archive-tagged); 1 redundant → delete; 2 stranded (`track-d`, `track-e-docs-freshen`, doc-only) → cherry-pick the GAPS/BUILD_PLAN freshening into trunk, then delete. Promote `feature/paper-on-live-data` → `main`.
2. **Doc collapse:** archive `docs/reviews/` (54) + `docs/research/*/raw/pages/` (179) + root `AMENDMENT-*.md`; prune `GAPS.md` 5858→~100 (open items only); pick one source-of-truth per topic.
3. **Dead/duplicate:** resolve the dual mode model to one authoritative axis; clean stale DBs; remove/wire the `AnthropicVetoMind` stub.
4. **Legibility splits (no behavior change, test-gated):** `daemon.rs` (→ composition/drive-loop/tasks/rota-snapshot), `repos.rs` (per-aggregate repo files), `rota.rs` (queries → ledger crate), a `DriveContext` for `drive()`'s 20 params.

## 13. Next Three Moves
1. **Phase B consolidation** (branches + docs + dual-mode) — cheap, safe, removes the fragmentation that hides the system.
2. **Close-the-loop wiring** (root cause A): persist settlement, calibration (paper stage), fills, bus recording — the 4 `insert` calls. → realized PnL + model arm wakes.
3. **The demo surface** (root cause B): `fortuna start paper-demo` + ROTA mode/freshness pills + the one chain-view. → demo-paper-ready, accruing data.

---

## Demo-Paper-Ready Readiness Scorecard

**Verdict: NOT demo-paper-ready. Distance = root-cause A (4 persistence wires) + root-cause B (demo surface + personas) + the clippy fix.** No rebuild required.

| # | BLOCKS finding | Phase C task | Sev |
|---|---|---|---|
| 1 | Settlement never persisted → no realized PnL | wire `SettlementsRepo` on settlement | P0 |
| 2 | Calibration never persisted → model arm dead | persist fitted calibration in `paper` | P0 |
| 3 | Fills never persisted (`fills`=0) | wire `FillsRepo` on fill-applied | P1 |
| 4 | Bus recording dropped → no live replay | thread `recording_jsonl` to durable store | P1 |
| 5 | No single demo command | `fortuna start paper-demo` | P1 |
| 6 | ROTA hides safety state | emit/render `execution_mode` + `order_mutation_enabled` | P1 |
| 7 | Personas inert (charter + registry + OFF) | per-persona Mind charter + seed registry + enable | P1 |
| 8 | World-forward watch events unscoreable | fix `resolution_source` match (`discovery.rs:689`) | P1 |
| 9 | funding GRANT missing on fresh DB | add GRANT to demo-launch runbook | P1 |
| 10 | clippy error fails DoD | fix `collapsible_if` (`daemon.rs:1681`) | Minor |

**SERVES (already good):** all 7 invariants enforced + tested; no real-order path in paper; DST green; sealed gate; clean money paths; sound adapter boundary; Mind trait works; branch/doc consolidation is safe.
