# WS2 — Proof Layer Implementation Plan (v2 — modular crate + research-backed edge)

> **For agentic workers:** Executed via the **full Hephaestus workflow** (`hephaestus` skill:
> Conductor + hp-implementer/hp-verifier/hp-guardian; slice→phase→final gate cascade; fail-closed
> completion gate; worktree). Each Task = one slice (builder → independent verifier). Steps use `- [ ]`.
> Grounded in `docs/superpowers/specs/2026-06-20-ws2-proof-layer-design.md` +
> `docs/research/2026-06-20-ws2-scoring-grounding.md`.

**Goal:** A reliable persona finding path + a *pure, decoupled* `fortuna-scoring` library carrying the
research-backed metric suite (Brier/Log/RPS/CRPS + CORP + PIT + Murphy-diagram + Diebold–Mariano) +
a source-agnostic Scorecard with an honest GO/NO-GO surface served read-only.

**Architecture:** Extract the scoring math into a **pure `fortuna-scoring` crate** (deps: serde +
thiserror + std only) so "not coupled on the math" is compiler-enforced and WS3 reuses it without
cognition. One concept per module. The GO *decision* stays Brier-beats-baseline; WS2 makes the
*surface* tell the whole truth (CORP MCB−DSC+UNC + reliability bands + DM significance + N).

**Tech Stack:** Rust 2021; new pure crate `fortuna-scoring`; `sqlx`/Postgres (scorecard snapshot);
`axum` (rota endpoint); `serde`.

## Global Constraints (verbatim from spec + constitution)

- **Brier stays the sole GO gate.** RPS/Log/CORP/PIT/DM are recorded + surfaced; never change the gate decision.
- **I5 append-only:** no per-belief Log/RPS persistence in WS2 (binary beliefs FK→`scalar_beliefs`; computing them as pure scorecard aggregates avoids the FK wall). Scorecard snapshots are recomputed append-only rows, never edited.
- **I6 propose-only:** model emits a finding; the harness scores + sizes. No model authority added.
- **I7 promotion is operator-only:** scorecard + verdict RECOMMEND; the Murphy-diagram model-vs-model is the shadow-comparison evidence, not an auto-promote.
- **A7 / decoupling:** `fortuna-scoring` depends on NOTHING but std+serde+thiserror; `ScoringRule` dispatch keyed on `PredictiveKind`; scope/producer/source are strings handled only in the aggregation/scorecard layer, never in a metric.
- **No `panic!`/`unwrap`/`expect`** in any scoring function; probabilities `f64` in scoring only; `Clock`-injected time at the IO edges only; `cargo fmt` + `clippy -D warnings` clean.
- **CORP MCB used as a gate threshold is cross-fit (out-of-sample).** ε for Log = `1e-15`, fixed + documented, never tuned.

## Hephaestus execution parameters

- **North-star:** WS2 spec + milestone spec `2026-06-19-...` + `CLAUDE.md` (I1–I7) + the Alexandria WS3 brief (source-agnostic, G-TRUTH) + the research report.
- **Autonomy:** `plan-gated` — drive slice gates; at the WS2 phase boundary run guardian + live smoke, then surface for operator review (do NOT proceed to WS3).
- **Offline gate commands** (`.hephaestus/ws2.gates`):
  - `cargo test -p fortuna-scoring 2>&1 | grep -q "test result: ok"`
  - `cargo test -p fortuna-cognition --test scoring --test persona_runner --test persona_scoring 2>&1 | grep -q "test result: ok"`
  - `cargo test -p fortuna-invariants 2>&1 | grep -q "test result: ok"`
  - `cargo clippy -p fortuna-scoring -p fortuna-cognition -p fortuna-ledger -p fortuna-ops --all-targets -- -D warnings`
  - `cargo fmt --check`
  - decoupling grep (mutation-proof first): `! grep -rnE "sqlx|tokio|reqwest|fortuna_ledger|fortuna_cognition|PgPool|Clock" crates/fortuna-scoring/src/` (the crate stays pure)
  - source-literal grep: `! grep -rnE '"historical-import"|"live"|"aeolus"|"meteorologist"' crates/fortuna-scoring/src/` (no producer/source literal in the math)
  - persona path retired: `! grep -rn "mind.decide(&ctx)" crates/fortuna-cognition/src/persona_runner.rs`
