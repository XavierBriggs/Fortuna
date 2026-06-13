# Perp strategies (T5.B7): runtime seam, `prob_claims/v1` scalar claims + swappable scoring, and the perp_event_basis basis model

Status: OPERATOR-APPROVED design (brainstorming pass, 2026-06-13). Build authorized.
Supersedes the "design-only" status of the scalar half of `signal-contract.md` §2/§5
(this note authorizes the scalar build, "scalar with the first scalar consumer" =
funding_forecast). Spec 5.11/5.15 + Section 6/11 govern; an adopted detail that the
spec leaves open is recorded here and folded into a spec touch-up if it proves
load-bearing.

## 0. Why these three together, and what's foundational

The operator's framing: design the scalar claim type once, because it is foundational
across **perps** (funding forecasts), **weather** (Aeolus temperature quantiles), and
**personas** (track E) — not a perps-only concern. The three seams are designed in one
pass because **funding_forecast exercises all three** (it is the first scalar consumer,
it rides the perp-data seam, and it shares the underlying with perp_event_basis), but
the scalar-belief section (§1) is written CONSUMER-AGNOSTIC so weather and personas
adopt it unchanged.

The cross-cutting reach is deliberate and is the point: §1 lives in `fortuna-cognition`
(types + scoring) + `fortuna-ledger` (storage), outside the perp domain; §2/§3 add perp
strategies in `fortuna-runner` + a `PerpTick` on the `fortuna-core` bus. Ownership and
sequencing are in §5.

## 1. Beliefs as `(PredictiveDistribution, RealizedOutcome)` + a swappable `ScoringRule`

The load-bearing decision (operator, 2026-06-13): score scalar forecasts **natively with
CRPS now** (not by fanning out to binary thresholds), and make the **scoring layer
swappable** so any piece can be optimized independently. The shape that delivers both:

### 1.1 Separate the durable facts from the score

A belief stores two things that never change once written:

- **`PredictiveDistribution`** — the predictive claim, by outcome shape (the
  `prob_claims/v1` shapes):
  - `Binary { p: f64 }`
  - `Categorical { bins: Vec<(Label, f64)> }`
  - `Scalar { quantiles: Vec<Quantile>, unit: String }` where `Quantile { q: f64, v: f64 }`.
  Validation (strict, deny-unknown-fields per major version): `q` strictly increasing in
  (0,1); `v` non-decreasing (no quantile crossing); ≥2 quantiles for scalar; all finite;
  binary `p` and categorical bin probs in [0,1] (bins sum within tolerance).
- **`RealizedOutcome`** — the realized result when the belief resolves:
  - `Binary { happened: bool }`
  - `Categorical { label: Label }`
  - `Scalar { value: f64 }` (the realized funding rate, the realized temperature).

Both are recorded IMMUTABLY. Re-stating the I-discipline: these are cognition-domain
forecast quantities, **never money** — `f64` is correct here exactly as `Brier`/`p` are
`f64` today (CLAUDE.md: "probabilities are f64 in cognition only"). When a forecast later
informs a perp ORDER, the conversion to `PerpPrice`/`Cents` happens at the gate/exec
boundary with the established rounding-against-us discipline, not in the belief.

### 1.2 `ScoringRule`: the swappable layer

```rust
pub trait ScoringRule {
    fn id(&self) -> &'static str;                          // e.g. "brier", "crps_pinball"
    fn applies_to(&self, kind: PredictiveKind) -> bool;    // which outcome shapes it scores
    fn score(&self, pred: &PredictiveDistribution, outcome: &RealizedOutcome)
        -> Result<f64, ScoreError>;                        // LOWER IS BETTER
}
```

First rules shipped as instances:

- **`BrierRule`** (binary): `(p − 1{happened})²`.
- **`CrpsPinballRule`** (scalar): the mean pinball/quantile loss over the provided
  quantiles, which IS the discretized CRPS — no CDF reconstruction:
  `pinball_q(y, v) = q·(y − v)` if `y ≥ v` else `(1 − q)·(v − y)`;
  `score = mean_k pinball_{q_k}(y, v_k)`. A proper scoring rule; lower is better; the
  1-quantile degenerate case orders like Brier.

Adding a log-loss rule, a weighted-CRPS, a categorical Brier, etc. is a new `impl`, not a
schema change.

### 1.3 Scores are derived, re-computable, and rule-tagged

