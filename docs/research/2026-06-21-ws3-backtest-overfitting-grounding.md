# WS3 Backtest-Overfitting Grounding — Research Report

**Date:** 2026-06-21 · **For:** the FORTUNA WS3 build (`docs/superpowers/specs/2026-06-21-ws3-generic-backtest-design.md`, §5 G-TRUTH + §7 sweep machinery) · **Method:** orchestrator-worker deep research, 5 parallel subagents on primary sources, lead synthesis + cross-verification.

> **Framing (non-negotiable):** FORTUNA's edge is a **forecasting** edge — calibration (Brier-beats-baseline margin) + CLV — *not* PnL. The overfitting deflation therefore lands on the **forecasting-edge metric**; the paper-trade Sharpe is reported DSR-deflated only as walled-off context. The "trials" are the sweep over {calibration-window, recal-method, scope, GO-threshold}. The data has **overlapping, serially-correlated labels** (same-day/same-station weather brackets share one resolution event).

---

## Summary — what to ADOPT

1. **PBO via *purged + embargoed* CSCV, on the forecasting-edge metric.** CSCV is provably metric-agnostic (the authors state it works for "any performance evaluation metric"), so rank configs by **mean Brier-skill-vs-baseline** (and CLV), not Sharpe. PBO = fraction of combinatorial splits where the in-sample-best config lands below the OOS median. **Gate: PBO > 0.05 ⇒ overfit ⇒ fail.**
2. **Purging + embargo is the #1 lie-prevention** and is mandatory here. The belief label window is `[t_issue, t_resolve]`; same-station-day sibling brackets overlap → must be purged across folds; embargo `h ≥ 1 resolution cycle` (a day) absorbs weather's strong day-to-day serial correlation. Without it PBO is artificially good and the whole gate lies.
3. **Best-of-N significance = Hansen's SPA (`SPA_c`)**, not White's RC, on the Brier-loss differential, with a stationary block bootstrap (block ∝ N^(1/3), ≥ the autocorrelation horizon). SPA is the multiple-testing extension of the DM test WS2 already has.
4. **INSUFFICIENT-EVIDENCE is a first-class verdict**, gated on **effective-N** (serial-correlation-haircut of raw N), not raw N: fail to INSUFFICIENT if `N_eff < ~30` (CLT floor) or the CSCV folds can't retain serial structure, or (for the Sharpe context) `N_eff < MinTRL`.
5. **DSR for the walled-off Sharpe context only**, with the trial count `N` = the *effective independent* trial count (cluster the trial-return correlation matrix), since undercounting `N` is the #1 way to inflate it.

---

## 1. PBO via CSCV — the overfitting probability

**Source:** Bailey, Borwein, López de Prado, Zhu, "The Probability of Backtest Overfitting," *J. Computational Finance* 20(4) 2017 (PDF: davidhbailey.com/dhbpapers/backtest-prob.pdf; SSRN 2326253). **[primary]**

**Procedure (CSCV):**
- Build a **T×N matrix M**: T time-slices × N sweep configs; cell `(t,n)` = config n's value of the chosen performance metric on slice t.
- Partition rows into **S even** disjoint equal submatrices (each T/S × N). Form all **C(S, S/2)** combinations splitting them into IS (train, T/2×N) / OOS (test, T/2×N), joined in original row order.
- Per combination c: pick the **IS-best config n\***; find its **OOS relative rank** `ω̄_c = r̄_{n*}/(N+1) ∈ (0,1)`; the **logit** `λ_c = ln(ω̄_c/(1−ω̄_c))`. High λ ⇒ IS/OOS consistent ⇒ low overfit.
- **PBO `φ = fraction of combinations with λ_c < 0`** = P(IS-best config lands below the OOS median). Decision rule from the paper: **reject configs with PBO > 0.05.**
- Auxiliary outputs: **performance degradation** (regress OOS R̄_{n*} on IS R_{n*}; β<0 ⇒ overfit), **probability of loss** (P[OOS R̄_{n*} < 0]), **stochastic dominance** vs a random pick.