- **DB-backed gates** (`SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp`): `cargo test -p fortuna-ledger --test ledger`; `-p fortuna-live --test daemon_smoke` at the phase boundary.
- **Live gate** (`.hephaestus/ws2.live.gates`, phase boundary; real Anthropic + Aeolus spend): `set -a; . ./.env; set +a; FORTUNA_LIVE_PERSONA_SMOKE=1 cargo test -p fortuna-cognition --test persona_live_smoke -- --nocapture` — loop-valid finding on **3 consecutive** runs.
- **Live-systems:** Anthropic (persona) + Aeolus KNYC feed (read-only); creds env-only.

## Deferral table

| Deferred | Why | Lands |
|---|---|---|
| Weather-ladder RPS (binary-per-threshold reconstruction) | weather proven by Brier/Log/CRPS/PIT/CORP; YAGNI | later, if a surface needs it |
| Per-belief Log/RPS persistence | `belief_scores.belief_id` FK→`scalar_beliefs`; binary beliefs live in `beliefs` → FK-fail. Computed as pure scorecard aggregates instead | later, schema reshape |
| Deflated Sharpe Ratio, PBO/CSCV | need trial count N / T×N config matrix — backtest/selection-only (research-confirmed) | WS3 |
| Cost-loss / economic-value envelope | needs the execution-cost model | WS3+ |
| Recalibration as a pre-trade transform | diagnostic now (CORP gives it); transform once sizing exists | later |
| G7 `k_unc`, G8 Edge Decay Watchdog | separate milestones | later |

---

## V&V-2 fixes (applied 2026-06-20 — SUPERSEDE the task text where they conflict)

A second independent V&V ran on this v2 plan. Binding fixes:

- **[Critical · S0] Use a re-export SHIM — do NOT repoint consumers.** `crate::scoring`/`fortuna_cognition::scoring` has 12+ consumers beyond the originally-listed set: fortuna-live (`daemon.rs:4294/4466/4727`), fortuna-runner (`funding_forecast.rs:62`), `cognition/aeolus_reliability.rs:28`, inline `crate::scoring::CategoricalBin` (`funding_baselines.rs:978/982`), and tests in live/runner/ledger (`daemon_smoke.rs`, `weather_resolve.rs`, runner funding tests, `aeolus_e2e.rs`). (`persona_beliefs.rs` imports NO scoring — phantom entry, drop it.) So S0 = create `fortuna-scoring`, move `scoring.rs`→`rules.rs`, **delete `cognition/src/scoring.rs`, and in `cognition/lib.rs` replace `pub mod scoring;` with `pub use fortuna_scoring as scoring;`** (the shim). This preserves EVERY `fortuna_cognition::scoring::*` path workspace-wide — **no other crate or file is repointed, no live/runner Cargo edits**. `fortuna-scoring/lib.rs` re-exports `pub use rules::*; pub use samples::*;` so both `fortuna_scoring::X` and the shimmed `fortuna_cognition::scoring::X` resolve.
- **[M2 · S0] dev-dep:** add `[dev-dependencies] proptest = { workspace = true }` to `fortuna-scoring/Cargo.toml` (the moved `tests/scoring.rs` uses it; dev-deps don't breach the `src/` purity gate).
- **[S0 · CRPS] Fix to TRUE CRPS + update test values.** Current `CrpsPinballRule` is `sum/len` = mean pinball = ½·CRPS (no factor-2, no Δτ — verified scoring.rs:360-365). Fix to `CRPS = 2·Σ pinball·Δτ` (equal grid → `(2/K)·Σ`); UPDATE the hand-computed CRPS expected values in `tests/scoring.rs`. CRPS is used only RELATIVELY (`forecast_crps < carry_forward_crps`, funding_baselines.rs:64) so **no GO decision changes** — the value change is EXPECTED, not a regression. Reword S0's "no behavior change" acceptance accordingly (it's a move + this one intentional scale fix; DBs hold no CRPS history yet).
- **[Important · S3] CORP bands = asymptotic/closed-form ONLY (no RNG).** The pure crate carries no `rand` and the constitution forbids non-injected randomness — drop the bootstrap option. Compute consistency bands via the asymptotic (closed-form) method; the band test is deterministic.
- **[Important · S5] Fix the DM test contradiction.** `loss_a==loss_b` → zero variance → the guard returns `None` (NOT stat≈0). So: `dm_identical_returns_none`; ADD `dm_near_zero_noisy_small_stat` (a tiny NOISY differential → stat≈0, p≈1) for the stat≈0 path; `dm_clear_winner_significant` MUST use a differential with genuine variance (A beats B on average + noise) or it hits the zero-variance guard. The DM formula `mean(d)/sqrt(HAC_var/n)` (Newey–West, two-sided normal) is correct.
- **[M4 · S5] Concrete Murphy elementary score.** Binary, threshold θ∈(0,1): `S_θ(p,o) = θ·(1−o)·𝟙{p>θ} + (1−θ)·o·𝟙{p≤θ}` (cost-loss for a user with cost/loss ratio θ who acts when p>θ). Murphy curve = mean `S_θ` over samples swept over a θ-grid; `dominates` = one curve ≤ the other at every grid point.
- **[M1 · S1] `MindOutput::empty()`** not `::default()` (no `Default` impl — mind.rs:141). The `StructuredStubMind.decide()` returns `MindOutput::empty()` → journal `None` → reverting the rewire is RED (bite holds).
- **[advisory · S6] Migration timestamp** — date the `scorecards` migration `2026-06-20`+ so it sorts strictly AFTER `20260619000001_price_snapshots_market_at_unique.sql`.
- **[advisory · S6] Label MCB as diagnostic** — the scorecard `go.reasoning` should tag the in-sample MCB explicitly as "diagnostic (cross-fit deferred to gating)" so no reader mistakes the in-sample MCB for a gated number (belt-and-suspenders on G-TRUTH; MCB is NOT a gate input in WS2).