The ledger stores `(belief_id, rule_id) → score` rows over the immutable
`(PredictiveDistribution, RealizedOutcome)`. Consequences (the "optimize every piece"
payoff):

- **Swap** the production scorer per belief class by config (CRPS vs pinball vs log-score
  for scalar; Brier vs log-loss for binary) — history is untouched, because the score is
  just a function of the durable facts + a rule id.
- **Backtest** a scorer: re-run any `ScoringRule` over every resolved belief and compare,
  because the prediction and the realized outcome are the immutable facts. Several scorers
  run side by side, each its own `(belief_id, rule_id)` row.

### 1.4 Storage (fortuna-ledger)

A scalar-belief path PARALLEL to the existing binary `BeliefRow` (binary/categorical
beliefs are untouched in slice 1):

- `scalar_beliefs`: `belief_id`, `event_key`, `quantiles` JSONB, `unit`, `horizon`,
  `provenance` JSONB, `created_at` (append-only, INSERT-only at the app layer).
- scalar resolution: the realized `value`, written exactly-once over the resolution
  window (the same score-once discipline the binary path enforces).
- `belief_scores`: `(belief_id, rule_id, score, scored_at)` — append-only; one row per
  (belief, rule). The binary path's `brier` column is "the BrierRule score" conceptually;
  it migrates onto `belief_scores` as a careful follow-up (slice 1 does NOT rip out the
  working binary scoring under track A — it adds the trait + the scalar path, and provides
  `BrierRule` so the binary path can adopt it incrementally).
- One migration in `crates/fortuna-ledger/migrations/`.

### 1.5 Two producers, one type

- **External producers** (Aeolus weather, personas) emit `prob_claims/v1` scalar
  envelopes through the existing `Source` → `SignalEnvelope` path (signal-contract.md §2);
  the strict mapper turns the scalar outcome into a `PredictiveDistribution::Scalar`.
- **Internal deterministic forecasters** (funding_forecast) construct the
  `PredictiveDistribution::Scalar` directly — no envelope round-trip for internal data.
- Both land in the same `scalar_beliefs` store, scored by the same `CrpsPinballRule`. That
  is the "design once" result.

## 2. The perp-strategy runtime seam

Two facts force this section: perp prices are `PerpPrice` (type-separated from `Cents`),
and the `Strategy`/`Proposal`/`ProposedLeg`/`CoreHandle` framework is event-contract-
shaped. The seam threads perp data in WITHOUT bending those types.

### 2.1 Perp market data on the bus: `PerpTick`

