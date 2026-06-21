# WS3 Generic Backtest Subsystem — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (or the Hephaestus loop) to implement this plan task-by-task (builder→verifier per slice). Steps use checkbox (`- [ ]`) syntax.

**Goal:** A self-contained `fortuna-backtest` crate that replays any historical source through the *same* WS1/WS2 scoring and produces an honest, overfitting-deflated GO/NO-GO, behind four FORTUNA-side integrity gates (G-PIT, G-DEAD, G-PARITY, G-TRUTH).

**Architecture:** New decoupled `fortuna-backtest` crate (`HistoricalSource` trait → portable JSONL generic records → idempotent/deterministic replay harness → same `fortuna-scoring` rules + same ledger write path). The deflation math is a pure sub-library in `fortuna-scoring` (reusable, deterministic); orchestration in `fortuna-backtest`. `AeolusArchiveSource` is the only source-coupled code.

**Tech Stack:** Rust 2021; `serde`/`serde_json` (JSONL); `thiserror`; `sqlx` (ledger, compile-checked + `SQLX_OFFLINE`); `rusqlite` + streaming (Aeolus adapter); `fortuna-scoring` (pure math); the injected `Clock`.

**Authority:** SPEC `docs/superpowers/specs/2026-06-21-ws3-generic-backtest-design.md`; RESEARCH `docs/research/2026-06-21-ws3-backtest-overfitting-grounding.md` (exact formulas — the implementer MUST read it for S4). Invariants I1–I7 absolute.