The V&V re-confirmed as CORRECT: the CORP identity (`MCB−DSC+UNC = mean Brier`, numerically exact), the S2 RPS=0.13 + the pure-no-persistence FK reasoning, the strict-`<` GO match to `review.rs:278`, the `/api/rota/v1/scorecard` route + PATHS approach, and all S1 mind.rs references.

---

## File Structure

- **CREATE `crates/fortuna-scoring/`** (pure): `Cargo.toml` (serde, thiserror); `src/lib.rs`; `src/rules.rs` (trait + types + Brier/CRPS, then Log/RPS); `src/samples.rs`; `src/pav.rs`; `src/corp.rs`; `src/pit.rs`; `src/murphy_diagram.rs`; `src/dm.rs`; `src/scorecard.rs`. Tests under `crates/fortuna-scoring/tests/`.
- **MODIFY `crates/fortuna-cognition/`**: add `fortuna-scoring` dep; delete `src/scoring.rs` (moved); repoint imports `crate::scoring::` → `fortuna_scoring::` in `aeolus_beliefs.rs`, `scalar_beliefs.rs`, `funding_baselines.rs`, `persona_beliefs.rs`, `tests/scoring.rs`; add `src/scorecard_agg.rs` (gathers per-scope samples from the ledger, calls `fortuna_scoring::assemble_scorecard`).
- **MODIFY `crates/fortuna-ledger/`**: add `fortuna-scoring` dep; migration `..._scorecards.sql` (append-only snapshot); `insert_scorecard`/`latest_scorecard` repo.
- **MODIFY `crates/fortuna-ops/`**: add `fortuna-scoring` dep (light, pure); `GET /api/rota/v1/scorecard` route in `rota.rs`; extend `tests/rota.rs` `PATHS`.
- **MODIFY `crates/fortuna-invariants/`**: repoint any `crate::scoring`/`fortuna_cognition::scoring` references to `fortuna_scoring` (ADD-only; never weaken).

---

## Task 0 (S0): Extract the pure `fortuna-scoring` crate

**Files:** Create `crates/fortuna-scoring/{Cargo.toml,src/lib.rs,src/rules.rs,src/samples.rs}`; move `crates/fortuna-cognition/src/scoring.rs` content → `rules.rs`; modify the cognition consumers + `tests/scoring.rs` + invariants imports; root `Cargo.toml` workspace members.