**Metric-agnosticism (the load-bearing fact for FORTUNA — verbatim):** *"our methodology does not rely on this particular performance statistic, and it can be applied to any alternative preferred by the reader"*; *"generic and can be applied to any performance evaluation metric R."* So M's cells hold the **forecasting-edge metric** (mean Brier-skill-vs-baseline, or CLV), not Sharpe. **Caveat:** the metric must be slice-computable; row order matters only for *path-dependent* metrics — a per-slice mean Brier-skill / mean CLV is order-insensitive and slice-computable, so it fits cleanly.

**Choosing S:** S even; #logits = C(S, S/2). **S=16 → 12,780 logits** (σ[φ] < 0.0045) is the paper's recommended default; for ~4yr daily data that's quarterly partitions, preserving serial structure. Too-small S underrepresents the left tail; too-large S shatters the time structure. Need N ≫ 10 for φ granularity.

## 2. Purging + embargo — the #1 lie-prevention

**Source:** López de Prado, "The 10 Reasons Most ML Funds Fail" (2017/2018), Pitfall/Solution #8 — the paper-form of *Advances in Financial ML* (2018) ch.7 (garp.org/hubfs/Whitepapers/a1Z1W0000054x6lUAA.pdf). **[primary]** *(Do NOT cite mlfinlab.com docs — that domain is currently hijacked/serves gambling; the GitHub `master` ships stub code. The procedure below is from the primary paper verbatim; the PurgedKFold code mechanics are reconstructed from it.)*

**Why plain CV leaks (verbatim):** *"Leakage takes place when the training set contains information that also appears in the testing set… Because of the serial correlation, X_t ≈ X_{t+1}; because labels are derived from overlapping data points, Y_t ≈ Y_{t+1}. Then, placing t and t+1 in different sets leaks information… leakage leads to false discoveries."* Effect: **OOS inflated ⇒ overfitting understated.**

**Purge (exact):** a label `Y = f([t0,t1])`. A train label i overlaps a test label j if any of: (1) `t_{j,0} ≤ t_{i,0} ≤ t_{j,1}`; (2) `t_{j,0} ≤ t_{i,1} ≤ t_{j,1}`; (3) `t_{i,0} ≤ t_{j,0} ≤ t_{j,1} ≤ t_{i,1}` — i.e. **`train.t0 ≤ test.t1 AND train.t1 ≥ test.t0`**. Drop matching observations **from train only**.

**Embargo (exact):** one-sided — only drops train obs *after* a test window: `t_{j,1} ≤ t_{i,0} ≤ t_{j,1} + h`. Default **`h ≈ 0.01·T`** (T = #observations; the widely-cited "5%" is illustrative-secondary, not the default). Implementation: extend each test window to `t1 + h` *before* purging. Needed *beyond* exact overlap because serial correlation leaks past the label window.

**CPCV / "purged CSCV":** the combinatorial split is run **with purge+embargo baked into every split's train set**, then the per-path metric feeds the CSCV/PBO logit. (BBLZ CSCV itself predates ch.7 and doesn't prescribe purging; combining them is standard practice.)