A new typed `EventPayload` variant (the `fortuna-core` bus already invites "typed variants
added by the tasks that own them"):

```rust
PerpTick {
    venue: VenueId,
    market: MarketId,                // e.g. KXBTCPERP / KXBTCPERP1
    marks: PerpMarks,                // settlement + conservative (+ liquidation/reference avail.)
    funding: FundingObservation,     // the venue ESTIMATE (running TWAP) + next_funding_time + ref
}
```

`FundingObservation` carries the venue's recorded funding **estimate** (the running TWAP)
+ `next_funding_time` + `reference_price` (the CF Benchmarks index). The premium proxy is
then `PerpMarks.venue_settlement − FundingObservation.reference_price` — settlement comes
from `marks`, reference from `funding`, no field duplicated. **Grounded in the fixtures
(§4): every field is a verbatim recorded field** of the WS `ticker` frame +
`/funding_rates/estimate`. The live feed/recorder
publishes `PerpTick`s; the Sim/DST/paper harness injects them. A strategy reads them in
`on_event` — no `CoreHandle` surgery.

### 2.2 Two strategy archetypes

- **funding_forecast — belief-producer, zero-capital.** Consumes `PerpTick`s; forecasts
  the FINAL funding rate from the recorded estimate trajectory (see §2.3); emits a scalar
  `PredictiveDistribution` (quantiles over the next finalized funding rate) via
  `drain_beliefs()`. NO `Proposal`, no perp execution path, no `Cents` impedance. It is the
  first scalar consumer and exercises §1 end-to-end. Stage = Sim; scored by `CrpsPinballRule`
  against the realized funding rate at `next_funding_time`.
- **perp_event_basis — trader.** Trades the event-contract BRACKET legs (`Cents`, the
  existing `Proposal` path) using the perp + funding as a price INPUT (§3). This fits the
  current trader plumbing with NO perp-execution surgery. A native perp-leg execution path
  is a separate, larger, explicitly-deferred item.

### 2.3 funding_forecast's input (refined by the fixture grounding, §4)

Raw 1-minute premiums are NOT recorded and the premium-index formula is venue-unpublished
(research §11). So:

- **Authoritative input: the recorded funding ESTIMATE** — which the venue defines as "the
  time-weighted average of the premium index over `[last_funding_time, now)`", i.e. the
  running TWAP itself. funding_forecast tracks the estimate across capture cycles and
  forecasts the final rate (the estimate projected to `next_funding_time`, finalized via
  `finalize_funding_rate`).
- **Secondary path: `FundingWindow` over a `settlement_mark − reference_price` premium
  proxy** — the already-built kernel (commit 507b1ad), fed an approximate premium (labeled
  approximate, since the exact formula is unpublished). Useful when premium-resolution data
  improves; not the primary number.
- The scalar `PredictiveDistribution`'s quantiles = the point forecast (central quantile)
  + a dispersion model that narrows as the window elapses (`FundingWindow::remaining()`).
  The exact dispersion shape is a rung-0 modelling choice, documented and unit-tested; it
  is the thing CRPS then measures and calibration refines.

### 2.4 Files touched

New files in `fortuna-runner` (`funding_forecast.rs`, `perp_event_basis.rs`) + the
`PerpTick` variant in `fortuna-core` bus + the basis kernel in `fortuna-core` perp. All
additive; the ONE shared touch is registering the strategies in the daemon composition
(`fortuna-live`), the track-A coordination point (§5).

## 3. The basis model (perp_event_basis)

Two surfaces on the same underlying (BTC), same venue family, same CF Benchmarks
reference, ZERO cross-venue settlement/latency risk:

- **Perp side — the point forecast of the fixing**: `settlement_mark_price` (operator-
  delegated choice; the mark the venue itself uses for settlement AND funding — the most
  defensible single number, already embodying the venue's premium handling). The funding-
  adjusted refinement (project the premium decay over the bracket horizon using
  funding_forecast's output) is a clean LATER synergy — rung-0 uses the mark directly.
- **Bracket side — the implied distribution**: the KXBTC15M event-contract ladder. Its
  central estimate is the implied median (the strike where cumulative implied probability
  crosses 0.5), computed from bracket prices (the existing bracket machinery's grain).

### 3.1 Signal + trade (deterministic, mechanical, rung-0)

- `basis = perp_forecast − bracket_implied_median`.
- Tradeable when `|basis|` exceeds BOTH a configured threshold (underlying price units)
  AND the round-trip bracket fees at the FEE-TRAP rate (assumed post-promo fees; promo-$0
  never justifies GO; amendment C). I7 unchanged.
- **Trade the bracket legs** (`Cents`), maker-only, toward the perp forecast — buy the
  brackets the perp says are underpriced. Worst-case loss = premium (event contracts), so
  the existing gate caps apply; NO perp/margin leg in rung-0.
- Stage = Sim; the harness sizes (I6). The basis math is a deterministic kernel in
  `fortuna-core` perp, unit-tested with synthetic ladders + perp forecasts.

### 3.2 The live-data drive (the fixture unblock)

The recorder on the main checkout pairs perp books with KXBTC15M bracket quotes by
`cycle_id` in `data/perishable/`. The perp side is fixture-gated; the **KXBTC bracket side
is NOT a committed fixture** (it lives only in the live perishable stream). To make
perp_event_basis fixtures-gated end-to-end:

- I sample ONE paired cycle (matching `cycle_id`: a perp book + marks + the KXBTC15M
  bracket quotes) into a committed fixture `fixtures/perishable/btc-perp-vs-kxbtc15m-
  <cycle>.json`, **reading the existing perishable stream, WITHOUT touching the running
  recorder** (loop rule).
- If the perishable stream is not yet carrying KXBTC15M brackets, that is the precise
  operator/recorder item I ledger (and the basis strategy ships unit-tested-only until it
  lands). The basis KERNEL (deterministic) does not block on it.

## 4. Fixture grounding (confirmed 2026-06-13 against `fixtures/kinetics-perps/` + the recorder)

- **`PerpTick` — fully grounded.** The WS `ticker` frame carries `settlement_mark_price`,
  `liquidation_mark_price`, `reference_price` (each `{price, ts_ms}`) and `funding_rate
  {rate, next_funding_time_ms, ts_ms}`; `/funding_rates/estimate` carries `{funding_rate,
  mark_price, next_funding_time}`. `PerpMarks` + `FundingObservation` map 1:1. No invention.
- **funding premiums — NOT recorded.** The estimate (running TWAP) is the authoritative
  input; the `mark − reference` proxy is the labeled secondary (§2.3).
- **basis paired data — perp side fixture-gated; bracket side live-perishable only.** The
  live-data drive (§3.2) samples a committed paired-cycle fixture; the bracket-fixture gap
  is the operator/recorder unblock if the stream lacks brackets.

## 5. Ownership + sequencing

This is an OPERATOR-DIRECTED cross-cutting build (the operator's design directive
transcends the per-track loop's crate partition). It spans `fortuna-cognition` +
`fortuna-ledger` (the scalar foundation), `fortuna-runner` (the strategies),
`fortuna-core` (PerpTick + basis kernel). Track C owns it as ONE effort with the
operator-granted expanded scope, building ADDITIVELY (new files), and coordinating
FILE-LEVEL with neighbors: track A at the daemon-composition registration (do not rewrite
track A's runner files), and track D if/when Aeolus adopts the scalar `prob_claims/v1`
(the mapper is shared). Every slice is independently gate-clean with the full battery.

**Build sequence (each its own gate-clean iteration + battery):**

1. **Scalar belief foundation** — `PredictiveDistribution` / `RealizedOutcome` /
   `ScoringRule` + `BrierRule` + `CrpsPinballRule` (cognition); `scalar_beliefs` +
   `belief_scores` + scalar resolution + one migration (ledger). Tests: pinball/CRPS
   vectors + proper-scoring properties, quantile validation, re-scoring determinism,
   storage round-trip. (Self-contained; no perp dependency.)
2. **funding_forecast** — `PerpTick` variant (fortuna-core bus) + the strategy
   (fortuna-runner) consuming `PerpTick`s → scalar `PredictiveDistribution` from the
   estimate (FundingWindow proxy secondary); standalone strategy tests + a perp DST arm
   (belief production under funding-tick chaos). First scalar consumer end-to-end.
3. **perp_event_basis** — the basis kernel (fortuna-core perp, deterministic, synthetic-
   input tests) + the live-data fixture drive (§3.2) + the bracket-trader strategy
   (fortuna-runner, bracket legs) + a fixtures-gated end-to-end test once the paired-cycle
   fixture lands.
4. **Daemon composition** — register both strategies into the Sim runner (coordinate with
   track A); confirm they run in a Sim soak.

Phase-5 EXIT (T5.B7) completes when 1–4 land gated. T5.B8 (perps ops) remains a separate
queue item with its own kill-switch-collision coordination (out of scope here).

## 6. What this deliberately does NOT do (YAGNI)

- No native perp-leg trading (perp_event_basis trades brackets; perp legs are a later,
  larger item).
- No scorecard export (the Aeolus-as-API product; v2, after a second consumer).
- No binary-path rewrite (binary adopts `ScoringRule`/`belief_scores` as a careful
  follow-up, not a slice-1 prerequisite).
- No funding_carry strategy (DATA-ONLY until ≥60d funding history, amendment B).
- No streaming/bus transport for external claims (batch envelopes suffice at trigger
  cadence, per signal-contract.md §4).

## 7. Testing + invariants

- **Scalar scoring**: pinball/CRPS property tests (a sharper-correct forecast scores
  lower; symmetry; the degenerate 1-quantile case; a real recorded funding rate as a
  vector), strict-parse/validation tests from the spec text (quantile monotonicity, bin
  sums), re-scoring determinism + multi-rule side-by-side.
- **funding_forecast**: belief-production tests against the recorded estimate trajectory;
  a DST arm (beliefs produced + scored under tick/gap chaos; determinism).
- **perp_event_basis**: basis-kernel unit tests (synthetic ladders + perp forecasts,
  threshold + fee-trap boundaries); fixtures-gated end-to-end once the paired cycle lands;
  the fee-trap edge floor enforced.
- **Invariants**: I6 (both archetypes PROPOSE; the harness sizes — funding_forecast
  proposes nothing, perp_event_basis proposes unsized bracket legs); I7 (Sim stage, no
  auto-promotion, the fee-trap gate on GO); I5 (every belief/score write audited);
  money/forecast boundary (scalar `f64` is forecast-quality, never money — the
  PerpPrice/Cents conversion stays at the gate/exec edge).
