# WS2 — Proof Layer: design spec

> Part of the **Loop-Close & Provable Demo** milestone
> (`docs/superpowers/specs/2026-06-19-loop-close-and-provable-demo-design.md`).
> WS1 (live spine) is COMPLETE. This spec elaborates the milestone's **WS2 — Proof
> layer** into an implementable design. The implementation **plan** is a separate
> doc (writing-plans, next step); this spec defines *what* and *why*, not the
> bite-by-bite code.

**Goal.** Make the loop's correctness **visible and provable**: richer scoring
metrics (RPS, Log, Murphy decomposition, PIT) layered additively on the existing
Brier/CRPS/CLV, an honest GO/NO-GO surface that tells the **whole truth**, and a
**source-agnostic scorecard contract** the UI session renders against —
recomputed on the daemon cadence as first-class capabilities (milestone D-E).

**Sequencing (load-bearing).** WS2 builds and proves this scoring on **seeded**
data. WS3 later replays **real March→June history** through the **exact same**
`resolve_and_score` / scorecard path. Therefore every WS2 surface MUST be
**source-agnostic** — it scores `provenance.source="historical-import"` rows
identically to live rows, selected only by a window/source filter. One scoring
path, never two. (Alexandria alignment, §Integrity.)

**North-star.** Milestone spec §WS2, §7 (worked example Part 2), §D-E, §G-TRUTH;
the constitution `CLAUDE.md` (I5/I6/I7); the Alexandria ↔ Fortuna WS3 brief
(2026-06-20) for the contract-shape + integrity framing.

---

## Scope

**In:** persona structured-output reliability fix; RPS + Log scoring rules;
Murphy decomposition + reliability diagram; PIT histogram; the `Scorecard`
contract + GO whole-truth surface + a read-only endpoint.

**Out (explicit):** the rich UI render (UI session, against our contract); WS3's
backtest harness + its G-PIT/G-DEAD/G-PARITY gates; PBO / deflated-Sharpe (needs
window/threshold selection, which only the backtest introduces); G7 (`k_unc`
sizing) and G8 (Edge Decay Watchdog) — separate later milestones.

---

## Global constraints (copied verbatim from the milestone + constitution)

- **Brier stays the sole GO gate.** RPS/Log/Murphy/PIT are recorded as data and
  surfaced; they never change the gate decision in WS2.
- **I5 append-only.** New scoring is additive: per-belief scores are new
  `belief_scores` rows keyed by `rule_id` (`UNIQUE(belief_id, rule_id)`,
  app-layer insert-once, the existing append-only trigger holds). No column is
  mutated in place. Aggregate scorecards are recomputed snapshots, never edited.
- **I6 propose-only.** The model emits a *finding*; the harness scores and sizes.
  WS2 adds no model authority.
- **I7 promotion is operator-only.** The scorecard + verdict **recommend**; they
  never self-promote (the existing `propose_promotion` contract holds).
- **A7 decoupling.** Producer / domain / venue are DATA. New scoring keeps the
  `ScoringRule` dispatch keyed on `PredictiveKind`; scope/producer are strings,
  no name literals in the scoring or scorecard core.
- Money is integer cents; probabilities are `f64` in cognition only; no
  `panic!`/`unwrap`/`expect` in the scoring path; `Clock`-injected time only.

---

## Architecture

```
beliefs (immutable) ──resolve──> outcomes
        │                            │
        ▼                            ▼
   ScoringRule fan-out (per belief, additive rule rows in belief_scores)
      Brier · CRPS · RPS(new) · Log(new)
        │
        ▼
   Scope aggregation (per scope+producer+window, on cadence)
      Murphy{REL,RES,UNC} · reliability_bins · PIT_bins · CLV · baseline · N
        │
        ▼
   Scorecard (Serialize, source-agnostic) ──> GET endpoint ──> UI session render
                                          └──> GO whole-truth surface
```

The same fan-out + aggregation runs for live rows and (WS3) for
`source="historical-import"` rows; the window/source filter selects the set.

---

## Architecture — the pure `fortuna-scoring` crate (modularity)

The scoring math is a **pure library crate** (`crates/fortuna-scoring`; deps: `serde` + `thiserror` +
`std` ONLY — no sqlx/tokio/reqwest/Clock/ledger/cognition). The crate boundary makes "not coupled on the
math" a **compile-time invariant** and lets WS3's backtest reuse the identical scoring without depending
on cognition. Layout — one concept per module:

- `rules.rs` — the `ScoringRule` trait + `PredictiveDistribution`/`RealizedOutcome`/`PredictiveKind`/
  `CategoricalBin`/`Quantile`/`ScoreError` + Brier · Log · RPS · CRPS (moved from `cognition/scoring.rs`).
- `samples.rs` — the lingua-franca inputs: `CalibrationSample { p, outcome }` (binary),
  `ScalarSample { quantiles, realized }`. The math depends ONLY on these, never on belief/ledger types.
- `pav.rs` — Pool-Adjacent-Violators isotonic primitive (SHARED by CORP + recalibration; DRY).
- `corp.rs` — CORP reliability curve + `MCB − DSC + UNC` decomposition (uses `pav`) + consistency bands.
- `pit.rs` — PIT value + histogram.
- `murphy_diagram.rs`, `dm.rs` — forecast-dominance (Ehm et al.) + Diebold–Mariano.
- `scorecard.rs` — the source-agnostic `Scorecard` struct + `assemble_scorecard` (pure composer).

Dependency direction (one-way, compiler-enforced):
`fortuna-scoring` ← `fortuna-cognition` (gathers samples from beliefs, calls scoring) ←
`fortuna-ledger`/`fortuna-live`/`fortuna-ops` (IO: persist, cadence, endpoint) ← `fortuna-backtest`
(WS3 — depends ONLY on `fortuna-scoring` for the math).

Staff-engineer principles: single-responsibility (one metric/file), dependency-inversion (math depends
on nothing but std+serde), open-closed (new metric = new module + one line in `assemble_scorecard`), DRY
(PAV shared), source-agnostic (operates on `Sample`s — WS3 replays identically), pure→testable (property
tests per module). The ops endpoint depends on the LIGHT pure crate for the `Scorecard` type (not the
whole cognition graph) — resolving the earlier V&V dependency-edge finding.

## Research-grounding revisions (2026-06-20 — SUPERSEDE the per-metric Slices text where they conflict)

Grounded in `docs/research/2026-06-20-ws2-scoring-grounding.md` (proper-scoring-rule + forecast-
verification literature, primary sources). Binding:

- **Murphy decomposition → CORP.** The binned `REL−RES+UNC` is a *biased* estimator at our n (~88;
  Ferro–Fricker 2012: severe below n≈300 — it overstates miscalibration). Replace it with the
  binning-free **CORP isotonic decomposition `S̄ = MCB − DSC + UNC`** (Dimitriadis–Gneiting–Jordan 2021,
  PNAS) + the stable CORP reliability curve + consistency bands. Keep `REL−RES+UNC=Brier` only as a
  synthetic unit-test identity. **MCB used as a gate threshold must be cross-fit** (out-of-sample) or it
  is optimistic. (Citation fix: binning bias is Ferro–Fricker 2012, NOT Bröcker 2009.)
- **Log score.** Keep the ε=1e-15 clamp (keeps it finite) but note it BREAKS strict propriety and ε
  moves scores — so ALSO surface a **count of `p_realized < ε` events** (the real overconfidence signal).
  ε is a fixed, documented constant, never tuned.
- **CRPS (existing).** During S0's move, verify `CrpsPinballRule` carries the **factor-of-2 + Δτ**
  weighting (`CRPS = 2·Σ pinball·Δτ`); else it is ½·CRPS. Document the convention.
- **PIT.** Plain PIT is correct for the continuous scalar producer (Aeolus envelope); record the caveat
  that **discrete** producers need the randomized (or Czado–Gneiting–Held non-randomized) PIT.
- **Add (the edge): Murphy diagrams + forecast dominance** (Ehm–Gneiting–Jordan–Krüger 2016) for the
  model-vs-model promotion check (I7 shadow comparison), and the **Diebold–Mariano test** (does the model
  beat the de-vigged baseline *significantly* on the forward window — single-window-valid, unlike PBO).
- **CLV** stays the parallel market-edge signal (validate the profit-claim on our own data; CLV ≠
  calibration). **Confirmed deferrals:** Deflated Sharpe + PBO/CSCV are backtest/selection-only (need a
  trial count N / T×N matrix) → WS3; cost-loss economic-value → later (needs the execution-cost model).

