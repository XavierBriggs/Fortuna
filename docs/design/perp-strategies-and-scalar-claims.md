# Perp strategies (T5.B7): runtime seam, `prob_claims/v1` scalar claims + swappable scoring, and the perp_event_basis basis model

Status: design approved by the operator in the 2026-06-13 brainstorming session (the
load-bearing decisions — native-CRPS scalar scoring, the swappable `ScoringRule` layer,
the `PredictiveDistribution`/`RealizedOutcome` naming, the `settlement_mark` basis
forecast, "build what you need to be complete" — were made by the operator there); the
adversarial design critique (track-c-scalar-claims-design-critique-2026-06-13.md,
ACCEPT-WITH-CONDITIONS) is folded in (the A3 egress-seam must-fix is §2.5; watch-items
addressed). Build proceeds SLICE-BY-SLICE, each gate-clean. BUILD_PLAN T5.B7 is IN
PROGRESS (the `FundingWindow` kernel, 507b1ad, is done; this design covers the remaining
slices §5); no box is ticked until its slice lands gated.
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
  `score = mean_k pinball_{q_k}(y, v_k)`. A proper scoring rule; lower is better. The
  single-quantile (median, q=0.5) degenerate case reduces to scaled absolute error
  `|y − v|/2` — the proper score for the median; it is NOT Brier's squared error (the two
  rules are distinct instances of the same swappable `ScoringRule`, not reductions of each
  other).

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
  `provenance` JSONB, `created_at`. **Append-only with the SAME DB-level trigger the
  binary tables carry** (I5; mirror the INSERT-only/no-UPDATE-no-DELETE trigger from the
  initial migration) — INSERT-only at the app layer too.
- scalar resolution: the realized `value`, written **exactly-once** over the resolution
  window — mirror `BeliefsRepo::resolve_and_score` (repos.rs:1056), which the repo enforces
  to be a single write per belief; the scalar resolver carries the identical guard so a
  belief is scored once.
- `belief_scores`: `(belief_id, rule_id, score, scored_at)` — append-only (same trigger);
  one row per (belief, rule). The binary path's inline `brier` column is "the BrierRule
  score" conceptually; it migrates onto `belief_scores` as a careful follow-up (slice 1
  does NOT rip out the working binary scoring under track A — it adds the trait + the
  scalar path, and provides `BrierRule` so the binary path can adopt it incrementally).
- One migration in `crates/fortuna-ledger/migrations/` (new tables + their append-only
  triggers; binary tables untouched).

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

**Instrument — `market` is demo-vs-prod, the SAME underlying (cross-slice confirm,
2026-06-14, grounded in `docs/research/venue/kinetics-perps-2026-06-10/research.md`).**
"Kinetics" is Kalshi's perpetual product (the Kalshi `/margin` API — NOT a separate venue;
`fortuna-venues/src/kinetics/mod.rs:1`, recorder host `external-api.demo.kalshi.co`). The
BTC perp lists as **`KXBTCPERP` in PROD** (research §3 line 133: 0.0001 BTC, BRTI index) and
**`KXBTCPERP1` in DEMO** (research §3 line 202: demo tickers carry a `1` suffix). They are the
SAME contract, differing only by environment. **Both strategies are environment-driven, NOT
hardcoded:** `funding_forecast` keys on `PerpTick.market` (whatever the feed carries — only its
test/fixture constants say `KXBTCPERP1`), and `perp_event_basis.perp_market` is a config field
(the basis kernel is instrument-agnostic — an f64 mark + bins). So a demo capture carries
`KXBTCPERP1`, a prod capture carries `KXBTCPERP`, and neither producer is mis-pointed; "0
`KXBTCPERP1` rows in a real (prod) Kalshi capture" is expected. **Fixture-hygiene rule:** any
NEW unified/paired fixture must use ONE environment's ticker consistently across the perp + the
funding series — the committed funding fixtures are demo `KXBTCPERP1`, the basis paired-cycle is
prod-aligned `KXBTCPERP`; each is internally consistent, but never mix the two in one cycle.

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
  `PredictiveDistribution` (quantiles over the next finalized funding rate) via a NEW
  additive egress seam (§2.5). NO `Proposal`, no perp execution path, no `Cents` impedance.
  It is the first scalar consumer and exercises §1 end-to-end. Stage = Sim; scored by
  `CrpsPinballRule` against the realized funding rate at `next_funding_time`.
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