**Interfaces:**
- Produces: crate `fortuna-scoring` re-exporting `ScoringRule, PredictiveDistribution, RealizedOutcome, PredictiveKind, CategoricalBin, Quantile, ScoreError, BrierRule, CrpsPinballRule`; new `samples::{CalibrationSample{p:f64,outcome:bool}, ScalarSample{quantiles:Vec<Quantile>, realized:f64}}`.
- Consumes: nothing but `serde`, `thiserror`, `std`.

- [ ] **Step 1: Scaffold the crate** — `Cargo.toml` (`[dependencies] serde {workspace, features=["derive"]}; thiserror {workspace}`), add to root workspace `members`. `lib.rs`: `pub mod rules; pub mod samples;` + `pub use rules::*;`.
- [ ] **Step 2: Move scoring.rs → rules.rs verbatim**, then add `samples.rs` with the two structs (derive `Debug,Clone,PartialEq,Serialize,Deserialize`).
- [ ] **Step 3: Repoint consumers** — in cognition `Cargo.toml` add `fortuna-scoring = { path = "../fortuna-scoring" }`; delete `cognition/src/scoring.rs`; replace `use crate::scoring::` → `use fortuna_scoring::` in `aeolus_beliefs.rs`, `scalar_beliefs.rs`, `funding_baselines.rs`, `persona_beliefs.rs`; move `cognition/tests/scoring.rs` → `fortuna-scoring/tests/scoring.rs` (imports `fortuna_scoring::`). Repoint any `fortuna-invariants` references.
- [ ] **Step 4: CRPS audit (research item)** — confirm `CrpsPinballRule` computes `CRPS = 2·Σ pinball·Δτ` (factor-2 + Δτ); if it omits the factor-2 or Δτ weighting, FIX it and add a known-value test (`CRPS` of a 2-quantile forecast vs a hand-computed value); if already correct, add a doc comment stating the convention. (Per research: "mean pinball" without ×2 is ½·CRPS.)
- [ ] **Step 5: Build + test** — `cargo build --workspace`; `cargo test -p fortuna-scoring`; `cargo test -p fortuna-cognition --test scoring`(if any remain) + the cognition suite that uses the types; `cargo test -p fortuna-invariants`. All green (behavior unchanged — this is a move).
- [ ] **Step 6: Commit** `refactor(scoring): extract pure fortuna-scoring crate (decoupled math)`.

