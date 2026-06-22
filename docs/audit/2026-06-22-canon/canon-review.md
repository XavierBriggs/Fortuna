# FORTUNA Canon Review (Phase 1) — v2, against `main`

Date: 2026-06-22. Target: `main` worktree `/Users/xavierbriggs/fortuna-main`, HEAD `1bb6959`. Supersedes the v1 review (which examined the `feature/ws3-generic-backtest` worktree at `a70daee`). Reviewer: Claude Code (main loop) over parallel evidence agents, with main-loop re-verification of every load-bearing claim.

## What changed since v1 (the delta this re-run covers)

- **40 commits on `main` since baseline `a70daee`**, almost entirely additive: 95 files, +15,581 / −69. WS1-WS4 work has landed on `main`.
- **One new crate: `fortuna-backtest`** (WS3 generic backtest / validation subsystem, ~6,400 LOC). Crate count 17 -> **18**.
- Churn since baseline by crate: backtest (new, 22 files), ledger (33), live (12), scoring (9), cli (8), ops (8), killswitch (2), invariants (1, additive).
- **The safety-core crates are byte-unchanged since baseline**: gates, exec, venues, core, state, cognition, runner, paper, sources, recorder all show 0 changed files. The v1 findings for I1/data-flow/money/determinism on those crates therefore stand unchanged and are carried forward; spot-confirmed by 0-diff.
- New evidence files this run: `scratch/evidence/{k-backtest, l-ledger-integrity, m-doc-drift-v2, n-prior-gaps-delta}.md`. The v1 evidence files `a-h` (in the ws3 worktree scratch) remain valid for the unchanged crates.

## How to read this report

Same conventions as v1: every claim cites `path:line`; as-built vs as-intended distinguished; `docs/spec.md` v0.9 is the authority. The audit-replay gaps from v1 are, per operator steer, treated as defects bound for `GAPS.md`, not as accepted as-built behavior in canon.

---

## 0. Headline findings (v2)

1. **Safety core unchanged and sound.** I1-I7 enforced and tested; decision/execution split type-enforced (`GatedOrder` sealed); money and determinism clean. The crates that carry these guarantees did not change since v1 (0-diff), so the conclusions hold by construction.
2. **The new backtest subsystem is well-built and quant-rigorous.** `fortuna-backtest` is read-only / paper-safe (no order path, archive opened read-only), determinism-positive (FNV-1a content-hash `run_id`, injected clock, idempotent rerun, DST-covered), and its four gates (G-PIT, G-DEAD, G-PARITY, G-TRUTH) are wired and asserting. The overfitting controls are real: purge/embargo is wired into the production `validate` path, DSR deflates against the joint family trial count, and a leak-trap is proven directionally.
3. **The recorded WS3 guardian gaps are fixed in code but stale-open in GAPS.md.** `GAPS.md:225-248` still lists G1 (placeholder edge-provider / purge-not-wired) and G2 (decoupling not test-enforced) as open; both shipped fixes (real `LedgerEdgeProvider`, `decoupling.rs`). This is GAPS staleness, not a live code gap.
4. **All four v1 accountability gaps are STILL PRESENT on main.** `prompt_hash` still unrecorded (now at three production stamp sites); the protected-invariant guard still scans only `tests/`; `InstrumentKind` still vestigial (now explicitly admitted in ASSUMPTIONS.md); daemon audit rows still `actor=NULL`. None were addressed by the 40 commits.
5. **Doc drift got worse.** Canonical docs still say "Sixteen crates" (`README.md:165`, `architecture.md:169`, `system-e2e-overview.md:16`) while the workspace has 18; the new `fortuna-backtest` and the pre-existing `fortuna-scoring` are invisible in every canonical doc; the dead `fortuna-cognition/scoring.rs` reference (`system-e2e-overview.md:151`) persists. The cleanup is more justified than at v1.
6. **I5 still holds, with one additive-test gap.** The new `validation_runs` table is append-only (DB trigger), but the protected I5 invariant test does not enumerate it (or scorecards/trade_scores/bus_recordings); those are proven only by per-crate tests.

---

## a. Repo and workspace map (updated)

Single Rust workspace, **18 member crates** (`Cargo.toml:3-21`), resolver 2, toolchain pinned `stable` + rustfmt/clippy (`rust-toolchain.toml`). Still no `rustfmt.toml`/`clippy.toml`/`deny.toml`. CI (`ci.yml`, `invariants-dst.yml`) unchanged and still latent (no remote).

New crate row (the other 17 are as in v1; see carried-forward evidence):

| Crate | Layer | Responsibility | Key path-deps | Bin? | Files changed since baseline | LOC |
|---|---|---|---|---|---|---|
| fortuna-backtest | (validation) | generic point-in-time, overfitting-deflated backtest/validation over any `HistoricalSource`; emits GO/NO-GO | core, scoring, ledger; sources via `HistoricalSource` | no (driven by cli) | new (22) | ~6,402 |