### 2.5 The scalar-belief egress seam (must-fix A3 — `drain_beliefs()` is binary-only)

`Strategy::drain_beliefs()` returns `Vec<BeliefDraft>`, and `BeliefDraft`
(fortuna-cognition beliefs.rs) is `deny_unknown_fields` with a REQUIRED `p: f64`
validated strictly in (0,1) — it is BINARY-ONLY. A scalar `PredictiveDistribution`
CANNOT flow through it, and (per the design's own "binary path untouched, no track-A
collision" constraint) it must NOT be widened. So scalar egress is a NEW ADDITIVE seam:

- a new `Strategy` trait method `drain_scalar_beliefs(&mut self) -> Vec<ScalarBeliefDraft>`
  (default `Vec::new()`, so every existing strategy is unaffected — same additive shape
  `drain_beliefs()` itself uses);
- a parallel runner buffer that drains it each tick (mirroring the binary
  `drain_pending_beliefs` path);
- a parallel daemon persist into `scalar_beliefs` + `belief_scores` (mirroring the binary
  `persist_beliefs`), append-only + audited.

`ScalarBeliefDraft` carries `{event_key, predictive: PredictiveDistribution::Scalar,
horizon, evidence}` — the scalar analog of `BeliefDraft`, the harness stamping provenance.
This is the ONLY correct §1 mechanism; the doc previously mis-named it `drain_beliefs()`.

### 2.4 Files touched

New files in `fortuna-runner` (`funding_forecast.rs`, `perp_event_basis.rs`) + the
`PerpTick` variant in `fortuna-core` bus + the basis kernel in `fortuna-core` perp + the
`drain_scalar_beliefs()` method on the `fortuna-runner` `Strategy` trait (§2.5). All
additive (default-impl trait method; new files). TWO shared touch-points to coordinate
with track A (§5): the `Strategy`-trait scalar drain method AND registering the strategies
in the daemon composition (`fortuna-live`) — neither rewrites track A's existing files.

### 2.6 funding_forecast scoring + quantile acceptance (binding; operator-endorsed 2026-06-13)

Two binding requirements refine §2.3. They are ACCEPTANCE criteria, not new mechanism —
§1.3's multi-rule, side-by-side scoring over the immutable `(PredictiveDistribution,
RealizedOutcome)` facts already supports them. (Endorsed amendments A2b, A2d.)

- **A2b — fixed quantile set.** funding_forecast's `PredictiveDistribution::Scalar`
  carries exactly the seven quantiles `{0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95}` (§2.3
  previously said "central quantile + dispersion", unfixed). Seven points characterize the
  body and both tails enough for CRPS and band-coverage without overfitting the dispersion
  model. The §1.1 validation (q strictly increasing, v non-decreasing, finite, ≥2 quantiles)
  enforces well-formedness; this fixes the SET. A unit test pins the produced q-vector to
  these seven.
- **A2d — must beat baselines (the edge test, the I7-spirit gate for a belief producer).**
  funding_forecast has no edge unless its CRPS BEATS naive baselines on the SAME resolved
  windows — above all **carry-forward** (the current venue funding ESTIMATE, the §2.3
  authoritative input, projected FLAT to `next_funding_time`), and also last-realized-rate
  and a random-walk. Implement each baseline as a trivial `PredictiveDistribution::Scalar`
  producer over the same ticks, score them side-by-side via §1.3's `(belief_id, rule_id)`
  rows (keyed here by a `producer`/`baseline` label), and surface the comparison (ROTA §9.1
  renders it). Carry-forward is THE bar: if funding_forecast cannot beat the venue estimate
  carried forward, it stays DATA-ONLY (no promotion past Sim) until it can. A test asserts
  the comparison is COMPUTED (the baseline rows exist); promotion is the operator's call on
  the measured result, never automatic (I7).

These are MEASUREMENT requirements: no behavior change to the gate/exec path, no money
touch (CRPS/quantiles are f64 forecast-domain). They make funding_forecast's edge
FALSIFIABLE rather than assumed.

## 3. The basis model (perp_event_basis)

Two surfaces on the same underlying (BTC), same venue family, same CF Benchmarks
reference, ZERO cross-venue settlement/latency risk:

- **Perp side — the point forecast of the fixing**: `settlement_mark_price` (operator-
  delegated choice; the mark the venue itself uses for settlement AND funding — the most
  defensible single number, already embodying the venue's premium handling). The funding-
  adjusted refinement (project the premium decay over the bracket horizon using
  funding_forecast's output) is a clean LATER synergy — rung-0 uses the mark directly.
- **Bracket side — the implied distribution**: the **KXBTC** event-contract ladder (CORRECTED
  2026-06-13 against the live capture — see GAPS "LIVE BRACKET-FORMAT INVESTIGATION"). KXBTC is the
  price-LEVEL ladder: `strike_type=between` range bins (e.g. "$74,500 to 74,999.99") plus open
  `greater`/`less` tails, YES quoted in dollar-strings on a $1 payout. Its central estimate is the
  implied median (the strike where cumulative implied probability crosses 0.5). NOTE: KXBTC15M
  (this section's original guess) is NOT this — it is a single DIRECTIONAL "BTC up in 15 min?"
  binary (`greater_or_equal`, floor=reference price), a P(up) not a price distribution; KXBTCD is a
  cumulative-threshold (CDF) ladder.

### 3.1 Signal + trade (deterministic, mechanical, rung-0)

- `basis = perp_forecast − bracket_implied_median`.
- Tradeable when `|basis|` exceeds BOTH a configured threshold (underlying price units)
  AND the round-trip bracket fees at the FEE-TRAP rate (assumed post-promo fees; promo-$0
  never justifies GO; amendment C). I7 unchanged.
- **Trade the bracket legs** (`Cents`), maker-only, toward the perp forecast — buy the
  brackets the perp says are underpriced. Worst-case loss = premium (event contracts), so
  the existing gate caps apply; NO perp/margin leg in rung-0.
- Stage = Sim; the harness sizes (I6). The basis math is a deterministic kernel in
  `fortuna-cognition` (`basis.rs` — f64-forecast, NOT core, per the money-discipline), unit-tested
  with synthetic ladders + perp forecasts. Slice 3b (DONE) refined the kernel to the REAL
  3-strike-type ladder: a `BracketStrike` enum {`Between`{floor,cap}, `Greater`{floor},
  `Less`{cap}} with `BracketBin{kind, prob: f64}`; the median INTERPOLATES within the crossing
  `between` bin and returns `None` when the 0.5 crossing lands in an OPEN tail (no finite width —
  conservative, no fabricated point). The dollar-string→probability parse is the CALLER's boundary
  (the kernel takes the `f64` mid, staying format-agnostic), and `compute_basis` takes the perp
  mark as caller-supplied `f64` BTC-dollars — the kernel has ZERO money-type touch (the
  per-contract→BTC ×10000 boundary is the caller's). The real-KXBTC e2e (`basis_live_fixture.rs`)
  now drives it on the committed paired cycle (§3.2).
- The `Cents` bracket-leg STRATEGY is DONE (`fortuna-runner::perp_event_basis`, propose-only, additive —
  a NEW module, NO venue-DTO change). On a `PerpTick` it rebuilds bins from `core.books` (YES mid
  `(bid_or_0 + ask_or_0)/2`, an absent quote = the 0c floor — REQUIRED so the live `0 bid / Nc ask` far
  tails keep their `ask/2` mass and the strategy reproduces the kernel's validated basis), calls
  `compute_basis`, and proposes ONE maker-only (`Urgency::Passive`) UNSIZED `Cents` leg (I6 — the harness
  sizes) on the bin CONTAINING the perp forecast, gated by the fee-trap. `fair_value = limit + premium`
  (clamped ≤99), mirroring `MechExtremes`. The strategy holds its OWN bracket catalog
  (`MarketId → BracketStrike`, injected at construction); the catalog is NOT read from `core.markets`
  (`fortuna_venues::Market` carries no strike metadata — out of scope to widen), so live
  catalog-population from the Kalshi market list is the slice-4 daemon concern. VALIDATED on live DEMO
  data: the committed e2e on cycle …753775 (basis −$55.53) + a fresh independent cycle …754035 (basis
  +$55.08), both with perp/ladder agreement <0.1%.

### 3.2 The live-data drive (the fixture unblock)

The recorder on the main checkout pairs perp books with **KXBTC** bracket quotes by `cycle_id` in
`data/perishable/<day>/{perp_orderbook,bracket_quotes}.jsonl` — CONFIRMED LIVE 2026-06-13 (the
recorder runs `--bracket-series KXBTC15M,KXBTC,KXBTCD`; the KXBTC `between` ladder IS being
captured). The perp side is fixture-gated; the KXBTC bracket side is not yet a committed fixture.
To make perp_event_basis fixtures-gated end-to-end (now drivable by me, operator-directed):

- Sample ONE paired cycle (matching `cycle_id`: a perp book + marks + the KXBTC `between`-ladder
  bracket quotes) into a committed `fixtures/kinetics-perps/` file (market data ONLY, no keys),
  **reading the existing perishable stream, WITHOUT touching the running recorder** (loop rule).
  DONE: `fixtures/perp-basis/paired_cycle_btc_perp_vs_kxbtc.json` (cycle
  1781160753775; 48 `between` + 1 `greater` + 1 `less`). It lives under `derived/` because it is
  a recorder-DERIVED pair, not a single Kinetics DTO capture — the top-level `kinetics_dto`
  coverage glob (non-recursive) must not require it to parse as a venue DTO.
- The basis KERNEL (deterministic) does not block on it; both the fixture and the kernel
  KXBTC-tail refinement (slice 3b) have LANDED, so `basis_live_fixture.rs` now drives the kernel
  end-to-end on real co-recorded data. The STRATEGY (the sized `Cents` bracket-leg trade) is the
  remaining fixture-gated step.

### 3.3 Basis model v2 — per-bracket EV, horizon-gated, freshness-measured (binding slice-3b-v2 spec; operator-endorsed 2026-06-13)

§3.1's rung-0 is DONE and demo-validated (median basis + buy-the-containing-bin; merged;
two independent cycles agreed <0.1%). v2 is the next rung: it replaces the single-median
signal with a per-bracket fair-probability model and a real expected-value gate, and it
stops ASSUMING the perp is the better price — it MEASURES it. Every change preserves the
rung-0 invariants: I6 (unsized propose-only; the harness sizes), I7 (Sim-stage, no
auto-promotion), the f64-forecast / `Cents`-money split, no `panic!`/`unwrap`/`expect`,
and "veto = propose nothing". The rung-0 kernel/strategy are NOT deleted — the median
becomes a health metric (A10) and the rung-0 path remains the fallback when v2's richer
inputs are unavailable. (Endorsed amendments A3, A6, A5, A9, A4, A8, A7, A10.)

- **A3 — per-bracket fair probability `q_j`, not a median.** For each bracket bin j compute
  the model probability that settlement lands in it:
  `q_j = P(settlement ∈ bin_j | S₀, σ, τ) = F(cap_j) − F(floor_j)` (open tails:
  `q_greater = 1 − F(floor)`, `q_less = F(cap)`), where F is the model settlement-price CDF
  centered on the anchor S₀ (A6) with dispersion σ over horizon τ (A5). Rung-0-of-v2 model:
  Gaussian in log-price (lognormal settlement); `σ` from the realized volatility of the
  perp-mark series scaled by √τ, config-overridable. The bracket-IMPLIED σ is a cross-check
  DIAGNOSTIC (A10), NEVER the pricing input — using the ladder's own dispersion to price the
  ladder is circular. The `q_j` vector is the new signal; `bracket_implied_median` is
  retained, demoted to diagnostics. All f64-forecast; degenerate inputs (non-finite σ, τ≤0,
  empty ladder) → empty `q_j` → no proposal (mirrors the kernel's `None`).

- **A6 — BRTI is the settlement anchor, not the perp mark; veto on stale feed.** KXBTC
  brackets resolve to the CF Benchmarks reference (BRTI family), NOT the perp settlement
  mark. So S₀ = the reference index (`FundingObservation.reference_price`, already on
  `PerpTick`); the perp mark's DEVIATION from it is the funding/premium SIGNAL (what
  funding_forecast models), not the anchor. At short horizon the two nearly coincide
  (rung-0's approximation); A6 makes the anchor explicit and correct at longer τ. The
  reference frame carries `ts_ms` (§4) — if it is STALE past a configured age
  (Clock-measured), the anchor is untrustworthy → DISABLE basis trading on that tick
  (propose nothing). Mis-anchoring mis-prices every `q_j`, so this veto is load-bearing.

- **A5 — horizon gating.** Compute τ = bracket settlement time − now (injected Clock).
  Three regimes: `τ ≤ 4h` **direct** (short-horizon σ; mark/reference a tight point
  forecast); `4h < τ ≤ 48h` **vol-adjusted** (σ scales with √τ; premium-decay from
  funding_forecast matters; widen F); `τ > 48h` **disabled** (the point-forecast+σ model is
  not trustworthy → propose nothing). The regime selects the σ model or vetoes; it never
  sizes (I6).

- **A9 — ladder no-arb validation (gate the inputs).** Before trusting executable quotes,
  validate the ladder is coherent: YES-implied probabilities monotone-consistent with a CDF
  (implied cumulative non-decreasing across the price-ordered bins), YES-sum ≈ 1 within
  tolerance, no crossed quotes implying a free lock (a genuine lock is `mech_structural`'s
  arbitrage, NOT a basis trade). A ladder that fails no-arb → DISABLE basis trading on it
  (propose nothing): you cannot compare `q_j` to an incoherent price vector. A health gate,
  not a trade.

- **A4 + A8 — per-bin expected-value gate with maker adverse-selection.** Replace rung-0's
  scalar `|basis| > fee_floor + min_basis` with a PER-BIN EV gate in probability units:
  `EV_j = q_j − ask_j − fee − slippage − reserve − adverse_j`, where `ask_j` is the
  EXECUTABLE YES price you would pay (the ASK, not the mid), `fee` is the round-trip fee at
  the fee-trap rate (amendment C, assumed post-promo; promo-$0 never lowers it), `slippage`
  and `reserve` are configured margins, and `adverse_j` (A8) is the maker adverse-selection
  penalty: a passive bid fills preferentially when flow is informed against it, so the
  realized fill is worse than `q_j − ask_j` implies — discount accordingly. Propose bin j
  only when `EV_j > threshold`. EV is an honest f64 edge claim and the strategy's GO/no-go
  (rung-0's fee-trap analog), NOT a size — the leg stays UNSIZED `Cents` (I6). Multiple bins
  may clear; each is a separate unsized maker leg (the harness sizes/caps).

- **A7 — measure "the perp is better informed," don't assume.** "Trade toward the perp"
  only holds when the perp price actually LEADS the bracket. Measure it per tick: per-side
  spreads, top-of-book depth, and quote staleness (age) of BOTH the perp and the target
  bracket bin. Form a relative-informativeness signal; when the bracket bin is
  fresher/tighter/deeper than the perp on the relevant side, do NOT assume the perp is right
  — down-weight (raise `reserve`/`adverse_j`) or veto that bin. Conservative default:
  unknown/stale → treat as NOT perp-favorable. DATA CAVEAT: per-level quote ages may not be
  on the current `OrderBook`/fixture; if so, record the gap in GAPS and gate on what IS
  available (bracket-vs-perp cycle freshness from the recorder pairing), treating missing
  age as stale — never assuming fresh.

- **A10 — full-CDF diagnostics; median → health metric (C produces, B displays).** The
  median is demoted from signal to a HEALTH metric. C PRODUCES the diagnostic data: the
  model `q_j` vector, the implied CDF (from quotes) vs the model CDF (from S₀,σ,τ), their
  divergence, and band-coverage of realized settlements — emitted as named `MetricSample`s
  (§8 grain) and carried in the proposal thesis/provenance. The DISPLAY half (rendering the
  fan/CDF, the basis trail) is track-B's ROTA §9.2 — C ships the numbers, B paints them
  (the §9 data-vs-view split).

**Build order (each a gate-clean ADDITIVE slice; v2 is POST-T5.B7-EXIT — see §5):**
1. §2.6 first (A2b 7-quantile + A2d baseline-beat) — isolated, no perp-strategy dependency.
2. A3 + A6 anchor — the `q_j` model on the BRTI anchor; median → diagnostic (A10 data).
   Kernel-level, synthetic-input tested.
3. A9 no-arb validation — guards A3's inputs.
4. A5 horizon gating — wraps A3 with the τ-regime σ model + the >48h veto.
5. A4 + A8 EV gate — the per-bin EV with adverse-selection replaces the scalar gate.
6. A7 measured informativeness + A6 stale-veto + A10 diagnostic emission — the freshness/
   health surface.
Each preserves propose-only/unsized/Sim and degrades to "propose nothing" on any degenerate
or stale input. The rung-0 path stays as the fallback; v2 activates only when its richer
inputs are present and coherent.

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
operator-granted expanded scope, building ADDITIVELY (new files + default-impl trait
methods), and coordinating FILE-LEVEL with neighbors. TWO shared touch-points on
track-A-adjacent files to coordinate (neither rewrites track A's existing logic): (i) the
`drain_scalar_beliefs()` method on the `fortuna-runner` `Strategy` trait (§2.5), and (ii)
registering the strategies in the daemon composition (`fortuna-live`). Track D coordinates
if/when Aeolus adopts the scalar `prob_claims/v1` (the mapper is shared). Every slice is
independently gate-clean with the full battery.

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

**Post-EXIT refinement — basis model v2 (§3.3) + funding_forecast scoring (§2.6),
operator-endorsed 2026-06-13.** Slices 1–4 (rung-0) ARE T5.B7 EXIT and are DONE/merged. The
v2 spec (§3.3 A3/A6/A5/A9/A4+A8/A7/A10; §2.6 A2b/A2d) is a SEPARATE post-EXIT queue owned by
track C, built as gate-clean additive slices (the §3.3 build order). SEQUENCING (operator to
confirm): recommended BEHIND the Kalshi demo-flip in C's queue — the demo-flip unblocks live
observability of the already-producing funding_forecast (the operator's "demo mode" goal),
whereas v2 deepens a Sim-stage, propose-only, non-live-capital strategy (I7) whose rung-0 is
already merged, so it gates nothing live. F5–F9 was moved off C → E (2026-06-14), freeing C's
load for this.

**v2 PROGRESS (2026-06-14):** §2.6 **A2b** (the fixed seven-quantile fan, @79e3dad) and **A2d
slice-1** (the carry-forward baseline kernel `funding_baselines.rs`, @0bb6d27) are MERGED + gated.
The per-bracket EV trader (§3.3 A3/A6/A5/A9/A4+A8/A7/A10) and A2d slice-2 (wiring the side-by-side
score into funding_forecast) remain unbuilt.

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

## 8. Telemetry (rich, slots into the existing metrics, extensible by construction)

The repo's telemetry grain (spec §8) is the **named `MetricSample`** (`runner.rs`:
`{name: &'static str, help, counter: bool, labels: Vec<(String,String)>, value: i64}`),
which the runner emits and the ops layer maps into `fortuna_ops::metrics::MetricsRegistry`
→ the `GET /metrics` endpoint. The runner stays telemetry-dependency-free; the ops layer
owns the registry. **We slot in by EMITTING new named samples — no struct field added to
the shared `StrategyMetrics`/`RunCounters`, no schema migration.** That IS the
extensibility property: a new metric, a new producer, a new scorer is a new
`(name, labels, value)` tuple, nothing else.

**Dimensional scheme (labels do the work, so expansion never forks the schema):**

- Scalar-belief lifecycle (producer-agnostic — funding_forecast now, Aeolus/personas
  later, identical metric, new `producer` label value):
  - `fortuna_scalar_beliefs_emitted_total{producer}` (counter)
  - `fortuna_scalar_beliefs_resolved_total{producer}` (counter)
- Scoring/calibration (labeled by `rule_id`, so swapping or A/B-ing scorers is visible —
  the `ScoringRule` swappability surfaced in telemetry):
  - `fortuna_scalar_score{producer, rule_id}` (gauge: rolling mean score, lower-better;
    e.g. rolling-N CRPS)
  - `fortuna_scalar_quantile_coverage{producer, band}` (gauge: realized fraction inside
    the e.g. 0.1–0.9 band ×10⁴ — calibration at a glance)
- funding_forecast: `fortuna_funding_forecast_rate{market}` (gauge, point forecast ×10⁶),
  `fortuna_funding_window_elapsed{market}` (gauge, candles observed).
- perp_event_basis: `fortuna_perp_basis{market}` (gauge, signed basis in underlying
  ten-thousandths), `fortuna_perp_basis_signals_total{market}` (counter, tradeable
  inconsistencies past the fee-trap floor); proposals/fills ride the existing
  `StrategyMetrics`.

**Value encoding** (the registry is `i64`): floats are fixed-point-scaled with the scale
named in `help` (rate ×10⁶, basis in ten-thousandths, coverage ×10⁴) — the same
integer-telemetry discipline the existing counters use; no `f64` enters the registry.
The scalar score gauges read off the durable `belief_scores` rows (§1.3), so they are
re-derivable and consistent with the ledger, not a parallel truth.

## 9. ROTA views (track-B can build these in the meantime — read-only spec)

ROTA is track-B's (`fortuna-ops/src/rota/`), read-only doctrine ABSOLUTE: zero mutating
endpoints, gold-on-black tokens, per-panel degraded state (HTTP 200, never 500), a
SEPARATE read-only pool (never the daemon's), `GET /api/rota/v1/<view>` snapshot JSON +
short-poll. This section is the **view CONTRACT** track-B builds against; track C ships
the data (the `scalar_beliefs`/`belief_scores` tables, slice 1b) the queries read. Track B
can build the panels + degraded states NOW against these contracts; they light up when the
data lands. (This refines the design's already-deferred "perps/funding-regime panel".)

### 9.1 `GET /api/rota/v1/forecasts` — the scalar-forecast SCORECARD (producer-agnostic, the headline)

The view that "shows the outcomes of the perps vendor and the whole process," generalized:
one row per `producer` (funding_forecast, and later aeolus/personas with ZERO view
change). Per producer:
- the recent scalar beliefs: the quantile fan (e.g. 0.1/0.5/0.9), `unit`, `horizon`,
  `event_key`, and the realized `value` once resolved (the forecast-vs-outcome pair —
  the heart of "did the vendor call it");
- the calibration summary: rolling mean score PER `rule_id` (swappable scorers shown
  side by side — Brier-of-binarized vs CRPS, when more than one runs), quantile-band
  coverage (is the 0.1–0.9 band ~80%?), resolved-N;
- a sparkline-ready series of (issued_at, median, realized) for the recent window.
This is the "publish claims, receive calibration-vs-market scorecards" product surface
(signal-contract.md §3), now operator-facing. View JSON: `{producers: [{producer,
calibration: {rule_scores: [{rule_id, mean, resolved_n}], coverage_bps, ...},
recent: [{event_key, issued_at, quantiles, horizon, realized, status}]}], degraded?}`.

### 9.2 `GET /api/rota/v1/perps` — funding regime + basis (perps-specific)

- Funding regime: current funding estimate + `next_funding_time` per market, the
  funding_forecast point+band, and recent realized funding rates (the regime trail) —
  the venue's funding world at a glance.
- Basis: per market, the current `basis = perp_forecast − bracket_median`, the recent
  basis trail, and the basis-signal events (tradeable inconsistencies) with their trade
  outcomes (proposed → gated → filled/rejected → settled PnL) — the perp_event_basis
  story end to end.

### 9.3 "The whole process" — lineage, woven into 9.1/9.2

Rather than a separate panel, each scorecard/basis row is CLICK-TO-EXPAND to its lineage
(the cognition-panel pattern, rota-dashboard.md §5): `PerpTick` ingest → forecast →
scalar belief (provenance) → score (per rule) → realized outcome → (for basis) the
bracket trade + settlement. One honest thread from input to outcome, no new storage (the
provenance + belief_scores rows already carry it).

**Extensibility for track B**: every view is keyed by `producer`/`market` labels, so a new
scalar producer or a new perp market is a new row/series, not a new panel or contract
change — the same zero-schema-change property as §8.

## 10. Why expanding this is trivial (the extensibility principles, made explicit)

Each foundational choice is a SEAM that absorbs the next consumer without a rewrite:

1. **Swappable `ScoringRule`** — a new score (log-loss, weighted-CRPS, a market-relative
   skill score) is one `impl` + its `belief_scores` rows; nothing else moves. Scorers run
   side by side and are backtestable over the immutable `(PredictiveDistribution,
   RealizedOutcome)` facts.
2. **Producer-agnostic scalar type** — funding_forecast, Aeolus weather, and track-E
   personas all emit `PredictiveDistribution::Scalar`; onboarding consumer N+1 is a
   `producer` label + (for external vendors) a registry row, not new Rust (signal-contract
   §3's "an hour, not an afternoon").
3. **Named-sample telemetry** — a new metric/producer/scorer is a new `(name, labels,
   value)` tuple; no shared struct grows, no migration.
4. **Producer-/market-keyed ROTA views** — a new producer or market is a new row/series in
   the existing contracts; track B never re-cuts a panel.
5. **Additive seams everywhere** — `drain_scalar_beliefs()` is a default-impl trait method;
   `PerpTick` is a new bus variant; `scalar_beliefs`/`belief_scores` are new tables beside
   the untouched binary path. Each addition is orthogonal to what exists.

The test of the design: adding "a second weather vendor" or "a persona forecaster" or "a
new perp market" or "a sharper scoring rule" should each touch ONE seam and zero schemas.
By construction here, each does.