**Acceptance (verifier):** workspace builds; ALL moved scoring tests pass identically; the decoupling grep (`fortuna-scoring/src` has no sqlx/tokio/reqwest/ledger/cognition/PgPool/Clock) passes; cognition/invariants consumers compile; the CRPS factor-2/Δτ is verified or fixed-with-test. No behavior change (it's a move) — confirm via the unchanged test outputs.

---

## Task 1 (S1): Persona structured-output (reliability fix)

**Files:** Modify `crates/fortuna-cognition/src/persona_runner.rs` (`run_persona_analysis`); Test `crates/fortuna-cognition/tests/persona_runner.rs`. *(Independent of S0; may run in parallel.)*

**Interfaces:** Consumes `Mind::decide_structured(&self, ctx, schema: serde_json::Value) -> Result<StructuredDecision, MindError>` (mind.rs:172, exists; `AnthropicMind` overrides with schema-constrained output; `StubMind` default re-parses journal.body). `StructuredDecision{value, cost_cents}`. `PersonaDef.schema`.

- [ ] **Step 1: Offline bite via a `StructuredStubMind`** — in tests/persona_runner.rs define a test-local mind that OVERRIDES `decide_structured` to return a scripted `StructuredDecision{value, cost_cents}` and whose `decide()` returns an empty/error `MindOutput`. Assert `run_persona_analysis` consumes `decision.value` (findings) + `decision.cost_cents` from THAT channel. Use the helpers that exist in this file (`persona()`, `findings_output()`, `signal()`, `t()`) — NOT `meteorologist()`/`scripted_findings()`/`ctx_item()` (those are in other test files).
```rust
struct StructuredStubMind { value: serde_json::Value, cost: i64 }
#[async_trait::async_trait] impl Mind for StructuredStubMind {
    fn id(&self)->&str{"structured-stub"}
    async fn decide(&self,_:&AssembledContext)->Result<MindOutput,MindError>{ Ok(MindOutput::default()) } // empty: journal None
    async fn decide_structured(&self,_:&AssembledContext,_:serde_json::Value)->Result<StructuredDecision,MindError>{
        Ok(StructuredDecision{ value: self.value.clone(), cost_cents: self.cost }) }
}
// test: run_persona_analysis with this mind yields findings==Some(value) + cost_cents==self.cost.
// Reverting the impl to `mind.decide(&ctx)` makes it FAIL (decide() journal is None → no findings).
```
- [ ] **Step 2: Run — expect FAIL** (current code uses `mind.decide`+journal.body → with StructuredStubMind's empty `decide()`, no findings). `cargo test -p fortuna-cognition --test persona_runner persona_structured -v`.
- [ ] **Step 3: Implement** — replace `mind.decide(&ctx)` + the `output.journal`/`journal.body` extraction with `mind.decide_structured(&ctx, persona.schema.clone())` → `StructuredDecision{value,cost_cents}`; `let findings = decision.value;` (no journal indirection); keep `validate_findings(&findings,&persona.schema)` (defense-in-depth) + the `content_hash` stamping; `budget.record_spend(decision.cost_cents, now)`.
- [ ] **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** `feat(persona): emit findings via schema-enforced decide_structured`.

**Acceptance:** the StructuredStubMind offline test bites (revert → RED); existing persona_runner tests green; **boundary live-smoke = 3 consecutive loop-valid findings** (the true integration bite — no journal-based StubMind test can distinguish the rewire; state this).

---

## Task 2 (S2): Log + RPS `ScoringRule`s (in fortuna-scoring)

**Files:** Modify `crates/fortuna-scoring/src/rules.rs`; Test `crates/fortuna-scoring/tests/scoring.rs`.

**Interfaces:** Produces `pub struct LogScoreRule;` (`id="log"`, Binary) + `pub struct RpsRule;` (`id="rps"`, Categorical). Pure; additive trait impls; **no persistence**.

- [ ] **Step 1: Failing tests** — Log: `log_score_at_half_is_ln2` (`−ln0.5`), `log_floor_keeps_finite` (p≈1, miss → finite, >30, not +∞), `log_rejects_scalar` (`UnsupportedKind`). RPS: `rps_known_value_three_bins` (Categorical masses [0.2,0.5,0.3], outcome=mid → CDFs [0.2,0.7,1.0] vs [0,1,1] → 0.04+0.09 = **0.13**), `rps_rejects_binary` (Binary pred + Binary outcome → `UnsupportedKind`), `rps_unknown_label_invalid` (outcome label absent → `InvalidPrediction`).
- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement** — both mirror `BrierRule`'s guard ORDER (`pred.validate()?` → `applies_to` (`UnsupportedKind`) → kind-parity (`KindMismatch`)). Log: `p.clamp(1e-15,1.0-1e-15)`, `if happened {-p.ln()} else {-(1.0-p).ln()}`. RPS: cumulative sums of `bins[i].p` (the Vec order is the ladder order — document it) vs the step-at-realized-label CDF; `Σ_{i=1}^{K-1}(P_i−O_i)²`; absent label → `InvalidPrediction`. No unwrap/panic.
- [ ] **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** `feat(scoring): LogScoreRule + RpsRule (additive, pure)`.

**Acceptance:** the 6 tests; monotonicity (worse → higher); guard order matches BrierRule; **no `belief_scores` INSERT** (pure functions only — the FK wall; aggregation consumes them in S6). The `p<ε` tail-event COUNT is surfaced at the scorecard (S6), not here.

---

## Task 3 (S3): PAV + CORP — reliability + MCB−DSC+UNC (in fortuna-scoring)

**Files:** Create `crates/fortuna-scoring/src/pav.rs`, `src/corp.rs`; `lib.rs` mods; Test `tests/corp.rs`. *(Replaces the binned Murphy — research-mandated.)*

**Interfaces:** Produces
```rust
// pav.rs
pub fn pav(values: &[f64], weights: &[f64]) -> Vec<f64>;   // isotonic (nondecreasing) fit, O(n log n), no panic
// corp.rs
pub struct ReliabilityPoint { pub p: f64, pub recalibrated: f64, pub count: usize }     // Serialize
pub struct Corp { pub mcb: f64, pub dsc: f64, pub unc: f64,
                  pub curve: Vec<ReliabilityPoint>, pub band_lo: Vec<f64>, pub band_hi: Vec<f64> } // Serialize
/// CORP decomposition of the Brier score: S̄ = MCB − DSC + UNC (all ≥ 0). `samples`=(p,outcome).
pub fn corp(samples: &[samples::CalibrationSample]) -> Option<Corp>;
```

- [ ] **Step 1: Failing tests** — PAV: `pav_is_monotone` (output nondecreasing; on already-sorted input = input). CORP **decomposition identity** (load-bearing): `corp_decomposition_equals_mean_brier` — `mcb − dsc + unc ≈ mean Brier` (within 1e-9) on a seeded set; `corp_terms_nonnegative` (mcb,dsc,unc ≥ 0); `corp_calibrated_set_has_near_zero_mcb` (a perfectly-calibrated synthetic set → mcb≈0); `corp_empty_is_none`.
```rust
#[test] fn corp_decomposition_equals_mean_brier() {
    let s: Vec<_> = [(0.1,false),(0.2,false),(0.8,true),(0.9,true),(0.5,true),(0.5,false),(0.3,true),(0.7,false)]
        .iter().map(|(p,o)| CalibrationSample{p:*p,outcome:*o}).collect();
    let c = corp(&s).unwrap();
    let brier = s.iter().map(|x|{let o=if x.outcome{1.0}else{0.0}; (x.p-o)*(x.p-o)}).sum::<f64>()/s.len() as f64;
    assert!((c.mcb - c.dsc + c.unc - brier).abs() < 1e-9);
    assert!(c.mcb>=0.0 && c.dsc>=0.0 && c.unc>=0.0);
}
```
- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement** — PAV (pool-adjacent-violators, weighted). CORP: sort by `p`; `recalibrated = pav(outcomes ordered by p)` (the isotonic conditional-event-rate); `UNC = ō(1−ō)`; `DSC = mean Brier(climatology ō) − mean Brier(recalibrated)`; `MCB = mean Brier(raw p) − mean Brier(recalibrated)`; so `S̄ = MCB − DSC + UNC` holds by construction. `curve` = distinct (p, recalibrated, count). Bands: bootstrap resample (a fixed-seed/deterministic resample — pass a seed in; NO wall-clock RNG) or asymptotic; if bootstrap, document determinism. No unwrap; N=0 → None.
- [ ] **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** `feat(scoring): PAV + CORP reliability and MCB-DSC-UNC decomposition`.

**Acceptance:** the decomposition identity holds on ≥2 seeded sets; nonnegativity; calibrated→mcb≈0; bands deterministic (seeded) — no wall-clock RNG; N=0→None; `Serialize` round-trips. **MCB used as a gate threshold is cross-fit** — document that the scorecard (S6) computes the gated MCB on a held-out split; the in-sample CORP curve is a diagnostic.

---

## Task 4 (S4): PIT histogram (in fortuna-scoring)

**Files:** Create `crates/fortuna-scoring/src/pit.rs`; `lib.rs` mod; Test `tests/pit.rs`.

**Interfaces:** `pub struct PitBin{lo:f64,hi:f64,count:usize}`(Serialize); `pub fn pit_value(quantiles:&[Quantile], realized:f64)->Option<f64>`; `pub fn pit_histogram(us:&[f64], k_bins:usize)->Vec<PitBin>`.

- [ ] **Step 1: Failing tests** — `pit_at_median_is_half` (ladder (0.25,9)(0.5,10)(0.75,11), realized=10 → 0.5); `pit_histogram_bins_sum_to_n`; `pit_below_lowest_quantile_is_zero` / `above_highest_is_one`.
- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement** — `pit_value`: linear-interpolate the q at v=realized over the `(q,v)` ladder (clamp [0,1]; below lowest v → 0, above highest → 1). Same CDF-at-realized definition Aeolus documents for `scorecards.pit` (doc comment). `pit_histogram`: equal-width [0,1] bins. No unwrap; empty ladder → None. **Doc the discrete-producer caveat** (randomized PIT needed for discrete predictive distributions; not applicable to the continuous Aeolus envelope).
- [ ] **Step 4: Run — PASS.** **Step 5: Commit** `feat(scoring): PIT value + histogram (continuous scalar)`.

**Acceptance:** median→0.5; monotone interpolation; bins sum to N; empty→None; the discrete caveat is documented.

---

## Task 5 (S5): Murphy diagram + Diebold–Mariano (in fortuna-scoring)

**Files:** Create `crates/fortuna-scoring/src/murphy_diagram.rs`, `src/dm.rs`; `lib.rs` mods; Tests `tests/dominance.rs`.

**Interfaces:**
```rust
// murphy_diagram.rs — binary elementary score S_θ(p,o) = (o - 1{p>θ})·... ; mean over samples, swept over θ∈[0,1]
pub struct MurphyPoint{ theta:f64, score_a:f64, score_b:f64 }  // Serialize
pub fn murphy_curve(a:&[CalibrationSample], b:&[CalibrationSample], grid:usize) -> Vec<MurphyPoint>;
pub fn dominates(curve:&[MurphyPoint]) -> Option<&'static str>; // Some("a")/Some("b") if one ≤ other ∀θ, else None
// dm.rs — Diebold–Mariano on a loss differential, HAC (Newey–West) std error
pub struct DmResult{ stat:f64, p_value:f64, n:usize }          // Serialize
pub fn diebold_mariano(loss_a:&[f64], loss_b:&[f64], hac_lag:usize) -> Option<DmResult>;
```

- [ ] **Step 1: Failing tests** — `murphy_strict_dominance` (a forecast uniformly closer to truth dominates for all θ → `dominates`→Some(better)); `murphy_crossing_is_none` (curves cross → None). DM: `dm_identical_forecasts_stat_zero` (loss_a==loss_b → stat≈0, p≈1); `dm_clear_winner_significant` (loss_a ≪ loss_b → |stat| large, p<0.05); `dm_too_short_is_none` (n<~8 → None).
- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement** — Murphy: elementary scoring function for the mean functional swept over θ; mean per θ; `dominates` checks one curve ≤ other at every grid point. DM: `d=loss_a−loss_b`; `stat = mean(d)/sqrt(hac_var(d, hac_lag)/n)` with Newey–West HAC variance; two-sided normal p-value; guard n small / zero variance → None. No unwrap.
- [ ] **Step 4: Run — PASS.** **Step 5: Commit** `feat(scoring): Murphy-diagram dominance + Diebold-Mariano test`.

**Acceptance:** dominance detects strict + crossing cases; DM stat≈0 on identical, significant on a clear winner, None on too-short; HAC variance is non-negative; pure (no RNG/Clock).

---

## Task 6 (S6): Scorecard + GO whole-truth + read-only endpoint

**Files:** Create `crates/fortuna-scoring/src/scorecard.rs`; `crates/fortuna-cognition/src/scorecard_agg.rs`; migration `crates/fortuna-ledger/migrations/..._scorecards.sql` + repo; modify `crates/fortuna-ops/src/rota.rs` + `tests/rota.rs`. Tests `fortuna-scoring/tests/scorecard.rs`.

**Interfaces:**
```rust
// fortuna-scoring/src/scorecard.rs (pure)
#[derive(Serialize)] pub enum GoDecision { Go, NoGo, Insufficient } // snake_case
#[derive(Serialize)] pub struct GoSurface { pub decision: GoDecision, pub reasoning: String }
#[derive(Serialize)] pub struct Scorecard {
  pub scope:String, pub producer:Option<String>, pub window:String, pub n:u32,
  pub brier:f64, pub brier_baseline:f64, pub rps:Option<f64>, pub log_score:Option<f64>,
  pub log_tail_events:u32, pub crps:Option<f64>, pub clv_mean_bps:Option<f64>,
  pub corp:Option<Corp>, pub pit_bins:Vec<PitBin>,
  pub dm_vs_baseline:Option<DmResult>, pub go:GoSurface }
pub fn assemble_scorecard(scope:&str, producer:Option<&str>, window:&str,
  samples:&[CalibrationSample], baseline_brier:f64, baseline_losses:Option<&[f64]>,
  rps:Option<f64>, log_score:Option<f64>, log_tail_events:u32, crps:Option<f64>,
  clv:&[f64], pit_bins:Vec<PitBin>, min_n:u32) -> Scorecard;
```

- [ ] **Step 1: Failing tests (pure assembly)** — `scorecard_go_strict_lt` (brier `<` baseline + n≥min_n → `Go`); `scorecard_tie_is_nogo` (brier `==` baseline → `NoGo` — matches WS1 `review.rs:278` strict `<`); `scorecard_insufficient_below_min_n`; `scorecard_reasoning_whole_truth` — `card.go.reasoning` contains `&card.n.to_string()`, `"baseline"`, and `"MCB"` (G-TRUTH: N + baseline + CORP); `scorecard_rps_none_for_binary`; `scorecard_serialize_golden_shape` (exact contract keys); **`scorecard_parity_seam`** (same samples, `window="forward"` vs `"historical"` → identical except the `window` field).
- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement `assemble_scorecard`** — `brier`/`n` from samples; `corp = corp(samples)`; `dm_vs_baseline = baseline_losses.map(|b| diebold_mariano(per-sample brier losses, b, lag))`; `clv_mean_bps = mean(clv) or None`; **GO: `Insufficient` if n<min_n; else `Go` if brier < baseline_brier; else `NoGo`** (strict `<`); `reasoning` = whole-truth string naming brier vs baseline, N, log + tail count, CORP MCB/DSC/UNC, DM p-value, and "single forward window, no selection (PBO N/A — WS3)". No panic on empty.
- [ ] **Step 4: Run — PASS.**
- [ ] **Step 5: Aggregation + persistence** — `cognition/src/scorecard_agg.rs`: gather per-(scope,producer,window) `(p,outcome)` samples + clv + the precomputed rps/log/log_tail/crps/pit from the ledger, call `assemble_scorecard`. Ledger: `..._scorecards.sql` append-only snapshot (`UNIQUE(scope,producer,window,computed_at)`, UPDATE/DELETE trigger refuses) + `insert_scorecard`/`latest_scorecard`; `cargo sqlx prepare --workspace`; DB round-trip test.
- [ ] **Step 6: Endpoint** — `rota.rs`: `.route("/api/rota/v1/scorecard", get(view_scorecard))` (the ROTA data-route namespace); `view_scorecard(Query, State<RotaState>)` reads `latest_scorecard` → `Json<Scorecard>` (404 absent). **Add the path to the hardcoded `PATHS` array in `tests/rota.rs` and bump its length** (so the existing 405-on-mutation + 200 test covers it). `fortuna-ops` depends on the LIGHT `fortuna-scoring` for the `Scorecard` type (resolves the earlier cognition-edge finding).
- [ ] **Step 7: Commit** `feat(scorecard): source-agnostic Scorecard + GO whole-truth + /api/rota/v1/scorecard`.

**Acceptance:** the pure-assembly tests incl. **strict-`<` tie→NoGo** (GO matches WS1 on identical data) + the **parity seam** + the G-TRUTH reasoning assertion; endpoint 200/404 + 405-on-mutation (PATHS updated); migration append-only trigger refuses UPDATE; `sqlx prepare` committed.

---

## Self-Review

- **Spec coverage:** S0 crate (architecture §) ✓; S1 persona ✓; S2 Log/RPS ✓; S3 CORP (replaces Murphy, research §) ✓; S4 PIT ✓; S5 Murphy-diagram+DM (edge §) ✓; S6 Scorecard+GO+endpoint ✓. CLV `Option` honest until linkage closes. Deferrals (DSR/PBO→WS3) explicit. ✓
- **Type consistency:** `CalibrationSample`/`ScalarSample` (S0) consumed by S3/S5/S6; `Corp`/`PitBin`/`DmResult` (S3/S4/S5) consumed by S6's `Scorecard`; `ScoringRule`/`PredictiveDistribution` names match the moved rules.rs. ✓
- **Placeholders:** none — formulas + test values concrete; the one repo (`belief_scores`) is not used for Log/RPS (pure aggregates).
- **Decoupling:** every metric is a pure module in `fortuna-scoring`; the crate's gate greps forbid IO + source literals; ops depends on the light pure crate. ✓
- **Order:** S0 first (foundation); S1 independent; S2/S3/S4/S5 independent of each other (parallelizable); S6 depends on S2–S5. Phase = all seven; boundary = guardian + live smoke + operator review.