**FORTUNA application:** belief label window = **`[t_issue, t_resolve]`** (issue-time `t0`, settlement-time `t1`). Same-station-day brackets share *one* resolution event → near-perfectly concurrent → a sibling bracket in train while another is in test is a self-leak → **purge them**. Embargo **`h ≥ 1 resolution cycle` (≈1 day)** for weather's strong day-to-day serial correlation (a belief issued the morning after a test day's resolution shares NWP/weather state). Complement (when dropping that many train rows is too costly): **average-uniqueness / sequential-bootstrap concurrency down-weighting** (ch.7 Solution #7).

## 3. Effective-N + MinTRL → the INSUFFICIENT-EVIDENCE verdict

**Source:** Bailey & López de Prado, "The Sharpe Ratio Efficient Frontier," *J. Risk* 15(2) 2012 (davidhbailey.com/dhbpapers/sharpe-frontier.pdf, Eq.11 PSR, Eq.13 MinTRL). **[primary]** Effective-N: standard ESS results **[secondary, well-established]**.

- **PSR:** `PSR[SR*] = Z[(SR_hat − SR*)·√(T−1) / √(1 − γ3·SR_hat + ((γ4−1)/4)·SR_hat²)]` (Z = normal CDF; γ4 **raw**, Normal=3 ⇒ term = ½; use √(T−1)). Skew/kurtosis enter *only* through the SR standard error.
- **MinTRL:** `MinTRL = 1 + [1 − γ3·SR_hat + ((γ4−1)/4)·SR_hat²]·(Z_α/(SR_hat − SR*))²` — the minimum **#observations** to claim `SR_hat > SR*` at confidence 1−α. Requires `SR_hat > SR*` (else undefined). Negative skew + fat tails sharply raise it (worked example: 3.24yr → 4.99yr, +54%).
- **CLT floor (load-bearing):** the PSR/MinTRL asymptotics need raw n ≥ **~30** to even be valid. Two-part gate: (i) n ≥ 30 AND (ii) n ≥ MinTRL.
- **Effective-N under serial correlation:** raw N overstates evidence. `N_eff = N/(1 + 2·Σ_{t≥1} ρ_t)` (general); **AR(1) closed form `N_eff ≈ N·(1−ρ)/(1+ρ)`** (ρ=0.5 → 0.33·N; ρ=0.8 → 0.11·N). Substitute **N_eff** into both the ≥30 floor and the MinTRL comparison. (HAC/Newey-West = inflate the SE instead; block-bootstrap independent units = N/blocklen.)
- **CSCV sufficiency:** each T/S fold must retain enough *contiguous, serially-coherent* observations (≥ the autocorrelation length, ideally ≥30) for the OOS rank to mean anything.

**The verdict rule (ADOPT):** **INSUFFICIENT-EVIDENCE iff `N_eff < 30` OR `edge ≤ 0` OR (Sharpe context) `N_eff < MinTRL`.** MinTRL is Sharpe-specific (→ the walled-off Sharpe gate directly); the **forecasting-edge analog** is: effective-N must give the CSCV/PBO + the SPA test enough power (per-fold obs ≥ autocorr length + CLT floor). Under-powered ⇒ INSUFFICIENT, never GO. "We don't have enough independent data to say" *is* part of the whole truth.

## 4. White RC / Hansen SPA → the best-of-N forecasting-edge gate

**Sources:** White, "A Reality Check for Data Snooping," *Econometrica* 68(5) 2000 (users.ssc.wisc.edu/~bhansen/718/White2000.pdf); Hansen, "A Test for Superior Predictive Ability," *JBES* 23(4) 2005 (cdr.lib.unc.edu/downloads/zp38wf793); Politis & White, "Automatic Block-Length Selection," *Econometric Reviews* 2004. **[all primary]**

- **The problem:** picking the best of N forecasts and DM-testing *that one* is data-snooping. RC/SPA test the composite null **`H0: max_k E(d_k) ≤ 0`** ("the best model is no better than the benchmark"), where `d_{k,t}` is a **loss differential** — and **both work on any real-valued loss**, so `d_{k,t} = Brier(baseline_t) − Brier(model_k,t)` (or a CLV-based metric) is a valid input. These are *forecast-evaluation* tests by origin, not trading-only tools.
- **White RC:** statistic `V̄ = max_k √n·d̄_k`; p-value via the **stationary block bootstrap** (Politis–Romano) of `V* = max_k √n(d*_k − d̄_k)`.
- **Hansen SPA (the improvement — ADOPT):** (a) **studentize** — `T^SPA = max_k max(√n·d̄_k/ω̂_k, 0)` (divide by each d_k's std, taming erratic forecasts); (b) **recenter the null** — keep only demonstrably-inferior models (`µ̂_k^c = d̄_k·1{√n·d̄_k/ω̂_k ≤ −√(2 log log n)}`). The consistent p-value `p_c` sits between a liberal `p_l` and the conservative `p_u` (= RC). **Why SPA beats RC: RC is contaminated by poor/irrelevant models** — "it is even possible to erode the power of the RC to 0 by including poor forecasts"; SPA's recentering makes them asymptotically irrelevant. **Use `SPA_c`.**
- **DM relationship:** the DM statistic `√n·d̄/ω̂` IS SPA's per-model studentized statistic for one pair. **DM = single-pair (simple null); SPA/RC = best-of-N (composite null) — the multiple-testing extension.** WS2's DM is the building block; WS3's SPA is the selection-corrected version.
- **Block length:** stationary bootstrap **b ∝ N^(1/3)** (Politis–White automatic selection), larger the more persistent the series; for h-step forecasts d_t are MA(h−1)-dependent so block > h. For weather, tie to the day-to-day autocorrelation horizon.
- **Adjacent (for per-scope control):** Romano–Wolf StepM / Hsu–Hsu–Kuan stepwise-SPA identify *which* (config/scope) pairs beat the benchmark, not just whether the best does — same studentized-stat + block-bootstrap machinery.

## 5. Deflated Sharpe Ratio — walled-off PnL context only

**Source:** Bailey & López de Prado, "The Deflated Sharpe Ratio," *JPM* 40(5) 2014 (SSRN 2460551); N-estimation: López de Prado & Lewis 2018. **[primary; formulas triangulated across Wikipedia + marti.ai + reference code because the author PDF served garbled — the three agree]**

- **DSR = `Z[(SR_hat − SR0)·√(T−1) / √(1 − γ3·SR_hat + ((γ4−1)/4)·SR_hat²)]`.** Denominator uses **SR_hat** (the selected strategy's Sharpe), *not* SR0 — verified against reference code (Wikipedia renders this wrong). = PSR evaluated at the benchmark SR0.
- **SR0 (expected max Sharpe under N trials, Gumbel):** `SR0 = √(V[{SR_n}])·[(1−γ_e)·Z⁻¹(1−1/N) + γ_e·Z⁻¹(1−1/(N·e))]`, γ_e = 0.5772 (Euler–Mascheroni), V = variance of the N trial Sharpes.
- **Inputs:** SR_hat/T/γ3/γ4 from the *selected* strategy; V[{SR_n}] + N from *all* trials. Same per-period frequency. Decision: **DSR > 0.95**. Bigger N ⇒ bigger SR0 ⇒ smaller DSR.
- **N when trials correlate:** the *effective independent* trial count — cluster the trial-return correlation matrix (angular distance `d=√(½(1−ρ))` → ONC / hierarchical (lower bound) / spectral → N = #clusters). **Undercounting N is the #1 DSR inflation.**

## 6. Creative-but-sound additions (senior-quant lens)

- **Per-scope multiple-testing burden.** If the sweep runs across *many scopes*, the trial count for the deflation must include the **scope × config grid**, not each scope in isolation — else you snoop across scopes. Use **stepwise-SPA (Romano–Wolf StepM)** to report *which* scopes genuinely beat baseline with family-wise control, feeding per-scope GO verdicts.
- **CLV × PBO as a second, faster edge axis.** CLV resolves per-trade (fast) while Brier needs volume (slow). Run PBO/SPA on **both** the Brier-skill metric and the CLV metric; require the *primary* (Brier) to pass and report CLV as the corroborating fast signal. CLV is serially correlated across related markets → purge/embargo applies to it too.
- **Recalibration is itself an overfit surface.** The recal method (Platt vs isotonic; isotonic especially) is *fit on data*, so make it a **swept config dimension** — the deflation then accounts for recalibration-overfitting. Pair with the WS2 CORP **cross-fit MCB** so calibration quality is measured out-of-sample.
- **Proxy-truth caveat (Hansen).** RC/SPA can be inconsistent when "truth" is a proxy. FORTUNA's *outcome* is the actual settlement (not a proxy → fine), but **CLV's benchmark is a market price, not ground truth** — treat CLV significance as edge-vs-market, not edge-vs-truth, and don't let a CLV-only result drive GO.
- **Report PBO + SPA-p + N_eff together, never one number.** The honest GO surface is the *joint* `{selected config, N_trials, N_eff, PBO, SPA p_c, OOS edge, sharpe_DSR, verdict}` — the literal "whole truth" G-TRUTH posture.

## 7. ADOPT → mapped to the WS3 spec

| Spec location | Adopt |
|---|---|
| §5 G-TRUTH (the gate) | Verdict ∈ {GO, NO-GO, **INSUFFICIENT-EVIDENCE**}. **GO iff** N_eff sufficient (≥30 + CSCV-fold-coherent) **AND** PBO ≤ 0.05 **AND** SPA `p_c` < α (selected config's deflated edge significant) **AND** OOS edge > 0. Else NO-GO; under-powered ⇒ INSUFFICIENT. |
| §7 sweep machinery | Build the **T×N edge matrix** (slices × {cal-window, recal, scope, GO-threshold} configs; cell = mean Brier-skill-vs-baseline). **Purged+embargoed CSCV** (S=16 default, tuned to keep per-fold obs ≥ autocorr length + ≥30; purge on `[t_issue,t_resolve]` overlap; embargo h ≥ 1 day) → **PBO**. **Hansen SPA_c** on the Brier-loss differential (stationary block bootstrap, block ∝ N^(1/3) ≥ autocorr horizon) → the **best-of-N p-value**. **Effective-N** haircut → the INSUFFICIENT gate. |
| §7 walled-off context | **DSR** on the paper Sharpe with N = effective-independent trials (cluster the trial-return correlation matrix). Report, never gate. |
| §5/§7 deflation module | Pure math (no IO) — lean toward `fortuna-scoring` (reusable, deterministic) with the orchestration in `fortuna-backtest`. PBO, purged-CSCV splitting, SPA bootstrap, DSR, effective-N, MinTRL. |

## 8. Confidence & caveats

- **High confidence (primary, verbatim or cross-consistent):** PBO/CSCV procedure + metric-agnosticism (verbatim); purge/embargo conditions + h≈0.01T (verbatim); PSR/MinTRL Eq.11/13 (the denominator form is *identical* across the MinTRL and DSR subagents → cross-verified); SPA studentization + recentering + DM relationship + block-length ∝ N^(1/3) (primary PDFs).
- **Medium-high (triangulated):** the DSR formula — the author PDF served garbled, so it rests on Wikipedia + a notation-faithful writeup + a reference implementation, which agree; the **denominator-uses-SR_hat** point was the one contested item, resolved against reference code (Wikipedia is wrong there).
- **Inferred (not load-bearing):** the exact PurgedKFold/getEmbargoTimes *code* (mlfinlab docs hijacked, GitHub stubbed) — but the *procedure* is primary; we implement from the procedure.
- **Open for the plan:** the AR(1) effective-N is the working approximation (use the general `N/(1+2Σρ_t)` if the edge series isn't AR(1)-like); the exact Lo (2002) serially-correlated Sharpe SE was named but not re-fetched (request if needed for the DSR context); weather-specific purge/embargo guidance is our faithful application of the general `[t0,t1]` event-label framework (no source gives weather-specific numbers).

## Sources (fetched)
- Bailey, Borwein, López de Prado, Zhu — *The Probability of Backtest Overfitting* — https://www.davidhbailey.com/dhbpapers/backtest-prob.pdf · SSRN https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2326253
- López de Prado — *The 10 Reasons Most ML Funds Fail* (= AFML ch.7) — https://www.garp.org/hubfs/Whitepapers/a1Z1W0000054x6lUAA.pdf
- Bailey & López de Prado — *The Sharpe Ratio Efficient Frontier* (PSR, MinTRL) — https://www.davidhbailey.com/dhbpapers/sharpe-frontier.pdf
- Bailey & López de Prado — *The Deflated Sharpe Ratio* — SSRN https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2460551 · notation https://marti.ai/qfin/2018/05/30/deflated-sharpe-ratio.html · ref code https://github.com/rubenbriones/Probabilistic-Sharpe-Ratio
- López de Prado & Lewis (2018) — *Detection of False Investment Strategies* (effective-N via clustering)
- White (2000) — *A Reality Check for Data Snooping* — https://users.ssc.wisc.edu/~bhansen/718/White2000.pdf
- Hansen (2005) — *A Test for Superior Predictive Ability* — https://cdr.lib.unc.edu/downloads/zp38wf793
- Politis & White (2004) — *Automatic Block-Length Selection for the Dependent Bootstrap* — https://public.econ.duke.edu/~ap172/Politis_White_2004.pdf
