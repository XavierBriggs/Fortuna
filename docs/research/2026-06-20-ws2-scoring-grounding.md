# WS2 scoring math — deep-research grounding

**Date:** 2026-06-20 · **Audience:** the FORTUNA WS2 "proof layer" build
(`docs/superpowers/specs/2026-06-20-ws2-proof-layer-design.md`).
**Method:** orchestrator + 4 parallel research subagents, each reading primary sources
(proper-scoring-rule + forecast-verification + quant-finance literature). Every load-bearing
claim traces to a fetched canonical paper; confidence is labeled.

---

## Summary (the verdict)

Our WS2 math is **mostly correct**, with **four correctness refinements** and **one
structural upgrade** that the literature strongly favors and that doubles as the "edge":

- **Correct as specced:** Brier `(p−o)²` (binary, [0,1]); RPS `Σ(P_i−O_i)²` (proper for ordinal,
  reduces to Brier at K=2); PIT `u=F(x)` for a *continuous* scalar producer; CLV via de-vigged
  closing price (industry gold-standard skill signal).
- **Refine:** (1) the **binned Murphy decomposition is a biased estimator** at our sample sizes —
  switch to **CORP/isotonic** (the modern standard, and our biggest edge); (2) **Log-score ε-clamping
  breaks strict propriety** — keep it but also count `p<ε` events; (3) verify the **existing
  CRPS** carries the **factor-of-2 + Δτ** weighting; (4) a **citation fix** (binning bias is
  Ferro–Fricker 2012, not Bröcker 2009).
- **Adopt as edge (WS2-scoped):** **CORP reliability + MCB−DSC+UNC decomposition** (replaces binned
  Murphy), **Murphy diagrams + forecast dominance** (model-vs-model, the I7 shadow check), and the
  **Diebold–Mariano test** (does the model beat the baseline *significantly* on the forward window).
- **Defer to WS3 (validated):** **Deflated Sharpe Ratio** and **PBO/CSCV** are backtest/selection
  instruments — they need a trial count N / a T×N config matrix and are **meaningless on a single
  live-forward window**. Our spec's deferral of PBO/deflated to WS3 is exactly right. Cost-loss
  economic value defers too (needs the execution-cost model).

The headline: **a Brier-only gate conflates calibration, discrimination, and intrinsic uncertainty,
and any binned diagnostic of *why* a forecaster scores well is biased + unstable. CORP fixes both for
the same Brier number we already compute** — it is a strict superset of the current gate.

---

## §1 Correctness — per metric (with spec corrections)

### Brier — CORRECT
`(p−o)²` (binary) is the modern [0,1] convention; Brier's 1950 two-class original is `2(p−o)²`
([0,2]). Our spec uses `(p−o)²` ✓. Strictly proper. *Pin one orientation (loss, lower-better) across
Brier+Log+RPS or you get silent decomposition bugs.* (Brier 1950, Mon. Wea. Rev. 78:1; Gneiting &
Raftery 2007, JASA.) **Confidence: high.**

### Murphy decomposition (REL−RES+UNC) — CORRECT IDENTITY, BIASED ESTIMATOR ⚠
Our per-bin formulas + the identity `REL − RES + UNC = Brier` are exactly right *as an algebraic
identity on the binned empirical values* (Murphy 1973). **But the binned REL and RES are biased
estimators of their true (population) values, and the bias is material at our sample sizes:**
- **Reliability is over-estimated, uncertainty under-estimated, resolution either way** — the bias
  grows with bin count K and with small per-bin counts (Ferro & Fricker 2012, read in full;
  abstract verbatim). They report `E(REL) ≥ 5×REL∞ when n < 40`, and that the **bias-corrected**
  REL′/RES′ are "negligible only when n > ~60; the naive REL/RES only once n > 300."
- The §7 worked example uses **n≈88** → squarely in the biased regime. A naive binned Murphy on the
  demo would **overstate miscalibration** of a genuinely-calibrated producer.