## Global Constraints
- Rust 2021; money is integer `Cents`/`i64`; probabilities `f64` in scoring only.
- **No `panic!`/`unwrap`/`expect`** in non-test code (esp. money/replay paths); `thiserror` per crate.
- **All time via the injected `Clock`**; `SystemTime::now()` outside `Clock` impls is a defect.
- `sqlx` compile-checked; run DB tests/clippy with `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp`.
- Append-only (I5): replayed rows + `validation_runs` INSERT-only, guarded by `fortuna_refuse_mutation`.
- **Decoupling:** `crates/fortuna-backtest/src/` carries NO source-name literals (grep gate, mutation-proven). `AeolusArchiveSource` is the only coupled code; the A7 guard excludes it.
- `crates/fortuna-invariants/` is **additions-only** — never weaken an existing assertion.
- `cargo fmt --check`; `clippy --all-targets -- -D warnings` clean.
- Per-slice gates are TARGETED (only the slice's tests); the FULL workspace battery + the four integrity-gate executable tests + DST run at the WS3 boundary.

## Slice map (→ spec §)
- **S1** contracts: `HistoricalSource` + generic records + JSONL + manifest (§3, §4).
- **S2** replay harness: as-of join, G-PIT, idempotent, deterministic, G-PARITY (§5 G-PIT/G-PARITY, §6).
- **S3** G-DEAD: engaged-set manifest + voided/NO-present check (§5 G-DEAD).
- **S4** deflation library in `fortuna-scoring`: purged+embargoed CSCV→PBO, Hansen SPA_c, effective-N+MinTRL, DSR (§5 G-TRUTH, §7, §8; RESEARCH).
- **S5** sweep driver + `validation_runs` + the G-TRUTH GO surface (§7).
- **S6** `AeolusArchiveSource` (§9).
- **S7** CLI `fortuna backtest` + `fortuna validate` (§10).
- **Boundary**: the four integrity-gate executable tests + DST + full battery (§11).

---

### S1: Source/Record contracts + portable serialization + manifest

**Files:** Create `crates/fortuna-backtest/Cargo.toml` (deps: serde, serde_json, thiserror, fortuna-core for `UtcTimestamp`/`Cents`; dev: proptest), `src/lib.rs`, `src/records.rs`, `src/source.rs`, `src/manifest.rs`; `tests/records.rs`. Modify `Cargo.toml` workspace members.

**Interfaces — Produces:**
- `enum BeliefPayload { Binary { p: f64 }, Scalar { quantiles: Vec<(f64,f64)> } }`
- `struct Provenance { producer_type: String, producer_id: String, mind_id: Option<String>, mind_version: Option<i64>, strategy_id: String, category: String, scope: String }`
- `struct HistoricalBelief { provenance: Provenance, payload: BeliefPayload, event_linkage: String, available_at: UtcTimestamp, decided_at: UtcTimestamp }`
- `struct HistoricalOutcome { event_linkage: String, outcome: f64, resolved_at: UtcTimestamp, resolution_source: String }`
- `struct HistoricalSnapshot { market: String, price: Cents, at: UtcTimestamp }`
- `struct HistoricalTrade { /* … */ orders: u32 /* invariant: always 0 */ }`
- `struct UniverseManifest { engaged: Vec<EngagedMarket> }`, `struct EngagedMarket { event_linkage: String, resolved: bool, voided: bool }`
- `trait HistoricalSource { fn beliefs/outcomes/snapshots/trades(&self) -> impl Iterator<Item=Result<…,SourceError>>; fn universe_manifest(&self) -> Result<UniverseManifest,SourceError>; }` (sync streaming iterators; `enum SourceError` via thiserror). **M2 note (document in the trait):** spec §4 prose shows `impl Stream` (async); we deliberately use **sync `Iterator`** — replay is deterministic/single-threaded with no async in the core loop and `rusqlite` is sync. State this in the trait doc so a reviewer doesn't read it as drift.
- Module doc on `source.rs` states the **bitemporal invariant**: `available_at` is KNOWLEDGE time (`fetched_at`), never event/observed/`target_date`; post-resolution fields carry `available_at = resolution time`. The **canonical `event_linkage` namespace** is documented here (the join key; Aeolus↔future-producer reconciliation rule).

**Failing tests (TDD):**
- `belief_jsonl_round_trips`: serialize a `HistoricalBelief` (binary + scalar) to one JSONL line, deserialize, assert equality.
- `outcome_snapshot_trade_round_trip`; `trade_orders_is_zero` (the paper-only invariant — `HistoricalTrade` constructor/validator rejects `orders != 0`).
- `manifest_round_trips` + `manifest_marks_voided_and_resolved`.
- proptest: round-trip stability over arbitrary records.

**Algorithm:** plain structs with `#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]`; JSONL = one `serde_json::to_string` per record. No logic beyond the `orders==0` validator.

**Acceptance / gate:** `cargo test -p fortuna-backtest --test records` green; `cargo clippy -p fortuna-backtest --all-targets -- -D warnings`; `! grep -rnE '"aeolus"|"meteorologist"|"kalshi"|"historical-import"' crates/fortuna-backtest/src/` (mutation-proof: plant `"aeolus"` in a doc-comment → grep catches → revert). Commit.

---

### S2: Replay harness — as-of join (G-PIT), idempotent, deterministic, G-PARITY

**Files:** Create `crates/fortuna-backtest/src/harness.rs`, `src/asof.rs`; `tests/harness.rs`, `tests/gpit.rs`. Depends on `fortuna-scoring`, `fortuna-ledger`, `fortuna-core` (Clock).

**Interfaces — Consumes** S1 records. **Produces:** `struct ReplayHarness<C: Clock>` with `fn replay(&self, source: &impl HistoricalSource, range: TimeRange) -> Result<ReplayReport, ReplayError>`; `fn asof_join(beliefs, snapshots, outcomes, as_of: UtcTimestamp) -> DecisionContext` (the as-of assembly); `struct ReplayReport { written: usize, skipped_idempotent: usize, look_ahead_rejected: usize }`.

**Failing tests (TDD):**
- `gpit_strict_excludes_equal_timestamp` (BVA — the load-bearing one): a record with `available_at == decided_at` is EXCLUDED; assert `look_ahead_rejected` counts it; a scratch mutation of `<` → `<=` reds this test.
- `gpit_rejects_future_data`: `available_at > decided_at` → excluded + counted.
- `asof_picks_latest_prior_snapshot`: CLV-entry snapshot = latest with `at < decided_at`.
- `replay_is_idempotent`: replay twice → second run writes 0 (content-hash / ON CONFLICT), `skipped_idempotent` == first run's `written`.
- `replay_is_deterministic`: same source + injected `SimClock` → identical ledger rows (no wall-clock).
- `replay_stamps_historical_import`: every written row has `provenance.source == "historical-import"` + preserved original timestamps.
- **`parity_seam_backtest_equals_live` (G-PARITY — must invoke the REAL paths, not a twin assemble call; Important#1):** construct N (belief, outcome, snapshot) records; score them BOTH via (a) the **live scoring path** — `fortuna_cognition::scorecard_agg::assemble_from_samples` (the same assembly the daemon's `recompute_scorecards` uses) — AND (b) `ReplayHarness::replay` against an in-memory `HistoricalSource` (which actually streams → as-of-joins → scores → produces the `Scorecard`/ledger rows); assert the two resulting `Scorecard`s are **byte-identical modulo `provenance.source`**. NOTE: the existing WS2 `scorecard_parity_seam` (tests/scorecard.rs:429) is a *pure twin `assemble_scorecard` call* and does NOT exercise a replay path — this test is the larger claim and MUST drive `ReplayHarness::replay`, else the gate has nothing to bite. Mutation: stubbing the replay path to just call the live assembler (or any divergence) reds it.

**Algorithm:** stream records; per decision, `asof_join` with **strict `available_at < decided_at`** (documented rule); feed `DecisionContext` to the *same* `fortuna-scoring` rules + the *same* ledger repos (source-stamped); idempotency via a content-hash unique key + `ON CONFLICT DO NOTHING`. No `panic`/`unwrap`.

**Acceptance / gate:** `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo test -p fortuna-backtest --test harness --test gpit -- --test-threads=1`; clippy; commit.

---

### S3: G-DEAD — engaged-set manifest + voided/NO-present

**Files:** Modify `src/harness.rs` (manifest enforcement); `src/manifest.rs` (coverage check). Test `tests/gdead.rs`.

**Interfaces — Produces:** `struct ScoredRow { event_linkage: String, outcome: f64, voided: bool }` (the per-market scored result the harness produces); `fn enforce_gdead(scored: &[ScoredRow], manifest: &UniverseManifest) -> Result<(), GDeadViolation>` — (a) every manifest-engaged market appears in `scored` (no silent drop) AND (b) voided + NO-resolved markets are present in `scored`.

**Failing tests (TDD):**
- `gdead_voided_present`: a manifest with a voided market → if `scored` omits it, violation; present → ok. (The load-bearing clause.)
- `gdead_no_resolved_present`: a NO-resolved (outcome=0) market must be in the scored set.
- `gdead_coverage_equals_manifest`: a dropped engaged market → violation.
- `gdead_unforecast_market_not_false_positive`: a market the producer never engaged (not in the manifest) does NOT trigger a violation. Mutation: dropping a voided row from `scored` reds `gdead_voided_present`.

**Algorithm:** set-difference of `manifest.engaged` vs scored event_linkages; separately assert the voided/NO subset ⊆ scored. Per spec §5 G-DEAD.

**Acceptance / gate:** `cargo test -p fortuna-backtest --test gdead`; clippy; commit.

---

### S4: Deflation library (pure) in `fortuna-scoring`

**READ FIRST:** `docs/research/2026-06-21-ws3-backtest-overfitting-grounding.md` — all formulas below are quoted there with citations. Implement them EXACTLY.

**Files:** Create `crates/fortuna-scoring/src/deflation/{mod.rs, cscv.rs, purge.rs, spa.rs, effective_n.rs, dsr.rs}`; `tests/deflation_*.rs`. Pure math — no IO, no Clock. **PURITY CONSTRAINT (Important#2): `fortuna-scoring/Cargo.toml` gains NO new dependency** — the crate's documented purity is "std + serde + thiserror only; no `rand`/`getrandom`/`libm`" (`lib.rs:6-8`; the `dm.rs` hand-rolled-erf precedent). The SPA stationary block bootstrap needs a seeded PRNG: define `trait SeededRng { fn next_u64(&mut self) -> u64; fn gen_range(&mut self, lo: usize, hi: usize) -> usize; }` in the deflation module and supply the concrete deterministic impl as a **hand-rolled SplitMix64 in-module** (no dep) — or let the *caller* (`fortuna-backtest`/tests) provide it. **`rand` MUST NOT enter `fortuna-scoring`.** Add a purity assertion (extend the existing decoupling/purity check, mutation-proofed) that `fortuna-scoring`'s dependency set is unchanged.

**Interfaces — Produces:**
- `purge.rs`: `fn purge_embargo(train: &[LabelWindow], test: &[LabelWindow], embargo: Duration) -> Vec<usize>` — indices to KEEP. Overlap = `train.t0 ≤ test.t1 && train.t1 ≥ test.t0`; embargo extends each test window to `t1+h` before the overlap test (one-sided).
- `cscv.rs`: `fn pbo(matrix: &Matrix, s: usize, label_windows: &[LabelWindow], embargo: Duration) -> PboReport` — purged+embargoed CSCV; `matrix[t][n]` = config n's forecasting-edge value on slice t; partition rows into S even submatrices; over all C(S,S/2) combos compute λ_c = ln(ω̄_c/(1−ω̄_c)) with ω̄_c = OOS-rank(IS-best)/(N+1); **PBO = fraction with λ_c < 0**. `PboReport { pbo, degradation_slope, prob_loss, n_logits }`.
- `spa.rs`: `fn spa_c(loss_diffs: &Matrix, block_len: usize, n_boot: usize, rng: &mut impl SeededRng) -> SpaReport` — Hansen `SPA_c`: studentized `T = max_k max(√n·d̄_k/ω̂_k, 0)`; recenter via `µ̂_k^c = d̄_k·1{√n·d̄_k/ω̂_k ≤ −√(2 ln ln n)}`; stationary block bootstrap (block ∝ n^(1/3)) → `p_c`. `SpaReport { statistic, p_c, p_l, p_u }`.
- `effective_n.rs`: `fn effective_n(series: &[f64]) -> f64` (AR(1) `N·(1−ρ)/(1+ρ)`, fall back to `N/(1+2Σρ_t)`); `fn mintrl(sr_hat, sr_star, skew, kurt, z_alpha) -> f64` (= `1 + [1 − γ3·SR + ((γ4−1)/4)·SR²]·(Z_α/(SR−SR*))²`).
- `dsr.rs`: `fn dsr(sr_hat, t, skew, kurt, trial_sr_variance, n_eff_trials) -> f64` (DSR with `SR0` = expected-max-Sharpe Gumbel; denominator uses `SR_hat`; γ4 raw → `(γ4−1)/4`).

**Failing tests (TDD) — the load-bearing ones:**
- `purge_drops_overlapping_train`: a train window overlapping a test window is dropped; non-overlapping kept.
- `embargo_drops_post_test_window`: a train window starting within `h` after a test window is dropped (one-sided — pre-test not dropped).
- **`purged_cscv_bites_on_known_overlap` (the test that proves purging works):** a fixture with deliberate same-slice overlap → with purging, PBO is materially higher than a no-purge mutation; assert `pbo_purged > pbo_nopurge + ε` (no-purge UNDERSTATES overfitting).
- `pbo_overfit_fixture_high` / `pbo_genuine_fixture_low`: a lucky-winner matrix → PBO ≈ 1; a genuinely-skilled config → PBO ≈ 0.
- `cscv_is_metric_agnostic`: feeding a Brier-skill matrix vs a Sharpe matrix runs identically (no metric assumption).
- `spa_c_studentized_and_recentered`: poor configs do NOT inflate the p-value (RC contamination test — add a terrible config; `p_c` ~unchanged; an RC-style un-recentered mutation degrades). Φ checks: a clear winner → `p_c < 0.05`; pure noise → `p_c` ~uniform/high.
- `spa_block_bootstrap_deterministic`: same seed → same `p_c`.
- `mintrl_matches_paper_worked_example`: pin the EXACT research §3 inputs (SR2-vs-1, daily, Normal — per-period SR_hat=2/√252, SR*=1/√252, skew=0, γ4=3, Z_α=1.645) → MinTRL ≈ 688 obs (≈2.73yr); `effective_n_ar1` (ρ=0.5 → 0.33·N).
- `dsr_denominator_uses_sr_hat` (guards the resolved contested point); `dsr_grows_with_t`, `dsr_shrinks_with_N`.

**Acceptance / gate:** `cargo test -p fortuna-scoring deflation`; clippy; commit. (Pure-crate purity grep still passes — deflation adds no IO.)

---

### S5: Sweep driver + `validation_runs` + the G-TRUTH GO surface

**Files:** Create `crates/fortuna-backtest/src/sweep.rs`; migration `crates/fortuna-ledger/migrations/2026062100000X_validation_runs.sql`; `crates/fortuna-ledger/src/repos.rs` (`ValidationRunsRepo`); extend the WS2 scorecard contract (`fortuna-scoring/src/scorecard.rs`) with the deflated view. Tests `tests/sweep.rs`, ledger `tests/validation_runs.rs`.

**Interfaces — Produces:**
- `struct TrialSpace { calibration_windows, recal_methods, scopes, go_thresholds }`; `fn run_sweep(...) -> ValidationRun`. **The trial count N is the JOINT scope × config grid (BLOCK-2 / research §6 cross-scope snooping):** when multiple scopes are validated, `n_trials = |scopes| × |configs|`, and the DSR/SPA `N` deflate against this **`family_n_trials`**, never a single scope's config count. (Romano–Wolf StepM family-wise control across the grid is a *recorded deferral*; the N-counting itself is NOT deferrable — it's the difference between a correct and an inflated deflation.) For each config compute the OOS edge series; assemble the CSCV matrix; call `pbo`, `spa_c`, `effective_n`, `mintrl`, `dsr`.
- **Verdict = the EXISTING `GoDecision { Go, NoGo, Insufficient }`** (reuse `fortuna-scoring/src/scorecard.rs`'s enum — do NOT add a fifth verdict type; M1). `fn decide(report) -> GoDecision` — **Brier is the PRIMARY gated metric (BLOCK-1; matches WS2's "Brier is the sole GO gate"):**
  - **GO iff** `N_eff` sufficient (≥30 + per-CSCV-fold coherent) **AND** the **Brier-skill** OOS edge > 0 **AND** `pbo` (computed on the **Brier-skill** matrix) ≤ 0.05 **AND** `spa.p_c` (on the **Brier-loss** differential) < α.
  - **CLV is a CORROBORATING axis ONLY** — reported with its own `pbo`/`spa`, it may *strengthen* the read but **never create a GO** (CLV's benchmark is a market price, not ground truth — research §6 proxy-truth caveat). A CLV-positive but Brier-flat config is **NOT** a GO.
  - **INSUFFICIENT** iff under-powered (`N_eff` < 30 / CSCV folds too thin); else **NO-GO**.
- `struct ValidationRun { run_id, scope, producer, trial_space, n_trials, family_n_trials, selected_config, brier_edge, brier_pbo, brier_spa_p, clv_edge, clv_pbo, clv_spa_p, effective_n, mintrl_ok, sharpe_dsr, verdict, computed_at }`; `ValidationRunsRepo::{insert, latest}`.

**Failing tests (TDD):**
- `verdict_go_requires_all`: drop any one of {N_eff ok, brier_pbo≤.05, brier_spa_p<α, brier_edge>0} → not GO (mutation-proof each conjunct).
- **`verdict_clv_cannot_rescue_brier_flat` (BLOCK-1):** a config with positive CLV but Brier-skill NOT beating baseline → **NoGo**; a mutation that ORs CLV into the GO condition reds it.
- **`sweep_n_trials_counts_scope_x_config_grid` (BLOCK-2):** validating K scopes records `n_trials = K × |configs|` and deflates DSR/SPA against `family_n_trials`; a mutation counting only one scope's configs reds it.
- `verdict_insufficient_on_thin_n`: N_eff<30 → `GoDecision::Insufficient` (NOT NoGo, NOT Go).
- `validation_runs_append_only`: UPDATE/DELETE rejected by `fortuna_refuse_mutation` (the WS2 scorecards pattern); insert + `latest` round-trip; newest-wins.
- `go_surface_serializes_whole_truth`: the serialized contract carries {n_trials, family_n_trials, effective_n, brier_pbo, brier_spa_p, clv_*, sharpe_dsr, verdict} — never a lone number.

**Files note:** the migration is `crates/fortuna-ledger/migrations/20260621000002_validation_runs.sql` (sorts strictly after the current tail `20260621000001_scorecards.sql`; M3).

**Acceptance / gate:** `SQLX_OFFLINE=… cargo test -p fortuna-backtest --test sweep` + `-p fortuna-ledger --test validation_runs -- --test-threads=1`; **regenerate `.sqlx` for the new `query!` against a LIVE Postgres** (per the ledger-gate-DB recipe — `fortuna_app` lacks CREATEDB; use the superuser socket; M5); clippy; commit.

---

### S6: `AeolusArchiveSource` (the only coupled code)

**Files:** Create `crates/fortuna-backtest/src/sources/aeolus_archive.rs` (+ `sources/mod.rs`); `tests/aeolus_archive.rs` (against a SMALL committed SQLite fixture, not the 17.8 GB live DB). Add `rusqlite` dep. Update the A7 decoupling guard to exclude this file.

**Interfaces — Consumes** S1 trait. **Produces:** `struct AeolusArchiveSource { aeolus_db, kalshi_db, range }` impl `HistoricalSource`, streaming.

**Algorithm + the load-bearing trap (spec §9):**
- `bracket_probability_log` → `HistoricalBelief` (binary), `available_at` = **forecast-issuance** instant (verify against the real schema it exists + isn't backfilled/target).
- Scalar belief = the **predicted distribution at issuance**; scores/PIT/CRPS in `scorecards` are **post-resolution → outcome-side only** (`available_at` = resolution time) and are **NOT imported as beliefs** — FORTUNA recomputes them (this preserves G-PARITY).
- `market_resolutions` → `HistoricalOutcome` (incl. voided); `snapshot_quotes` → `HistoricalSnapshot`; `shadow_intents` → `HistoricalTrade` (orders=0). `universe_manifest` = the full engaged market set incl. voided.
- Canonical `event_linkage`: station-code normalization + bracket-threshold parsing per the real schema (firmed here).

**Failing tests (TDD):**
- `aeolus_belief_available_at_is_issuance` (the trap): assert a mapped belief's `available_at` == issuance, and that NO score/PIT field flows into a belief payload. Mutation: mapping `available_at` to target_date → a G-PIT test (S2) reds when this source feeds the harness.
- `aeolus_manifest_includes_voided`; `aeolus_streams_without_full_load` (iterator, bounded memory); `aeolus_trade_orders_zero`.

**Acceptance / gate:** `cargo test -p fortuna-backtest --test aeolus_archive`; the A7 guard test still passes (adapter excluded); clippy; commit.

---

### S7: CLI — `fortuna backtest` + `fortuna validate`

**Files:** Modify `crates/fortuna-cli/src/main.rs` (+ a `backtest`/`validate` subcommand module). Test `crates/fortuna-cli/tests/backtest_cli.rs`. **M4: dispatch via the DB-backed async path (`main.rs:1043`, where the tokio runtime + pool are built), NOT the read-only block (`main.rs:138`)** — both commands write the ledger.

**Interfaces:** `fortuna backtest <source> [--from --to]` (idempotent replay via `ReplayHarness`); `fortuna validate --scope … --producer …` (run the sweep → write `validation_runs` → print the GO surface). Paper-safe: read-only on the source, no real orders.

**Failing tests (TDD):**
- `backtest_cli_idempotent`: two invocations → second writes 0 new rows.
- `validate_cli_emits_verdict`: prints the whole-truth surface incl. the verdict.
- `cli_is_read_only_on_source` (no writes to the source DB).

**Acceptance / gate:** `cargo test -p fortuna-cli --test backtest_cli`; clippy; commit.

---

## Boundary (after S1–S7) — the four integrity gates + DST + full battery

- **Integrity-gate executable tests** (each ships a mutation that reds it): G-PIT (S2 `gpit_*`), G-DEAD (S3 `gdead_*`), G-PARITY (S2 `parity_seam_*`), G-TRUTH (S4 `purged_cscv_bites_*` + S5 `verdict_*`).
- **DST scenarios** (add to the DST corpus): `backtest_rerun_idempotent`, `backtest_partial_replay_recovery` (crash mid-stream → resume → no dupes/gaps), `backtest_clock_determinism`.
- **Full battery:** `cargo test --workspace` (per-crate, handling DST harness + DB `--test-threads=1` per the battery-ops recipe) + `clippy --workspace --all-targets -- -D warnings` + `fmt --check` + the decoupling/source-literal greps + DST corpus.
- **Live smoke (optional, if a small real Aeolus slice is available):** `fortuna backtest aeolus-archive --from … --to …` on a bounded date range → assert a non-empty, source-stamped, idempotent replay + a `validate` GO surface.

## Self-review (done)
- **Spec coverage:** S1↔§3/§4; S2↔§5 G-PIT/G-PARITY + §6; S3↔§5 G-DEAD; S4↔§5 G-TRUTH + §7 + RESEARCH; S5↔§7; S6↔§9; S7↔§10; Boundary↔§11/§12. All spec sections mapped.
- **Type consistency:** `event_linkage: String`, `Verdict` enum, `HistoricalBelief`/`BeliefPayload`, `ValidationRun` used consistently across S1→S7.
- **No placeholders:** formulas are referenced to the research report (exact); test cases are concrete; the one deliberate deferral (exact Aeolus schema column names) is firmed in S6 against the real schema, which is the correct place.

## Deferrals
- Shared cross-language Source/Record *standard* with Alexandria — deferred until a second producer (spec D2).
- Arrow/Parquet record format — JSONL is the canonical contract now; Arrow is an optional scale path.
- Lo (2002) serially-correlated Sharpe SE — the effective-N haircut is used instead (research §8).
- Romano–Wolf StepM / stepwise-SPA family-wise control across the scope × config grid — deferred (research §6); the joint `family_n_trials` N-counting (S5) is the **non-deferrable** part that prevents cross-scope snooping now.