---

## Slices

Each slice runs the Hephaestus cascade (builder → independent verifier;
guardian + live-smoke at the WS boundary). Acceptance criteria are the
verifier's gate.

### S1 — Persona structured-output (reliability fix)

**Problem (live-gate finding, 2026-06-20):** the persona path forces the
synthesis `{beliefs,proposals,journal}` schema where `journal` is
`anyOf:[null,{body}]`; the charter can only *instruct* the model to populate
`journal.body`, so it intermittently returns `journal:null` → no finding
(`persona_runner.rs:325`). Fail-closed but unreliable.

**Design.** Give the persona path its **own enforced output schema** instead of
the nullable-journal indirection:
- Expose `decide_structured(&self, ctx, schema: serde_json::Value) -> Result<serde_json::Value, MindError>` on the `Mind` trait, wrapping the existing
  private `AnthropicMind::call_priced_structured` (`mind.rs:665`). `StubMind`
  implements it by returning its scripted `Value` (tests stay deterministic).
- `run_persona_analysis` calls `decide_structured` with the persona's
  `findings/v2` schema (`persona.schema`) as the **required** provider output.
  The provider's structured-output enforcement makes the findings object the
  required root — **no null-escape**.
- Slice A's recursive `validate_findings` stays as defense-in-depth (covers the
  `StubMind`/scripted path and guards against any provider drift).
- The journal.body route is retired **for personas** (discovery already uses
  `call_priced_structured` directly; unchanged).

**Interfaces.** `Mind::decide_structured` (new trait method, both impls);
`run_persona_analysis` signature unchanged (internal call swap).

**Acceptance.** Unit: `StubMind` structured path returns the scripted finding,
parsed + validated, artifact produced. Property: a scripted finding missing
`thresholds` is a counted defect (not a crash). **Live-smoke (boundary):** the
gated `persona_live_smoke` test produces a loop-valid finding on ≥3 consecutive
real runs (the reliability the fix exists to deliver). Mutation: revert the
schema-enforced call → `journal:null` failure reproduces.

### S2 — G5: RPS + Log as additive `impl ScoringRule`

**Data-model fact (verified).** FORTUNA bracket ladders (weather/Aeolus) are
modeled as **N independent Binary threshold-beliefs** — `map_persona_analysis` /
`map_aeolus_envelope` fan each `ge` threshold onto its own binary belief.
`PredictiveDistribution::Categorical` exists and is constructed only by **funding**
(`funding_baselines.rs`). So a per-belief RPS keyed on `Categorical` does **not**
fire on the weather demo scope.

**Design.** Two new rules behind the existing trait
(`fn id`, `fn applies_to(PredictiveKind)`, `fn score(dist, outcome) -> Result<f64, ScoreError>`):
- `LogScoreRule` — `id()="log"`, `applies_to(Binary)`. `−ln(p)` if happened else
  `−ln(1−p)`, `p` clamped to `[ε, 1−ε]` (`ε=1e-15`) so `p∈{0,1}` cannot yield
  `−∞`. Fires per-belief on **every** binary threshold-belief (weather + funding)
  — this is the additive metric the weather demo actually gains.
- `RpsRule` — `id()="rps"`, `applies_to(Categorical)`.
  `RPS = Σ_{k=1}^{K-1} (CDF_pred_k − CDF_obs_k)²` over the ordered ladder. Fires
  per-belief for **Categorical producers** (funding today); the milestone's
  "additive impl ScoringRule" form, recorded as an additive `belief_scores` row.

Both stored as additive `belief_scores` rows (`rule_id` = `"log"` / `"rps"`).
**Brier stays the GO gate**; these are data.