- **Fixes:** (a) Ferro–Fricker 2012 bias-corrected `REL′/RES′/UNC′` (UNC′ unbiased; clip to keep the
  sum); or **(b) CORP/isotonic** — binning-free, the modern standard (see §3.1). **We recommend (b).**
- **Citation correction:** the binning-bias result is **Ferro & Fricker 2012** (QJRMS) + **Bröcker
  2011** (Climate Dynamics) — **NOT Bröcker 2009** (which is the *population-level* generalization of
  the decomposition to all proper scores, with no binning-bias content). Our spec/plan paired it with
  "Bröcker"; correct the attribution. **Confidence: high (Ferro–Fricker read verbatim).**

### RPS — CORRECT
`RPS = Σ_{i=1}^{K−1}(P_i−O_i)²` (cumulative-prob squared distance), proper for *ordered* categories,
reduces exactly to Brier at K=2. Normalization `1/(K−1)` is the common [0,1] variant — "K−1" is a
**divisor, never a multiplier**. (Epstein 1969; Murphy 1971.) Consistent with our S2 resolution
(weather ladders are binary-per-threshold → `RpsRule` fires for Categorical producers like funding).
**Confidence: high.**

### Log score — CORRECT FORMULA, ε-CLAMP IS A PROPRIETY HACK ⚠
`−ln p_realized`; strictly proper and the *unique local* rule (Gneiting & Raftery 2007). It is
**unbounded**: `p→0` on a realized event ⇒ `+∞`. **ε-clamping `p∈[ε,1−ε]` breaks strict propriety**
(it flattens the gradient near the boundary, so honest reporting is no longer the unique optimum) and
**ε becomes a free parameter that materially changes scores** — a single 0-prob hit contributes `−ln ε`
(≈34.5 at ε=1e-15), so model rankings can hinge on ε. **Recommendation:** keep a fixed, documented ε
(our 1e-15) so the score stays finite, **but ALSO record the count of `p_realized < ε` events** — that
count, not the clamped log-mean, is the real signal of overconfident tail calls. (Alternative:
exclude-and-count.) **Confidence: high on formula/unboundedness; the "clamp breaks strict propriety"
is a standard derived result, not a single-sentence quote — but uncontroversial.**

### CRPS (existing code) — VERIFY THE FACTOR-OF-2 + Δτ ⚠
Not new in WS2, but the research flags a correctness check on the existing `CrpsPinballRule`:
**`CRPS = 2∫₀¹ pinball_τ dτ`**, and the correct discrete estimator is the Riemann sum
`2·Σ_τ pinball_τ·Δτ`; for **equally-spaced** levels (Δτ=1/K) this is `(2/K)Σ pinball` = **2 × mean
pinball loss**. Two traps: (i) "mean pinball loss" without the **factor 2** is `½·CRPS`; (ii) for
**unevenly-spaced** quantiles you must weight by Δτ or it's biased. *Action:* confirm the existing rule
either includes the factor-2 + Δτ or documents its convention. (Gneiting & Raftery 2007; Tibshirani et
al. 2022 Eq 6, read verbatim; Gneiting 2011.) **Confidence: high.**

### PIT — CORRECT for continuous; RANDOMIZE for discrete ⚠
`u=F(x_realized)`; **uniform PIT ⇔ probabilistic calibration** (necessary, not sufficient). U-shape ⇒
under-dispersed/overconfident; ∩ ⇒ over-dispersed; slope ⇒ biased. Our S4 targets the **scalar Aeolus
envelope (continuous)** → plain PIT is correct. **Caveat to record:** for any **discrete** predictive
distribution, plain PIT is *not* uniform even for the ideal forecast — use the **randomized PIT**
`u=F(x−1)+v·(F(x)−F(x−1))`, `v∼U(0,1)`, or the non-randomized Czado–Gneiting–Held mean-PIT (preferred
for a deterministic audit trail). Not needed for the continuous demo producer; note it for future
discrete producers. (Gneiting–Balabdaoui–Raftery 2007; Czado–Gneiting–Held 2009.) **Confidence: high.**

