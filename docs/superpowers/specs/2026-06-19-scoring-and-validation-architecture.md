# Scoring & Validation Architecture — FORTUNA's Proof Machinery

**Status:** design (decision-grade v1) · **Date:** 2026-06-19 · **Authority:** `docs/spec.md` (v0.9) > `CLAUDE.md` > this. Invariants I1–I7 are absolute.

**How this was built (triangulation):** reconciled three legs — **code reality** (a 7-agent rules audit + a 6-agent code/spec grounding), **spec & goals intent** (spec 5.5/5.8/5.10/5.14 + I5–I7, the audit/MVP/close-the-loop docs), and **scoring theory** (a 6-agent web-grounded literature review: Gneiting–Raftery 2007, Savage, Murphy 1973, Buja–Stuetzle–Shen 2005, Dawid, Baker–McHale/Chu–Wu–Swartz, Lopez de Prado). Where they disagree, the spec governs intent and the code governs current state; the theory sets the bar.

> **One-line thesis.** FORTUNA's legitimacy is *"we forecast event probabilities better than the market and prove it through process gates — calibration + CLV — not PnL."* Scoring is that proof. A probability that hasn't been properly scored is a guess with a decimal point. This doc specifies *what is scored, by what rule, keyed by what data, how it gates capital, and how it is defended against decay* — decoupled, invariant-safe, per-strategy and per-mind.

---

## 0. Context, North Star, and where Phase C left us

- **North star:** $50k/month **validated** PnL run-rate. "Validated" = sustained, gate-proven edge, not a lucky streak.
- **The demo's job:** accrue **trustworthy validation data** — per-producer scored beliefs (Brier/CRPS + CLV) and realized PnL — so the edge is *measurable* before capital scales.
- **Phase C (just completed A1–A7, B1–B3, C1–C5, D1–D3) closed the PERSISTENCE/loop layer:** fills, settlement→realized-PnL, trade scoring, calibration *persisted* and *delivered to sizing* (B1/B3), bus-recording replay, per-producer belief authorship (meteorologist parallel to Aeolus, D3), venue-neutral discovery. The loop now *writes* scores at all — previously the dominant defect was "computed but never persisted."
- **This doc addresses the PROOF layer that Phase C did not:** the scoring is *recorded* but not yet *rigorous or fully measurable*. The remaining gaps are scoring-quality, not plumbing.

---

## 1. Principles (the non-negotiables)

1. **Proper rules only.** Every belief is graded by a strictly proper scoring rule (Brier, log, CRPS/RPS/WIS). **Never** accuracy / win-rate / hit-rate / ROC-AUC for a probability — they are improper, magnitude-blind, threshold-fragile, and (Harrell) maximizable by dropping a genuinely predictive feature. Catastrophic for a Kelly sizer because they are blind to the calibrated magnitude that sets bet size. Never gate, never dashboard. Trade-PnL (`trade_scores`) is a *separate* legitimate measure, walled off from belief grading — FORTUNA already does this; keep it airtight.
2. **Score by DATA TYPE, not by producer (the decoupling principle, applied to scoring).** The scoring rule is dispatched on the *belief's outcome space*, so Aeolus, the meteorologist persona, the synthesis Mind, and every strategy are graded by **identical rules on identical data**. This is what makes cross-producer comparison and model-swap shadow tests (I7) a *fair, paired* test — and what keeps scoring spine-decoupled (no `if producer == "aeolus"`). Scores are keyed by data columns, never code branches.
3. **Forward-only / out-of-sample, always.** Calibration, Murphy decomposition, PIT, and every gate metric are computed on **resolved beliefs the calibrator never saw** (Dawid). In-sample calibration is meaningless — isotonic/Platt make a reliability diagram diagonal *by construction*. This is the spine of the whole discipline.
4. **Calibrate before you size.** No sizing until a scope has a persisted calibration. A cold scope prices **zero** (structurally fail-closed). Sizing uses the **calibrated** probability `p_cal`, never `p_raw`.
5. **Two complementary gates.** Brier/CRPS-vs-outcome audits calibration against *truth* (slow, needs volume). CLV-vs-market audits edge against the *most-informed benchmark* (fast, per-trade). Run **both** on every resolved belief.
6. **Scores are append-only data, set once (I5).** A belief's `(status, outcome, brier, clv_bps)` are written exactly once post-resolution; a correction is a *new* superseding belief row, never a mutation. Replayable from the ledger.

---

## 2. The Scoring Taxonomy & Keying (the decoupling model — the heart)

### 2.1 The unified scoring key

Every score row — `belief_scores`, `calibration_params`, `trade_scores`, and the producer columns on `beliefs`/`scalar_beliefs` — is keyed by **DATA columns**, never literals in code:


| Dimension                       | Meaning                                       | Today                                                                                                                  | Target                                                                                                                             |
| ------------------------------- | --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `producer_id` + `producer_type` | the data source                               | **partly there:** `scalar_beliefs.producer` is ALREADY a first-class indexed column (migration 20260613000002:24, for the §9.1 scorecard); the BINARY path is the non-uniform one — Aeolus → `provenance.model_id="aeolus"`, meteorologist → `provenance.persona_id` | **uniform `provenance.producer` + a `producer_type` ∈ {Mind, MechanicalEdge, ScalarProducer, Veto}** on every belief shape (D4 extends the scalar pattern to binary) |
| `mind_id` + `mind_version`      | the decision model (for model-backed beliefs) | `model_id` is a bare literal string with no version pair — two *versions* of one mind are indistinguishable in scoring, so the shadow-swap A/B in §8 cannot be keyed | `**(mind_id, mind_version)` data columns** — beliefs reference the version, not a charter filename; null for mechanical edges      |
| `strategy_id`                   | the strategy that proposed                    | present on `trade_scores`, on the calibration `ScopeKey`                                                               | keep; every belief/order carries `strategy_id`                                                                                     |
| `category`                      | the event family                              | free string today (C3 added a controlled vocab gate for world-forward)                                                 | controlled vocabulary (data), e.g. `temperature_ny`, `funding_btc` — no hardcoded enum                                             |
| `scoring_rule`                  | which proper rule graded it                   | `belief_scores.rule_id` exists                                                                                         | keep; the rule is chosen by data type (§3), recorded as data                                                                       |


**Design decision D-1 (per-mind/producer keying):** introduce a uniform `(producer_type, producer_id, mind_id, mind_version)` quadruple as **data columns** on the belief/score rows. The spine never branches on a producer or mind name; the harness *stamps* the identity in provenance at belief-formation. This is the operator's core requirement — per-strategy *and* per-mind scoring with the spine decoupled — and it is the prerequisite for the head-to-head (Aeolus vs meteorologist vs synthesis Mind) the demo must show.

**Design decision D-2 (decouple the ledger):** remove the domain-coupled literal `provenance->>'model_id' = 'aeolus'` in `open_aeolus_weather_due` (repos.rs ~1349) — parameterize `resolved_stats(category, producer: Option<&str>)` and the due-query by producer/category **data**. (Recorded in GAPS as the ledger-decoupling follow-on; the A7 guard already excludes the ledger pending this.)

### 2.2 Producer / mind / strategy taxonomy (who produces what, scored how)