**Weather-ladder RPS is DEFERRED (YAGNI).** Computing RPS for a *binary-per-threshold*
weather ladder requires reconstructing the ordinal ladder per event (group the
event's binary threshold-beliefs, form the CDF). That is a scope-level aggregate,
not a per-belief score — and the weather scope is already proven by
Brier + Log + CRPS + PIT + Murphy + reliability (milestone §7 worked example shows
exactly those, no RPS). So `Scorecard.rps` is `None` for binary-threshold scopes
in WS2; the ladder reconstruction is a later add if a demo surface needs it.

**Acceptance.** Known-value property tests: hand-computed RPS for a 3-bin
**Categorical** distribution; Log at p=0.5 (`−ln0.5`) and p=1 hitting the ε-floor
(finite, not `−∞`). `applies_to` parity (RPS rejects Binary, Log rejects Scalar →
`UnsupportedKind`, no panic); monotonicity (worse forecast → higher score).
Additive-row test: scoring a binary belief writes a `log` row without touching its
`brier` row (I5).

### S3 — G3: Murphy decomposition + reliability diagram

**Design.** Over the resolved+calibrated **Binary** set per scope:
- Bin predicted `p` into K bins (default K=10). Per bin `k`: count `n_k`, mean
  predicted `p̄_k`, observed frequency `ō_k`. Global base rate `ō`.
- `REL = Σ_k (n_k/N)(p̄_k − ō_k)²` · `RES = Σ_k (n_k/N)(ō_k − ō)²`
  · `UNC = ō(1 − ō)`.
- **Property (the proof):** `REL − RES + UNC == mean Brier` over the same set
  (within ε) — the calibration-refinement identity. This is the load-bearing
  test that the decomposition is correct.
- `reliability_bins: Vec<ReliabilityBin { p_mean, obs_freq, count }>`, `Serialize`.

**Acceptance.** The `REL − RES + UNC == Brier` identity on a seeded set;
empty/degenerate sets (N=0, all-same-p) degrade to `None`, never NaN/panic;
`Serialize` round-trips.

### S4 — G6: PIT histogram

**Design.** For **Scalar** producers, the Probability Integral Transform:
`u_i = F_i(x_realized_i)` (the predictive CDF at the realized value), binned into
a histogram (`pit_bins: Vec<PitBin { lo, hi, count }>`). Uniform ⇒ calibrated.
Implement the **same CDF-at-realized PIT definition Aeolus documents** for its
`scorecards.pit` — there is **no Rust function to reuse** (`aeolus_reliability.rs`
is Brier/CRPS only); the cross-pollination is definitional, so live +
Aeolus-archive agree on the formula. `Serialize` bins.

**Acceptance.** A known Normal predictive at its own mean → `u=0.5`; a calibrated
synthetic set → approximately uniform bins (χ² within tolerance); reuse verified
against the Aeolus definition (same input → same `u`).

### S5 — Scorecard contract + GO whole-truth + endpoint

**The contract** (the UI boundary; source-agnostic, `Serialize`):
```rust
struct Scorecard {
    scope: String,            // "weather:KNYC:tmax"
    producer: Option<String>, // "aeolus" | "meteorologist" | None (merged)
    window: String,           // "forward" (live) | "historical" (WS3)
    n: u32,                   // resolved trial count in the window
    brier: f64,
    brier_baseline: f64,      // the de-vigged market baseline (same source as CLV)
    rps: Option<f64>,         // Categorical producers only; None for binary-threshold (weather) scopes in WS2
    log_score: Option<f64>,
    crps: Option<f64>,        // scalar producers
    clv_mean_bps: Option<f64>,
    murphy: Option<Murphy>,   // { rel, res, unc } over the binary set
    reliability_bins: Vec<ReliabilityBin>,
    pit_bins: Vec<PitBin>,    // scalar producers
    go: GoSurface,            // { decision, reasoning } — see G-TRUTH
}
enum GoDecision { Go, NoGo, Insufficient } // Insufficient when N < threshold
```
- Computed per `(scope, producer, window)` by aggregating the additive
  `belief_scores` rows + the resolved set, **recomputed on the daemon cadence**
  (D-E). The `window="forward"` filter excludes `source="historical-import"`
  (matching the WS1 go_nogo forward-only count); `window="historical"` is WS3's.
- **Served** as a new GET route on `rota_router` (`fortuna-ops/rota.rs`,
  GET-only, 405 on mutation per R3): `/rota/scorecard?scope=&producer=&window=`
  → `Json<Scorecard>`. Read-only; no money path.

**G-TRUTH (the whole truth).** `GoSurface.reasoning` and the scorecard report the
**multi-metric view**: Brier **and** its baseline, RPS, Log, CRPS, CLV, Murphy
REL/RES/UNC, the reliability diagram, **and the trial count N** — never one
flattering number. The GO **decision** stays the WS1 rule (Brier beats the
de-vigged baseline, forward-only, N≥ threshold) — WS2 only makes the *surface*
honest. PBO / deflated-Sharpe is **deferred to WS3** (it needs the
window/threshold selection the backtest introduces; noted in the `go.reasoning`
as "single forward window, no selection" so the absence is explicit, not hidden).

**Acceptance.** `Scorecard` `Serialize` round-trips + a golden-JSON shape test
(the UI contract is pinned); the endpoint returns 200 on a seeded scope and 405
on POST/PUT/DELETE; `Insufficient` when N below threshold; the GO decision
matches the WS1 gate on the same data (no behavior change to the gate).

---

## Data model

- **`belief_scores`** (exists): additive `rule_id` rows for `rps`, `log`
  (alongside `brier`, `crps_pinball`). No schema change — `UNIQUE(belief_id,
  rule_id)` + append-only trigger already enforce exactly-once-per-rule.
- **Scorecard storage:** a scope-keyed `scorecards` snapshot table (recomputed
  on cadence) **or** on-demand computation at the endpoint. **Decision:**
  persist a recomputed snapshot (D-E "first-class on cadence"), keyed
  `(scope, producer, window, computed_at)`, append-only (history of scorecards
  is itself an audit trail). Migration in `crates/fortuna-ledger/migrations/`.
- Murphy/PIT/reliability bins serialize as JSON columns on the scorecard
  snapshot (they are derived aggregates, not per-belief facts).

---

## Integrity (Alexandria alignment)

- **Source-agnostic by construction.** The `ScoringRule` fan-out, the scope
  aggregation, and the `Scorecard` carry no `source` literal in their logic; the
  window/source filter is a query parameter. This is the line that lets WS3 run
  real history through *this* code unchanged (one ingest/scoring layer, not two).
- **G-TRUTH is a WS2 hard surface** (above): the GO/NO-GO reports the whole
  multi-metric truth + N, not a lone number.
- **G-PIT / G-DEAD / G-PARITY are WS3 gates**, not WS2 — but WS2 enables them:
  the scoring is point-in-time-replayable (no wall-clock reads; `Clock`-injected)
  and source-stamp-blind, so WS3's as-of replay + keep-the-dead + backtest↔live
  parity plug into the same path. WS2 adds a parity seam test (the same seeded
  belief scored "as live" and "as historical-import" yields an identical
  `Scorecard` modulo the `window`/`source` label).

---

## V&V plan

- **Per slice:** Hephaestus builder → independent verifier; TDD; mutation-proof
  every new gate/metric (a planted error in the math must turn a test RED).
- **Metric correctness is property-based**, not example-only: the Murphy identity
  (`REL−RES+UNC==Brier`), RPS/Log known-values + monotonicity, PIT uniformity,
  reliability-bin invariants (Σ counts == N).
- **No-panic / determinism:** degenerate inputs (N=0, p∈{0,1}, empty ladder)
  degrade to `None`/`Insufficient`; `Clock`-injected time; identical inputs →
  identical `Scorecard`.
- **Boundary (WS2 exit):** full workspace + invariants + DST + clippy/fmt; the
  persona structured-output **live-smoke** (≥3 consecutive loop-valid findings);
  the parity seam test; guardian audit (drift / I5-I6-I7 / no faked green /
  G-TRUTH honesty).

---

## Assumptions / deferrals (this milestone's ledger)

- Scorecard snapshot table is append-only history (a scorecard is never edited;
  a new cadence writes a new row). If storage growth matters, retention is a
  later ops concern (not WS2).
- `meteorologist` CLV is still `None` until the CLV per-event linkage gap
  (loop-close-gaps) is closed; the `Scorecard.clv_mean_bps` is `Option` and
  renders honestly absent until then. Closing that linkage fits naturally in S5.
- **RPS scope (resolved in S2):** weather/Aeolus ladders are binary-per-threshold,
  so `RpsRule` (Categorical) fires only for Categorical producers (funding);
  weather-ladder RPS (a scope-level reconstruction) is deferred — the weather
  scope is proven by Brier/Log/CRPS/PIT/Murphy. `Scorecard.rps` is `None` for
  binary-threshold scopes in WS2.