### Reliability diagram — BINNED IS UNSTABLE ⚠
Binned predicted-p vs observed-freq is **arbitrary + unstable** (depends on bin count/edges; sampling
noise makes calibrated forecasts look off) — same root cause as the Murphy binning bias. **Fix = CORP
isotonic reliability diagram with consistency bands** (§3.1). **Confidence: high.**

---

## §2 Industry practice — what quants actually use

- **Sports betting → CLV (Closing Line Value).** Beat the **de-vigged closing price** (de-vig by
  normalization `p̂ᵢ = pᵢ/Σpⱼ`). The closing line aggregates all late/sharp info, so it's the best
  available true-probability proxy; beating it = systematic +EV. CLV is **leading + lower-variance**
  than realized PnL (you can be +EV and lose) — a skill read in *days* not months. Validates CLV as
  FORTUNA's market-edge signal. (Pinnacle; Boyd's Bets; Buchdahl via Pinnacle Odds Dropper —
  *reputable/practitioner*; academic closing-line-efficiency is mixed — favorite-longshot bias exists,
  so CLV is an excellent *relative* signal, **not** a substitute for a calibration gate.)
- **Weather/epidemic → CRPS, and WIS for quantile forecasts.** The **Weighted Interval Score** (US/Euro
  COVID Forecast Hub; Bracher et al. 2021, PLoS Comp Biol, primary) is a proper score that
  **approximates CRPS** from quantile/interval submissions, **decomposes into sharpness +
  over-prediction + under-prediction**, and (unlike log) never diverges. Operational config: median +
  K=11 central intervals. **Candidate** for FORTUNA's interval/quantile producers (the Aeolus envelope)
  — a CRPS-consistent decomposition for free. (Optional WS2; strong for WS3 over real history.)
- **Prediction markets (Kalshi/Polymarket) → Brier + log-loss + calibration curve.** Prices graded as
  forecasts; markets sharpen toward resolution (the CLV analog). The one peer-reviewed-context study
  (Clinton & Huang, 2024 election, ~2,500 markets) stresses **miscalibration/herding** over raw
  accuracy — i.e. don't assume market prices are perfectly calibrated. (SEO "Brier ~0.08" numbers are
  *weak*, unverified.)
- **The overfitting toolkit + the BOUNDARY that matters for FORTUNA:**
  - **Diebold–Mariano** test — compares two forecasts' accuracy via the loss-differential
    (`H₀: E[L(e₁)−L(e₂)]=0`, HAC std error). **Works on a single forward window.** → **ADOPT in WS2.**
    Caveat (Diebold himself): it compares *forecasts*, not *models*; use HAC + check stationarity.
  - **Deflated Sharpe Ratio** — corrects multiple-testing + non-normality; needs the **trial count N**,
    skew, kurtosis, T. **Needs a backtest.** → WS3.
  - **PBO / CSCV** — P[in-sample-best ranks below OOS median]; needs the **T×N config matrix**.
    **Needs a backtest.** → WS3.
  - **This exactly validates our spec:** on the single live-forward window WS2 reports proper scores +
    CLV + (now) DM; PBO/DSR are deferred to WS3 where window/threshold selection actually happens.
    Reporting a backtest number without N is the canonical overfitting abuse DSR exists to stop.

---

## §3 Edge methods — sound, adoptable advantages over a Brier-only gate

### 3.1 CORP reliability + MCB−DSC+UNC decomposition — ADOPT NOW (top priority)
Dimitriadis, Gneiting & Jordan 2021 (**PNAS**, arXiv 2008.03033). Replace bin-and-count with
**nonparametric isotonic regression (Pool-Adjacent-Violators / PAV)**. CORP = Consistent, Optimally
binned, Reproducible, PAV-based. It yields, for the **same Brier score we already gate on**, a
binning-free decomposition:
> **S̄ = MCB − DSC + UNC** (all three guaranteed ≥ 0)
- **MCB** (miscalibration) = score lost to being miscalibrated (recoverable by recalibration) — the
  "is the model honest" number.