`fortuna-backtest` is consumed only by `fortuna-cli` (`run_backtest`/`run_validate`, registered `fortuna-cli/src/main.rs:1193-1233`); nothing in the live runtime depends on it. Dependency graph remains an acyclic DAG.

(Evidence: `scratch/evidence/k-backtest.md`; v1 `a-workspace.md` for the unchanged 17.)

---

## b. Subsystem inventory (delta: new validation subsystem)

The codename resolution from v1 is unchanged and confirmed: Iris/Mercury/Atlas/Artemis/Nemesis/Nike are Olympus lineage, not subsystems; only Aeolus (signal source) and Kinetics (perps, still half-wired) are real. The 18 `fortuna-*` crates are the subsystems.

### New subsystem: `fortuna-backtest` (WS3)

| Aspect | Finding | Evidence |
|---|---|---|
| Responsibility | Replays any `HistoricalSource` through the SAME scoring rules and ledger write path as live, producing an overfitting-deflated GO/NO-GO. Validates deterministic components (forecast probabilities -> recomputed Brier/CLV), never LLM-decision PnL (spec Principle 4). Brier-skill is the gated headline; CLV corroborates; Sharpe/DSR is walled-off context. | `lib.rs`, `harness.rs:257` |
| Boundary contracts | `HistoricalSource`/`Source` (read-only ingest), `EdgeProvider` trait (`edge_provider.rs:239`, `windows()` at `:313` supplies purge `LabelWindow`s). Only source-coupled code is `AeolusArchiveSource` (`sources/aeolus_archive.rs`). | `edge_provider.rs:239,313` |
| Parity with live | Uses the same `scorecard_agg::assemble_from_samples` and the same repos as live -> parity by construction (G-PARITY). | `harness.rs:257`, `edge_provider.rs:268` |

Boundary issues from v1 unchanged (`InstrumentKind` vestigial — see d/j; ingest crate split; `MarketView` placement). No new leaky boundary introduced by backtest: it imports no venue/exec/gate crate (grep clean). (Evidence: `scratch/evidence/k-backtest.md`, `b-subsystems.md` v1.)

---

## c. Data flow and decision/execution split (delta: backtest is read-only)

The live data-flow path (12 hops) is unchanged from v1 (the crates did not change). The new backtest path is a separate, offline, read-only branch:

archive (read-only) -> `HistoricalSource` -> as-of join (G-PIT) -> recomputed beliefs/scores -> sweep + deflation -> `validation_runs` row + GO surface. No `GatedOrder`, no `Venue::place`, no live submission.

**I1 verdict for the new surface: SAFE.** `fortuna-backtest` constructs no `GatedOrder` and imports no venue/exec/gate crate; the archive is opened `SQLITE_OPEN_READ_ONLY`; `HistoricalTrade::new` machine-rejects `orders != 0`. The CLI `backtest`/`validate` subcommands write only beliefs and a `validation_runs` row; the sole venue path (`start paper-demo`) hard-asserts `execution_mode == "paper_ledger"` and fails closed (`fortuna-cli/src/main.rs:545-577`). So a backtest cannot place a real order. (Evidence: `scratch/evidence/k-backtest.md`, `n-prior-gaps-delta.md`.)

---

## d. Invariants audit (delta)

The I1-I7 + perp + unnumbered table from v1 stands (enforcement crates unchanged). Deltas:

- **I4 strengthened.** The one changed invariants file is `crates/fortuna-invariants/tests/i4_killswitch_revocation.rs` (two new `#[test]` fns + one `use`), strictly additive. Killswitch gained an additive `RevocationGuard` three-way fail-closed re-arm guard. (Evidence: `n-prior-gaps-delta.md`.)
- **I5 surface grew; still enforced.** New `validation_runs` table is append-only via `validation_runs_append_only BEFORE UPDATE OR DELETE ... fortuna_refuse_mutation()` (`migrations/20260621000002_validation_runs.sql:44-46`, re-verified). `fortuna_beliefs_guard()` and `fortuna_refuse_mutation()` are byte-identical to baseline. No new UPDATE/DELETE in `repos.rs` (the +582 lines are SELECT/INSERT only); the sole belief mutation remains the C1 set-once `resolve_and_score` (`repos.rs:1630`). (Evidence: `l-ledger-integrity.md`.)

### Coverage gaps (updated)

The six v1 gaps persist (re-verified: guard-script still `DIR="crates/fortuna-invariants/tests"`, `scripts/check-protected-invariants.sh:18`). One new gap:

| # | Gap | Status |
|---|---|---|
| 1-6 | v1 gaps (guard misses `src/lib.rs`; CI latent; I4 Slack/monthly untested; I7 unit-only; 2 unnumbered tests; ledger spine-purity assert omitted) | STILL PRESENT |
| 7 (new) | Protected I5 test `i5_audit_append_only.rs` is `audit`-table-specific; does not enumerate `validation_runs`/`scorecards`/`trade_scores`/`bus_recordings`. Those are proven only by per-crate tests (e.g. `fortuna-ledger/tests/validation_runs.rs:126-172`). Additive parametric I5 test recommended. | new, minor |

This table feeds the CONSTITUTION invariant-to-test gap list in Phase 2.

---

## e/f. Money and determinism (delta: backtest)

Carried forward clean from v1 for the unchanged crates. New crate:

- **Money in backtest: clean.** All money is `Cents`/i64; every `f64` is a probability or process metric (Brier/CLV/Sharpe/DSR) or a count cast. The one money-adjacent f64 (`yes_mid_cents`, a SQLite REAL read) converts at the boundary via `Cents::new(round() as i64)`. No float-on-money in any PnL/exposure/fee path. (Evidence: `k-backtest.md`.)
- **Determinism in backtest: positive.** `run_id` and all ids use a hand-rolled FNV-1a content hash (`harness.rs:406`), explicitly not `DefaultHasher` (grep confirms none). Clock is injected; ids are clock-independent; rerun is idempotent (`ON CONFLICT DO NOTHING`); `backtest_dst.rs` covers rerun-idempotency, partial-replay recovery, and clock-determinism. This subsystem improves the determinism posture rather than threatening it.

---

## g. Observability and audit reconstructability (delta)

Append-only substrate remains STRONG and now covers `validation_runs`. The v1 reconstruction gaps are unchanged and re-verified on main:

| Break | Status | Evidence |
|---|---|---|
| `prompt_hash` never recorded (spec 5.5:181 mandates it) | STILL PRESENT, now at 3 production stamp sites | `cognition/src/mind.rs:692-696`, `shadow.rs:109-114`, `discovery.rs:796-800` |
| daemon/runner audit rows `actor=NULL` | STILL PRESENT (file byte-unchanged) | `fortuna-live/src/audit_bridge.rs:103` |
| no audit-log-driven decision-replay tool | STILL PRESENT | `replay-verify.rs` (bus replay only) |
| successful order submits write no `audit` row (intent_events is the record) | unchanged (by design) | `runner.rs:1321-1332` |

New persistence: the validation GO surface (Brier headline + CLV + family_n_trials joint grid + PBO/SPA/MinTRL/DSR) is written via a single append-only INSERT (`ValidationRunsRepo::insert`, `repos.rs:3403`, called from `backtest_cmd.rs:260`); `run_id` is a pure function of inputs, so the validation record is reproducible. (Evidence: `g-observability.md` v1, `l-ledger-integrity.md`.)

---

## h. Existing-doc inventory and drift map (delta: worse)

| Prior item | Status on main | Citations |
|---|---|---|
| Crate count stated multiple wrong ways | WORSE: now 18 actual; canonical docs say "Sixteen" (2 behind) | `README.md:165`, `architecture.md:169`, `system-e2e-overview.md:16` vs `Cargo.toml` |
| `fortuna-scoring` invisible in canonical docs | STILL PRESENT | 0 grep hits |
| `fortuna-backtest` undocumented | NEW: invisible in spec/architecture/system-e2e/operator docs; CLI documented only in `docs/runbooks/ws4-demo.md` | 0 grep hits in canonical docs |
| Dead `fortuna-cognition/scoring.rs` ref | STILL PRESENT (file confirmed absent) | `system-e2e-overview.md:151` |
| I4 wording drift (Postgres) | STILL PRESENT | `spec.md:43` vs `CLAUDE.md:18`/`README.md:31` |
| FINAL_REPORT vs spec v0.8/v0.9 | STILL PRESENT | `FINAL_REPORT.md:3` |
| `docs/playbook.md` orphan | FIXED (now linked + classified authoritative) | `doc-triage.md:19,54` |
| GAPS.md G1/G2 (purge/decoupling) | STALE-OPEN: fixed in code, not retired in GAPS | `GAPS.md:225-248` vs `sweep.rs:344-349`, `validate_real_edges.rs:402-437`, `decoupling.rs` |

Good news: CHANGELOG/GAPS/ASSUMPTIONS track-own docs are otherwise consistent with code (the guardian findings are recorded, the InstrumentKind limitation is admitted at `ASSUMPTIONS.md:742`). The WS3 design spec (~143 lines) is canonical-quality and is the fold-candidate for ARCHITECTURE. (Evidence: `m-doc-drift-v2.md`.)

---

## i. Risk tiering (updated)

Unchanged from v1 except the new crate:

| Crate | Materiality | Churn | Tier | Rationale |
|---|---|---|---|---|
| fortuna-backtest | High (gates promotion) | new/high | **T2 High** | A wrong validation verdict is the mechanism by which a non-edge strategy could reach live capital (I7). Read-only, so no direct money path, but materially gates go-live decisions. |

All v1 tiers hold (T1: gates, killswitch, exec, venues, live, runner, ledger, invariants; T2: core, state, cognition, ops, +backtest; T3: scoring, sources, paper, cli; T4: recorder).

---

## j. Tech-debt and go-live risk hotspots (updated, ranked)

1. **Audit replay still not exact (no `prompt_hash`, no replay tool, `actor=NULL`).** Unchanged top item; per operator steer it is logged as a defect in GAPS.md. (`mind.rs:692`, `audit_bridge.rs:103`.)
2. **Protected-invariant guard scope hole.** Still scans only `tests/`; the I1 doctests in `src/lib.rs` remain weakenable without tripping CI. (`check-protected-invariants.sh:18`.)
3. **CI invariant gate latent (no remote).** Unchanged.
4. **Perps (Kinetics) still half-wired.** `InstrumentKind` still vestigial, now explicitly admitted (`ASSUMPTIONS.md:742`). Decision pending (see open questions).
5. **GAPS.md staleness (new).** G1/G2 list fixed work as open; if operators triage from GAPS, fixed items read as outstanding and outstanding items lose signal. Retire resolved entries.
6. **Doc drift worsened.** 18 crates, docs say 16; two crates invisible; a dead file ref. The canon cleanup is the fix.
7. **I5 protected test does not cover new append-only tables (new).** Per-crate tests cover them; the protected harness does not. Additive parametric test recommended.
8. **No `rustfmt.toml`/`clippy.toml`/`deny.toml`.** Unchanged.

---

## Findings summary (v2)

- The 40-commit delta is overwhelmingly additive and high-quality: a new, decoupled, read-only, deterministic, overfitting-rigorous validation subsystem that does not touch the safety core and does not open an order path.
- The safety core is unchanged, so its guarantees hold by construction.
- None of the four v1 accountability gaps were closed; doc drift widened; GAPS.md now carries stale-open entries. These are the cleanup targets, not new risk introduced by the delta.
- The largest as-built/as-intended gap remains perps (Kinetics), unchanged.

## Open questions (v2)

Carried from v1 (still unresolved): `prompt_hash` deferral vs defect; perps finish vs fence; `aeolus_eval`/`synth_events` strategy realization; extend the guard to `src/lib.rs`; where the decision-replay tool belongs; `compose.rs` f64 basis fields; `actor=NULL` policy; ledger spine-purity exception. New:

9. Should the protected I5 test be made parametric over all append-only tables (so new tables are auto-covered), or is per-crate coverage acceptable?
10. GAPS.md G1/G2 are fixed in code: retire them now, or keep until an operator confirms the residual recal-label caveat is acceptable?
11. The backtest recal-method grid labels (Platt/Isotonic/None) are illustrative while the applied transform is temperature scaling. Acceptable disclosure, or should the grid be made faithful before the validation surface is trusted for promotion?

## Proposed canon table of contents (v2 adjustments)

The closed set is unchanged. Scope adjustments forced by the delta:

- **ARCHITECTURE.md**: the crate map is now the 18-crate map (single source of truth, ending the 16-vs-18 drift), and must add the `fortuna-backtest` validation subsystem and the validation/GO-surface data path; fold the WS3 design spec here.
- **CONSTITUTION.md**: I5 surface now includes `validation_runs` append-only; note the (recommended) parametric I5 test as the named test target.
- **STANDARDS.md**: add the backtest determinism discipline as a house rule (content-hash ids via FNV-1a not `DefaultHasher`, point-in-time strict `<`, leak-trap, purge/embargo wired before a GO surface is trusted).
- **STATE.md (generated)**: must surface validation-run GO/NO-GO status per strategy and the perps "data-collection-only / not production-wired" state.
- **decisions/**: add an ADR for the overfitting-control methodology (Brier-primary GO surface; DSR deflated against the joint family trial count; purged/embargoed CSCV/PBO; Hansen SPA_c; MinTRL/effective-N) — a significant, now-implemented decision.
- **canon.manifest + CI**: per operator steer, both checks, advisory in CI, runnable locally now.

The other docs (README, NORTH_STAR, .claude/CLAUDE.md) keep their v1 scope.

---

*End of Phase 1 review v2. New evidence: `scratch/evidence/{k-backtest, l-ledger-integrity, m-doc-drift-v2, n-prior-gaps-delta}.md`. Carried-forward (unchanged crates): v1 `scratch/evidence/{a..h}` in the ws3 worktree.*