- **Deterministic forecasters** (`aeolus` — quantile temperature; `funding_forecast` — scalar rate): `producer_type = ScalarProducer`; emit SCALAR beliefs (+ Aeolus also binary bracket beliefs). No `mind_id`. Scored by CRPS/WIS (scalar) and Brier/RPS (the implied brackets).
- **Minds** (`meteorologist` persona; `synthesis` Opus): `producer_type = Mind`; carry `(mind_id, mind_version)`. The meteorologist authors binary weather beliefs *parallel to Aeolus* (D3) → scored by the **same** Brier/RPS on the **same** brackets ⇒ a fair head-to-head. Synthesis prices beliefs → scored when it authors a belief.
- **Mechanical edges** (`mech_structural` bracket arb; `perp_event_basis` basis): `producer_type = MechanicalEdge`; produce **no belief** — they have no probability to grade. Scored ONLY on realized PnL + fill-realism (`trade_scores`), never Brier. (Resolves the grounding tension "every belief is scored" vs mechanical edges: mechanical edges aren't beliefs.)
- **Veto** (`mech_extremes` favorite-longshot fade veto): `producer_type = Veto`.

**Design decision D-3 (per-strategy scoring config):** a `[scoring.<strategy>]` config section assigns the rule(s) and gates per strategy as **data** (no code branch), e.g. `binary_belief_rule = "brier"`, `also = ["log"]`, `scalar_rule = "crps"`, `requires_clv = true`. The harness reads the config; the spine stays generic.

---

## 3. The Scoring Rules — by data type (theory → choice)

The characterization (Gneiting–Raftery 2007 / Savage): every proper rule = a strictly convex entropy `G` whose Bregman divergence you penalize forecasters by. The *choice* of rule is a choice of what to punish.


| Belief data shape                                                                   | Primary rule                                                                                                               | Secondary         | Why                                                                                                                                                                                                                                                       |
| ----------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Binary** event (single Kalshi yes/no bracket, world-forward event)                | **Brier** `(p−o)²` — bounded [0,1], low-variance, **Murphy-decomposable**, robust to one fat-tail belief; the GATE metric  | **Log** `−ln p_o` | Log = the **Kelly growth objective** the sizer optimizes (expected log-wealth ≈ expected log-score; bankruptcy ↔ log-score −∞). A producer with good Brier but bad Log makes rare confident-wrong calls that blow up a leveraged book — surface that gap. |
| **Ordinal bracket ladder** (a row of Kalshi temperature brackets on one underlying) | **RPS / CRPS-via-threshold-Brier (Hersbach)** — reconstruct the implied step-CDF over thresholds, integrate Brier          | —                 | Flattening a ladder to one binary + Brier **discards ordinal structure** and gives no credit for adjacent-bracket near-misses. RPS is the proper discrete-ordinal CRPS analogue. **(Current code flattens — a real rigor gap.)**                          |
| **Scalar quantile set** (Aeolus 10/50/90; an LLM persona's quantiles)               | **CRPS via WIS** (averaged pinball over emitted levels) — proper, CRPS-consistent, degrades gracefully as the grid changes | PIT (diagnostic)  | Don't require a closed-form CDF you don't have. Units-comparable to MAE; distance-sensitive.                                                                                                                                                              |
| **Scalar closed-form** (a Gaussian/truncated-normal density)                        | **CRPS closed-form**                                                                                                       | PIT               | The proper scalar generalization (matches existing `CrpsPinballRule`).                                                                                                                                                                                    |
| **Categorical** (k≥3 unordered outcomes)                                            | **Multiclass Brier** (or **Log**); **RPS** if the categories are ORDERED                                                  | Log               | `PredictiveKind::Categorical` + `validate_categorical` ALREADY exist in `scoring.rs` (schema-representable) but have **NO scorer** — a future `impl ScoringRule`. Not on the demo critical path (no multinomial strategy ships yet) but a latent gap to note. |


**Design decision D-4 (UNIFY rule dispatch at resolution + BUILD the missing resolvers).** Correction to my first draft (adversarial review, then a second V&V pass): scoring *does* run live, but the precise current state is narrower than I first wrote. There are exactly **TWO** belief resolvers (`grep 'fn resolve_and_score'`): `resolve_and_score_weather_beliefs` (daemon.rs:4637) and `resolve_and_score_funding_beliefs` (daemon.rs:4378). The real state:
- (a) **Mixed trait usage.** The weather/binary path **hand-rolls** Brier via the free fn `score_bracket` (aeolus_resolve.rs:101, which calls `brier_score` directly, *not* the `BrierRule` trait). The funding/scalar path ALREADY dispatches the trait (`let rule = CrpsPinballRule; rule.score(...)`, daemon.rs:4401/4498). So the gap is *not* "no resolver uses the trait" — it is the un-trait'd binary path plus the fork.
- (b) **Forked per producer**, AND the weather resolver is **Aeolus-only**: its queue `open_aeolus_weather_due` hard-filters `provenance->>'model_id' = 'aeolus'` (repos.rs:1362) and guards `event_id` on the `aeolus:` prefix (daemon.rs:4723).
- (c) **Persona AND synthesis binary beliefs have NO resolver at all.** Meteorologist beliefs (event_id `{region_key}#{suffix}`, provenance `persona_id`, persona_beliefs.rs) match neither filter, so they are **never resolved or scored in production** — `resolved_persona_stats` (repos.rs:1305) selects `WHERE outcome IS NOT NULL`, but nothing live sets that outcome (only persona_e2e.rs does, by hand). **This means the §0/§2.2/§8 meteorologist-vs-Aeolus head-to-head currently accrues ZERO scored data — building persona binary resolution is a hard prerequisite, not a nicety.**

D-4 therefore = (1) a single `score_resolved_beliefs(scope)` that selects the rule by `PredictiveKind` (data), calls `rule.score(...)`, persists via `resolve_and_score` — *unifying the two existing forks*; AND (2) **adding resolution for persona + synthesis binary beliefs** (today unscored). So G2 is *unify the 2 forks AND build the missing persona/synthesis resolution* — partly a refactor, partly net-new.

**Design decision D-5 (add LogScoreRule + RPS/CRPS-ladder):** both are new `impl ScoringRule` (the extension point exists). **Do not replace Brier** — Brier stays the primary/gate; Log and RPS are additive.

---

## 4. Calibration & diagnostics (forward-only)

- **Recalibration** (exists, keep, forward-only): Platt (`calibration.rs:81`), isotonic PAV (`:190`), shrinkage-toward-market `w=min(n/50,1)` (`:260`), extremization `k` (default 1.0). Use `p_cal` downstream, persist `p_raw`.
- **Murphy decomposition (BUILD — proof gap #2):** today only a single Brier mean + a principled-but-non-Murphy 2-factor `calibration_quality` (n-ramp × reliability-gap, calibration.rs:347 — fine for the sizing haircut, but it does NOT separate resolution). Implement `BS = Reliability − Resolution + Uncertainty` per scope from the existing `CalibrationBucket` data (Reliability = Σ nₖ(p̄ₖ−ōₖ)²/N; Resolution = Σ nₖ(ōₖ−ō)²/N; Uncertainty = ō(1−ō)). **Resolution is the proof the edge isn't the base-rate-forecaster trap** — without it a single Brier number cannot prove informativeness. Persist (REL, RES, UNC) per scope; surface in the demo scorecard.
- **Sharpness subject to calibration (Gneiting–Balabdaoui–Raftery 2007):** report sharpness (forecast concentration) alongside reliability; the objective is "as sharp as possible *while* calibrated."
- **Reliability diagram (SURFACE — gap #4):** the data exists (`calibration_curve` → buckets) but `CalibrationBucket` isn't `Serialize` and no ROTA endpoint exposes it. Make it `Serialize`, add an endpoint, render binned predicted-p vs observed-freq with **per-bin sample counts + confidence bands** (thin-bin noise is real). Coordinate the contract with the parallel Operator-UI track.
- **PIT histogram (BUILD — gap #3, scalar):** for CRPS-scored scalar beliefs, bin `u = F(x_realized)`; uniform = calibrated, U-shape = under-dispersed/overconfident, hump = over-dispersed. Standard ensemble diagnostic; also the **drift input** (CUSUM on PIT-uniformity, §10). EMOS stays in the *separate* Aeolus system (FORTUNA consumes its quantiles); FORTUNA still runs PIT to *audit* them.
- **In-sample guard:** all of the above on resolved/forward data only; never on the calibrator's fit set.

---

## 5. CLV — market-relative validation (BUILD — gap #1, highest priority)

Theory: CLV measures entry vs the most-informed benchmark (the closing/pre-resolution line); it's a **fast per-trade** signal that predicts long-run edge, complementary to slow truth-scoring.

- **The gap:** CLV *compute* machinery exists — `events.rs::clv_bps` (events.rs:314-339) computes CLV in integer bps and returns `Option<i64>` — but it is **never computed live**: weather passes `None`, funding never calls it, `price_snapshots` is **never populated** (the only writer, `SnapshotsRepo::insert` at repos.rs:806, has no live caller), and the CLV/freshness machinery that consumes `benchmark_at` (`due_snapshots`, `FreshnessPolicy::assess`) is exercised only in tests. (NB: the `benchmark_at` field *itself* is live — populated by the daemon on event creation and surfaced in the ROTA discovery dashboard; it is just not yet wired into a live CLV computation.) **Storage caveat:** the `beliefs.clv_bps` SCHEMA column is `DOUBLE PRECISION`/f64 (migration 20260609000001:73), persisted as `Option<f64>` via `resolve_and_score` (repos.rs:1219) — the integer `i64` is the in-flight compute type, widened to f64 at the storage boundary. The fast gate is dark.

**The design principle (operator-directed): DECOUPLE CAPTURE FROM COMPUTATION.** Capture a *rich, generous* price time-series per tracked market; *choose* the benchmark snapshot at scoring time. We would rather have too much (correct, dense) data than too little and miss the benchmark. This makes CLV **robust** (never missing the snapshot we need), **dynamic** (the capture cadence adapts), and **revisable** (the "what counts as the closing line" policy can change without re-capturing).

### 5a — The CLV capturer (writes `price_snapshots`; append-only, restart-safe, liquidity-gated)
Capture is keyed on the **MARKET**, not the belief — which directly handles the belief-precedes-market case: *a belief can exist for days before a market is tagged to it; the capturer starts the instant a market is mapped, and a snapshot is taken AT THAT TAGGING moment* (the earliest price we could have traded). Triggers (capture on ANY of these, generously):
- **Market-tag / discovery** — when a market is first mapped to an event/belief (the user's case): snapshot the initial price + mark `first_seen`.
- **Every FORTUNA order** — the entry reference for CLV (the `at` of the fill).
- **Adaptive cadence while live + liquid** — denser as horizon approaches (e.g. ~hourly far out → ~minutely in the final window), sparser in the quiet middle; AND on a meaningful price move (snapshot-on-change, not just on a clock) so we never miss a regime shift.
- **Horizon-relative marks** — explicit captures at the candidate "closing line" offsets (T-24h, T-6h, T-1h, T-5m, T-1m before the event horizon).
- **Resolution** — a marker snapshot (NOT a CLV benchmark — settlement converges to ~$0.99/$0.01 = fake skill).

Each `price_snapshots` row carries: `(market_id, snapshot_at, yes_bid, yes_ask, mid, last_trade, depth/spread, source, trigger)`. **Liquidity-gated:** only persist a snapshot from a two-sided / liquid book (an empty or one-sided book is recorded as a `degraded`/skipped marker, not a fake price) — and the spread/depth metadata lets the benchmark selector PREFER liquid snapshots. **Append-only (I5), idempotent** on `(market_id, snapshot_at)`; a persisted cursor makes it restart-safe (the daemon already polls books every segment — sample from that stream; do NOT add a second poller).
**Retention (manage volume AFTER the data is safe):** keep ALL snapshots until the event resolves AND CLV is scored; THEN prune the long-tail to the benchmark-relevant set (entry, the selected closing line(s), the near-horizon marks, first/last-liquid) for the permanent record. Capture-generously, prune-conservatively — never prune before CLV is computed.

### 5b — The benchmark-selection policy (at scoring time; CONFIG)
CLV is computed at resolution by SELECTING a benchmark from the rich series, per a **config policy** (decoupling: the policy is data, not a literal): default = *the last LIQUID snapshot at or before a configured pre-horizon offset* (e.g. T-1h), with fallbacks (nearest liquid within a window) if that mark is missing. **REUSE, don't rebuild:** this selection algorithm ALREADY EXISTS and is tested — `events.rs::clv_bps` (events.rs:310-339) already takes the snapshot series, filters to liquid snapshots strictly before `benchmark_at`, and `max_by_key`s on time to pick the last-liquid-pre-benchmark mark, returning integer bps; and `SnapshotsRepo::latest_liquid_before` (repos.rs ~831) already implements the DB-side benchmark query. So the genuine work is the **capture side** (wire a live writer of the existing `SnapshotsRepo::insert`; populate `price_snapshots`) plus **calling the existing `clv_bps` in the live resolver** — not a new selection API. `clv_bps = devig(benchmark) − devig(entry)`; the *compute* is integer bps (`i64`, keeping price math off `f64`), but note it is then persisted into the `beliefs.clv_bps` column, which is `DOUBLE PRECISION`/f64 today (§5 caveat) — widened at the storage boundary, with any further `f64` confined to the analytics/reliability layer. (If end-to-end integer money is the goal, migrating `beliefs.clv_bps`/`brier` to `BIGINT` + an `Option<i64>` `resolve_and_score` signature is an explicit follow-on, not current state.) The live resolution path computes CLV for every TRADED belief and persists via `resolve_and_score`.

### 5c — Rigor (theory mistakes to avoid)
- **Pre-benchmark liquid snapshot, NOT settlement** (settlement = fake skill).
- **De-vig both sides on the same method.** NOTE for Kalshi: a single binary contract's yes/no are complementary on one book; "vig" appears as the bid/ask spread and the yes+no overround — de-vig by normalizing the two-sided mid (or Shin/power where favorite-longshot bias matters). Qualify per venue; don't blindly apply sportsbook two-way de-vig to a single Kalshi contract.
- Treat positive CLV as an **estimator**, not a guarantee; require a *trend* over a rolling window (§8), not a single positive reading.

---

## 6. Sizing — calibrate-before-size (mostly done; formalize)

The standing chain (implemented, B1/B3): cold scope ⇒ Mind call skipped ⇒ zero size; `kelly_binary` (fortuna-state `sizing.rs:31-51`) ALREADY computes `f_kelly = (p·100−c)/(100−c) = (p−q)/(1−q)` correctly, clamps to [0,1], then multiplies by the haircut `fraction`; the haircut itself is `haircut_kelly_fraction(base, calibration_quality) = base × quality` (cognition `cycle.rs:196`, NaN→0 fail-closed); shrinkage below n=50. The Kelly↔calibration safety chain is the *one fully-rigorous piece* and it holds. **So D-6 is mostly a re-statement of shipped code — the ONLY genuinely new factor is `k_unc`** (and the discipline of feeding `p_cal`, not `p_raw`).

**Design decision D-6 (formalize the sizing formula — makes spec 5.14's open question concrete):**

```
stake = k_base · h_cal(category, producer) · k_unc(n, σ²) · f_kelly(p_cal, q)
```

- `f_kelly = (p_cal − q)/(1 − q)` for a binary Kalshi contract — `**p_cal` (calibrated), never `p_raw**` (raw inflates the edge by exactly the calibration bias → oversizes).
- `k_base ∈ [0.25, 0.5]` standing fractional-Kelly (start 0.25).
- `h_cal ∈ (0,1]` calibration-quality haircut (exists).
- `k_unc ∈ (0,1]` **estimation-uncertainty shrinkage → 1 as resolved-count n grows** (Baker–McHale 2013 / Chu–Wu–Swartz). **Result: under a parameter posterior with variance σ²>0, posterior-expected log-growth is maximized at a fraction strictly BELOW the plug-in full-Kelly fraction `f*(p̂)` — i.e. full (plug-in) Kelly is *suboptimal in expected log-growth*, so shrink below it.** (Not "strictly dominated" in the decision-theoretic sense — full Kelly still wins in favorable realizations; the correct claim is suboptimality in expectation, the concave-objective shrinkage result.) `k_unc` is the formal "humility discount" the spec hand-waves; make it a diagnosable factor. (New work; small.)

---

## 7. The learning loop (closed vs open)

```
signal → belief(p_raw) → [calibrate→p_cal] → [size: Kelly×quality, cold⇒0] → order → gate → fill
        → settlement → realized PnL + trade_score
belief  → (post-resolution) → proper score (Brier/RPS/CRPS) + CLV → calibration re-fit (forward, n≥50, versioned)
        → re-deliver calibration to sizing → promote/demote
```

- **CLOSED by Phase C:** belief persist; fill persist w/ strategy (A2); settlement→realized-PnL, DB-as-truth (A3); trade scoring (A4); funding window-dedup (A5); recording replay (A6); **calibration persisted, count-triggered, paper-only** (B1); **calibration delivered to synthesis per segment, cold-start gated** (B3); per-producer authorship (D3).
- **OPEN (this doc's targets):** the **live ScoringRule dispatch + persona/synthesis resolution** (D-4), **CLV-live** (§5), the **Murphy/PIT/reliability** diagnostics (§4), **per-mind keying** (D-1), **RPS for ladders + Log score** (D-5), and **automated demotion** (§10). Feedback latency: weather ~daily, funding ~8h → both reach the **calibration-autonomy ramp `n=50` (FULL_AUTONOMY_N)** in ~1–2 weeks (the demo window). **Be honest about what the demo window proves:** `n=50` is the calibration ramp, NOT the synthesis GO bar — that bar is `min_resolved_beliefs_synthesis` (shipped 100 / spec 60), which at ~daily weather cadence is ~60–100 days. So the demo window proves *calibration autonomy + accruing scored data*, not a clearable synthesis GO verdict.

---

## 8. The Demo Scorecard (what "it works" means — concrete)

The demo is **demo-paper-ready** and a strategy is GO-eligible when, **per (strategy, category, producer/mind) scope** and **forward/out-of-sample**:


| Gate             | Threshold (v1; tune from spec 5.8)                                              | Rule                              |
| ---------------- | ------------------------------------------------------------------------------- | --------------------------------- |
| Volume           | `n ≥` the spec §11 threshold. The shipped `go_nogo` field is `min_resolved_beliefs_synthesis` (review.rs:148; **shipped config 100**, config/fortuna.example.toml:93) — spec §11 mandates **≥ 60 resolved beliefs for synthesis** (spec.md:384), so config (100) is *stricter* than spec. `FULL_AUTONOMY_N=50` is the calibration-autonomy ramp, NOT the GO bar. Mechanical: `min_paper_days_mechanical` — **spec §11 mandates ≥ 30 trading days (spec.md:384); the shipped config sets 14 (fortuna.example.toml:92) — a config-vs-spec divergence, recorded in GAPS.md.** | thin-data guard |
| Calibration      | Reliability error low **AND Resolution > 0** (Murphy)                           | proves informative, not base-rate |
| Edge (spec §11 semantics) | **positive CLV (prediction markets) OR positive expectancy net of modeled fees**, AND **(for synthesis / belief-producing scopes only)** Brier beating the market-implied baseline on the strategy's categories (spec.md:384 scopes the Brier clause to synthesis strategies; mechanical edges are never Brier-graded, §2.2) | proper + market-relative |
| Economics        | **fee/PnL ratio < 0.35** (spec.md:384) — note the shipped config ships `max_fee_pnl_ratio = 0.5` (fortuna.example.toml:94), a config-vs-spec divergence flagged in GAPS.md; fill-realism (maker/through-not-touch) | not a fee trap                    |
| Selection honesty | multiple-testing correction across the scopes/producers tried, applied to the CALIBRATION + CLV gates (**not** a PnL/Sharpe gate — legitimacy is calibration+CLV; a deflated-Sharpe-style returns control belongs to a future trade-validation layer) | not luck-from-many-tries |


**Reuse, but EXTEND — the shipped verdict is only half the gate:** the GO/NO-GO verdict is partly computed by `go_nogo`/`weekly_review` (review.rs:189-308) returning Go/NoGo/InsufficientData + reasons — but the shipped logic gates ONLY on volume (`min_paper_days_mechanical` / `min_resolved_beliefs_synthesis`), CLV > 0, net-expectancy > 0, and fee/PnL ratio. **It contains NO Brier-beats-baseline gate and NO Murphy-Resolution gate** (grep review.rs:189-262 for brier/calibrat/resolution → nothing). So the §8 Calibration row ("Resolution > 0") and Edge row ("Brier beating the baseline") are NOT yet in the verdict — surfacing the existing verdict would silently omit the entire calibration/Brier half of spec §11 (the half this whole doc exists to make rigorous). The architecture's job is therefore (a) make the gate inputs (CLV, Murphy, per-mind keying) live + correct, (b) **extend `go_nogo` itself with the Brier-beats-baseline and Resolution > 0 gates** (part of G3, not merely "make inputs live"), and (c) surface the extended verdict. Thresholds must match **spec Section 11** (spec.md:384), not a new invented bar.

The operator dashboard surfaces, **per scope/producer/mind**: the reliability diagram (w/ bands), the Murphy split (REL/RES/UNC), rolling Brier + Log + CRPS, rolling CLV, sharpness, n, and the GO/NO-GO verdict with reasons. The Aeolus-vs-meteorologist-vs-synthesis head-to-head BECOMES a fair paired comparison once the beliefs are scored by the **same data-type-dispatched rule on the same brackets** (§3) — which is the TARGET, FURTHER from today's state than a refactor: today there are TWO resolvers (`resolve_and_score_weather_beliefs`, Aeolus-only; `resolve_and_score_funding_beliefs`, scalar) and **persona/synthesis binary beliefs are never resolved or scored at all** (D-4). So the head-to-head has **zero scored meteorologist data today** — building persona binary resolution is the binding prerequisite, then the unified data-keyed dispatch (D-4).

---

## 9. Invariant & decoupling guarantees for scoring

- **I5 (append-only, set-once) — two distinct mechanisms, do not conflate:**
  - **Scalar** (`scalar_beliefs` + `belief_scores`): DB-enforced. `scalar_beliefs_guard` permits ONLY the NULL→value resolution transition; `belief_scores` is INSERT-only with `UNIQUE(belief_id, rule_id)` giving crash-safe idempotency; a correction/re-score is a new superseding row. (Adversarial review: this is the *scalar* model.)
  - **Binary** (`beliefs.brier`/`clv_bps` written in-place by `resolve_and_score`): set-once is enforced at the **APPLICATION layer** by the WHERE clause (`outcome IS NULL AND status IN ('open','superseded')`, repos.rs:1224) + the `rows_affected()!=1` guard — NOT by `fortuna_beliefs_guard`, which only freezes *content* columns and refuses DELETE. A raw `UPDATE ... WHERE outcome IS NOT NULL` would bypass the trigger. **Action:** harden the binary `beliefs` guard to DB-enforce scoring-column set-once (match the scalar guard). The spec's own I5 text (spec.md:44) inherits this same gap.
- **I6 (propose-only):** the **model** emits no order/size/price (beliefs flow on `drain_beliefs`, a channel separate from proposals). The **harness** scores and sizes; it MAY derive score columns (e.g. `clv_bps` from the fill price — events.rs:314) — that is I6-safe because the model never sees or emits them.
- **I7 (promotion/demotion):** no programmatic self-*promotion* (operator action; i7_promotion_gates.rs). **Demotion is different:** the spec (Section 11, spec.md:381/386) defines it as an *automatic STAGE step-down on breach* and the i7 test BLESSES a system-actor stage transition (`record(Paper→Sim, "system")`). So the watchdog's §10 "demote" rung = **emit a demotion record stepping the strategy's STAGE down** (the I7-governed retreat). This is mechanically distinct from the §10 *sizing-knob* rungs (lower `h_cal`, widen shrinkage) which are I7-NEUTRAL calibration-driven sizing already shipped in B1/B3 — keep them separate. (No strategy-envelope/stage-demotion machinery exists today — only lesson-decay (review.rs:447) and source-registry demotion (repos.rs:972); this is new.) I2: a drawdown HALT stays human-rearm — the watchdog never auto-resumes.
- **I4 (kill-switch independence):** verifiable, not aspirational — `i4_killswitch_independence.rs` asserts (from `cargo metadata`) that `fortuna-killswitch`'s dep graph contains no Postgres/ledger/cognition. A watchdog placed in cognition/ops therefore *cannot* enter the killswitch graph without RED-ing that test.
- **Decoupling guard (scope it precisely):** the A7 guard (`i_decoupling_spine.rs`) scans ONLY `fortuna-gates`/`-exec`/`-state` for bare domain literals and EXCLUDES `fortuna-live` (composition root) + `fortuna-cognition` (legitimately holds producer logic, incl. `scoring.rs`). So "extend the scan to the scoring spine" has no clean target — the scoring rules live in cognition, dispatch in fortuna-live, both excluded by design. Instead: enforce decoupling by (a) keeping the `ScoringRule` dispatch keyed on `PredictiveKind` (data), and (b) a targeted test that the ledger's `resolved_stats`/due-queries carry NO producer literal once D-2 lands (close the `'aeolus'` literal). Do NOT claim a blanket scoring-spine scan.
- **New invariant — ASPIRATIONAL today (be honest):** *every sized order traces to a scored, calibrated belief or a declared mechanical edge.* This is NOT currently enforceable: there is no `belief_id` on the order path (Proposal/CandidateOrder/GatedOrder carry no belief reference — grep finds none in fortuna-gates/exec; beliefs flow on a separate channel). Making it structural requires threading a belief reference (or explicit mechanical-edge marker) onto Proposal→intent→order — a non-trivial spine+schema change this doc scopes as future work, not a free audit.

---

## 10. The Edge Decay Watchdog (config-named; §C of the prior brief, folded in)

The **reverse of the I7 promotion gate**: continuously re-validate each scope's edge is still **live** (CLV trend) and **calibrated** (Brier/PIT trend), and *defend* when it decays. The name + thresholds are **config** (`[edge_decay_watchdog] name = "..."`, `enabled=false` default), no literals in the spine.

- **Input stream:** the per-belief residuals already produced — Brier residual `(p−o)²`, log residual, CRPS, PIT `u` — keyed by `(producer, strategy, mind, resolved_at)`. **Never keys on PnL** (legitimacy = calibration+CLV).
- **Detection (thin-data-robust):** CUSUM / Page-Hinkley / ADWIN on the residual + CLV streams; PIT-uniformity drift (creeping U = dispersion drift, creeping skew = bias drift); change-point on rolling reliability/resolution. Calibrate the alarm rate against the thin-data noise floor.
- **Graded defense ladder** — two mechanically distinct tiers (do NOT conflate, per the review):
  - **Sizing responses (I7-NEUTRAL — calibration-driven, already shipped):** (1) re-fit calibration NOW, don't wait for the cadence (B1); (2) widen shrinkage-toward-market / lower `h_cal` (B3). These reduce *size* via the existing calibration→Kelly path; they are NOT a capital-stage change and need no I7 record.
  - **Stage demotion (I7-GOVERNED):** (3) emit a **demotion record stepping the strategy's STAGE down** — this is the spec's "automatic on breach" stage step-down (spec.md:381/386), which the i7 test already blesses for a system actor (`record(Paper→Sim, "system")`); (4) shadow-swap to a challenger (`shadow.rs`); (5) operator NO-GO alert. (No strategy-stage-demotion machinery exists yet — this is new, distinct from lesson-decay/source-registry demotion.)
- **Constitution:** the sizing rungs are calibration-driven (no I7 record); the stage-demotion rung emits the spec's stage step-down (I7 permits a *system*-actor demotion — the asymmetry with promotion, which stays operator-only); a HALT stays human-rearm (I2 — the watchdog never auto-resumes); findings append-only (I5); deterministic from the injected Clock + persisted scores; placement in cognition/ops keeps it OUT of the killswitch dep graph (I4, `i4_killswitch_independence.rs`).

---

## 11. Gap analysis & phased plan (prioritized for the demo + north star)


| #   | Gap                                                                             | Priority | Why                                                                                   | Rough scope                                                                                                      |
| --- | ------------------------------------------------------------------------------- | -------- | ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| G1  | **CLV not live** (snapshots unpopulated; weather/funding pass None)             | **P1**   | the FAST gate + a spec GO criterion; the demo is half-blind without it                | snapshot schedule + `clv_vs_entry` + wire into resolution; de-vig                                                |
| G2  | **Unify ScoringRule dispatch + BUILD persona/synthesis resolution (D-4)**       | **P1**   | the meteorologist head-to-head has ZERO scored data today (persona binary beliefs are never resolved); unifying alone is insufficient | unify the 2 forks (`_weather_`, `_funding_`) → one `PredictiveKind`-dispatched `rule.score(...)` AND add resolution for persona + synthesis binary beliefs |
| G3  | **Murphy decomposition + reliability-diagram serialization**                    | **P2**   | proves *resolution* (informative, not base-rate); makes calibration *visible*         | compute REL/RES/UNC; `Serialize` + ROTA endpoint                                                                 |
| G4  | **Per-mind/producer keying (D-1) + ledger decouple (D-2)**                      | **P2**   | the operator's core ask; enables the fair head-to-head; closes the `'aeolus'` literal | normalize `provenance.producer` (D4 starts) + `(mind_id, mind_version)` columns + parameterized `resolved_stats` |
| G5  | **RPS/CRPS for bracket ladders + LogScoreRule (D-5)**                           | **P3**   | ordinal near-miss credit; Log = the ruin/Kelly objective                              | 2 new `impl ScoringRule` (additive, Brier stays gate)                                                            |
| G6  | **PIT histogram (scalar)**                                                      | **P3**   | diagnose Aeolus quantile dispersion; the watchdog's drift input                       | bin `F(x_realized)`; surface                                                                                     |
| G7  | `**k_unc` estimation-uncertainty shrinkage (D-6)**                              | **P3**   | full plug-in Kelly is suboptimal in expected log-growth; formalize the humility discount | one factor in the sizing formula                                                                                 |
| G8  | **Edge Decay Watchdog (§10)**                                                   | **P4**   | defends the edge over time; needs G1–G6 as inputs first                               | config-named subsystem; build after the proof layer                                                              |
| —   | Persistence/loop (settlement→PnL, calibration-persist, per-producer authorship) | **DONE** | —                                                                                     | Phase C A1–A7, B1–B3, D1–D3                                                                                      |


**Sequencing recommendation:** G1+G2 (make scoring real + the fast gate live) → G3+G4 (make it provable + decoupled per-mind) → G5+G6+G7 (rigor + diagnostics) → G8 (the watchdog). G1–G4 are what make the *demo's data trustworthy*; G5–G8 harden it toward the north star.

---

## 12. Open questions for review

1. Demotion trigger ownership: scoring job vs ROTA vs a standalone watchdog — who computes the rolling metric and *writes* the demotion record? (§10 assumes the watchdog; confirm against spec 5.8/Section 11.)
2. Bracket-ladder identity: how does the harness know which beliefs form one ordinal ladder (for RPS) vs independent binaries? (event/category grouping — needs a `ladder_id` or derivation rule.)
3. CLV benchmark choice per venue (Kalshi has no true "closing line" — define the pre-resolution liquid snapshot window precisely).
4. `k_unc` functional form + how σ² is estimated from the resolved sample.
5. Should the demo scorecard's GO thresholds be config (per the decoupling principle) or spec-fixed?

---

*This doc supersedes the narrow `docs/research/2026-06-18-scoring-learning-loop-edge-decay-watchdog-brief.md` (the watchdog is now §10). Next: adversarial review (invariant-safety, decoupling-as-data, scoring rigor, implementability), then fold G1–G4 into the Phase-C-tail / a scoring-hardening track per operator decision.*