- **DSC** (discrimination) = score gained from actually separating outcomes — the "does the model know
  anything" number a Brier-only gate **cannot isolate**.
- **UNC** (uncertainty) = base-rate difficulty; identical across producers → makes cross-period Brier
  comparisons fair.
Plus **consistency/uncertainty bands** (resampling or asymptotic) → "is this deviation real or noise."
**This is the modern replacement for the binned Murphy decomposition AND the binned reliability
diagram, in one.** Cost: low — PAV is O(n log n), standard; port it + compute three mean-score terms.
**Soundness caveat (must):** the MCB used as a *gate threshold* must be **cross-fit / out-of-sample**
(recalibration fit on held-out data), else it's optimistically low. The diagram in-sample is fine as a
*diagnostic*. **Confidence: high** (PNAS, cross-confirmed by two subagents; sign `MCB−DSC+UNC`
verified — one web summary had it wrong as "+", corrected).

### 3.2 Murphy diagrams + forecast dominance — ADOPT NOW (cheap, decisive for I7)
Ehm, Gneiting, Jordan & Krüger 2016 (**JRSS-B**, arXiv 1503.08195). Every consistent scoring function
is a mixture of **elementary** scores `S_θ` indexed by a threshold θ; plotting mean `S_θ` vs θ shows
performance across *all* reasonable scoring rules at once. **If forecaster A's Murphy curve lies below
B's for all θ, A beats B under every consistent scoring rule** (robust to the Brier-vs-other choice).
Where curves cross, A is better for some decision thresholds, B for others — the trading-relevant "where
in probability space is my edge." This is the right **model-vs-model promotion check (spec I7 shadow
comparison)** — it doesn't hinge on Brier being the "right" loss. Cost: low (grid of θ, two curves,
bootstrap the difference). Ships as the third "Triptych" panel with CORP. **Confidence: high.**

### 3.3 Isotonic/Platt/beta recalibration — ADOPT for diagnosis (transform later)
Post-hoc calibration map; **same PAV machinery as CORP** (CORP's curve *is* the isotonic recalibration
map; MCB *is* the score it would recover). Sample-size rule: isotonic ≥ Platt only above ~1–2k points;
below that prefer **beta calibration** (Kull et al. 2017 — 3-param, can fit the identity so it won't
de-calibrate good probabilities) for thin Kalshi history. **Propriety pitfall:** fit on a *separate*
fold or you understate miscalibration (same cross-fit rule as 3.1). Use now as a diagnostic + an
optional **pre-trade probability transform**; not a gate change. **Confidence: high.**

### 3.4 Cost-loss / economic value envelope — DEFER (needs the cost model)
Murphy 1977 / Richardson 2000: score a forecast by its **value to a decision-maker** across cost/loss
ratios C/L. Value peaks at C/L = base rate (= the Hanssen–Kuipers skill score); a probabilistic
forecast's value is the **envelope** of per-threshold value curves (≈ the ROC re-expressed in economic
units) — the principled bridge from "calibrated" to "dollars." Needs FORTUNA's execution-cost model
(fees/spread/adverse-selection), so it lands once the paper loop + cost model exist. **Confidence:
high on the math; adopt-later on timing.**

### 3.5 CLV as a parallel market-edge gate — ADOPT NOW (validate the folklore)
Use CLV as the principal **market-edge** metric *alongside* (not replacing) the calibration gate — they
measure different things (price-edge vs probabilistic-accuracy; a model can have +CLV yet be
miscalibrated). The strong "positive CLV ⇒ almost universally profitable" claim is
**practitioner-grade, not peer-reviewed** — adopt the concept, but **validate it on our own data** rather
than inheriting the number. **Confidence: medium (mechanism sound; the profit-claim is folklore).**

---

## §4 ADOPT-in-WS2 vs DEFER-to-WS3

| Capability | Verdict | Note |
|---|---|---|
| Brier (GO gate), strict `<` | ADOPT (unchanged) | the gate stays Brier-beats-baseline |
| Log score + **count of `p<ε` hits** | ADOPT (refined) | clamp finite + report the tail-call count |
| RPS (Categorical producers) | ADOPT (unchanged) | weather is binary-per-threshold (deferred ladder-RPS) |
| PIT (continuous scalar) + discrete-randomization caveat | ADOPT | plain PIT for the Aeolus envelope |
| **CORP reliability + MCB−DSC+UNC** | **ADOPT (replaces binned Murphy + binned reliability)** | the headline upgrade; cross-fit MCB if gated |
| **Murphy diagram + dominance** | **ADOPT** | model-vs-model (I7); cheap; Triptych panel 3 |
| **Diebold–Mariano (model vs baseline)** | **ADOPT** | forecast-superiority significance on the forward window |
| CLV as market-edge gate | ADOPT (concept) | parallel to calibration; validate profit-claim on our data |
| WIS (interval/quantile producers) | OPTIONAL WS2 / strong WS3 | CRPS-for-quantiles, decomposes |
| Recalibration as a pre-trade transform | DEFER | diagnostic now; transform once sizing exists |
| Cost-loss / economic value envelope | DEFER (WS3+) | needs the execution-cost model |
| **Deflated Sharpe Ratio** | **DEFER → WS3** | needs trial count N; backtest-only |
| **PBO / CSCV** | **DEFER → WS3** | needs T×N config matrix; backtest-only |

---

## §5 Concrete changes to the WS2 spec/plan

1. **S3 (Murphy) → CORP.** Replace the binned REL/RES/UNC with a **PAV-isotonic CORP decomposition
   (MCB−DSC+UNC)** + the CORP reliability diagram with consistency bands. Keep `REL−RES+UNC=Brier` only
   as a *unit-test identity on synthetic binned data* (it's still a valid algebra check), but the
   **scorecard's calibration numbers come from CORP**, and MCB-as-a-gate-threshold is **cross-fit**.
2. **S2 (Log).** Keep ε=1e-15 clamp; **add a `log_tail_events` count of `p_realized<ε`** to the scorecard
   (the real overconfidence signal). Document ε as a fixed constant, never tuned.
3. **New small slice — Murphy diagram + Diebold–Mariano** for model-vs-model + model-vs-baseline on the
   forward window (serves I7 shadow comparison + the GO whole-truth). Cheap; could fold into S5.
4. **CRPS audit** (existing code): confirm factor-2 + Δτ weighting or document the convention.
5. **PIT:** add the discrete-producer randomization caveat (note; not needed for the continuous demo).
6. **Citations:** binning bias → Ferro–Fricker 2012 / Bröcker 2011 (not Bröcker 2009); CORP → DGJ 2021.
7. **DEFER table:** confirm DSR + PBO → WS3 (research validates this is correct, not a punt).

The CORP change is the material one (it reshapes S3 + S5's calibration fields). Everything else is
additive/refinement.

---

## §6 Confidence & caveats

- **High confidence:** all core formulas (Brier/RPS/Log/CRPS/PIT), the Murphy binning-bias result
  (Ferro–Fricker read in full), CORP `MCB−DSC+UNC` (PNAS, cross-confirmed), the DSR/PBO single-window
  boundary (Bailey & López de Prado).
- **Derived (standard but not a verbatim quote):** "ε-clamp breaks strict propriety" — follows from the
  uniqueness-of-optimum definition of strict propriety; uncontroversial.
- **Medium/practitioner:** the CLV "predicts profitability" claim (validate on our data); prediction-
  market calibration specifics (SEO numbers weak; the academic study stresses miscalibration).
- **Not fully extracted:** Hersbach 2000 exact per-bin CRPS-decomposition weights (not load-bearing —
  we are not implementing it in WS2). A peer-reviewed closing-line-efficiency paper wasn't fetched
  (CLV efficiency rests on practitioner sources here).

---

## §7 Sources (fetched primary unless tagged)

- Brier 1950, *Mon. Wea. Rev.* 78:1 — journals.ametsoc.org (primary, landing).
- Murphy 1973 "A New Vector Partition of the Probability Score," *J. Appl. Meteor.* 12:595 (primary).
- Bröcker 2009 "Reliability, sufficiency, decomposition of proper scores," arXiv:0806.0813 (primary, read full).
- **Ferro & Fricker 2012 "A bias-corrected decomposition of the Brier score," QJRMS 138:1954** —
  empslocal.ex.ac.uk/.../ferro-fricker2012copyright.pdf (primary, read full — the binning-bias source).
- Epstein 1969 / Murphy 1971 (RPS) (primary, refs confirmed).
- Gneiting & Raftery 2007 "Strictly Proper Scoring Rules," *JASA* 102:359 —
  sites.stat.washington.edu/raftery/Research/PDF/Gneiting2007jasa.pdf (primary, read full).
- Gneiting, Balabdaoui & Raftery 2007 "Probabilistic forecasts, calibration and sharpness," *JRSS-B* —
  …/Gneiting2007jrssb.pdf (primary).
- Gneiting 2011 "Making and Evaluating Point Forecasts," arXiv:0912.0902 (primary).
- Tibshirani et al. 2022 "Flexible Model Aggregation for Quantile Regression" —
  stat.cmu.edu/~ryantibs/papers/quantagg.pdf (primary — CRPS=2∫pinball + discrete estimator).
- Jordan, Krüger & Lerch, scoringRules (JSS 90:12) — cran.r-project.org/.../scoringRules vignette (primary).
- Czado, Gneiting & Held 2009 "Predictive Model Assessment for Count Data," *Biometrics* 65:1254 (primary — randomized PIT).
- Hersbach 2000 (CRPS decomposition), *Wea. Forecasting* 15:559 (primary, abstract; PDF 403).
- **Dimitriadis, Gneiting & Jordan 2021 "Stable reliability diagrams via isotonic regression," PNAS
  118(8)**, arXiv:2008.03033 (primary — CORP). R `reliabilitydiag` (authors' pkg).
- **Ehm, Gneiting, Jordan & Krüger 2016 "Of quantiles and expectiles," JRSS-B**, arXiv:1503.08195
  (primary — Murphy diagrams/dominance). R `murphydiagram`.
- Niculescu-Mizil & Caruana 2005 (ICML, recalibration); Kull et al. 2017 (AISTATS, beta calibration) (primary).
- Bracher, Ray, Gneiting & Reich 2021 "Evaluating epidemic forecasts in an interval format," *PLoS
  Comp Biol*, arXiv:2005.12881 (primary — WIS).
- Bailey & López de Prado 2014 "The Deflated Sharpe Ratio" — davidhbailey.com/dhbpapers/deflated-sharpe.pdf (primary).
- Bailey, Borwein, López de Prado & Zhu "The Probability of Backtest Overfitting" —
  davidhbailey.com/dhbpapers/backtest-prob.pdf (primary — PBO/CSCV).
- Diebold 2015 "Comparing Predictive Accuracy, Twenty Years Later," *JBES* 33:1 —
  sas.upenn.edu/~fdiebold/papers/paper113/Diebold_DM%20Test.pdf (primary — DM test).
- Richardson 2000 / Murphy 1977 (cost-loss value) — ECMWF; CAWCR verification (primary lineage).
- CLV: Pinnacle, Boyd's Bets, Buchdahl (reputable/practitioner); closing-line efficiency academic (mixed).
