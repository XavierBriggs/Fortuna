# Perpetual Futures — Funding-Rate Dynamics & Basis / Cross-Market Relative Value

**Decision-grade research memo · 2026-06-18**
Roles: senior quant (derivatives/funding-rate modeling) + perp-microstructure specialist.
Method: deep-research-protocol (6 parallel web-research streams → synthesis, contradiction map,
red team, confidence calibration). Sources graded T1 (peer-reviewed/official) → T4 (forum/anecdote).
Load-bearing event-perp + predictability claims independently re-verified by the main loop.

---

## 1. Executive Summary

For **crypto** perps, funding is not a clever-model problem: funding is mechanically a clamped
transform of the basis/premium index, and it is so autocorrelated (AR(1) ≈ 0.97–0.99) that a
naïve "no-change" forecast is near-optimal — the only paper with clean out-of-sample evidence
(DAR beats no-change, Inan 2025) wins mostly on funding *variance*, not level. **The edge is not
in forecasting funding; it is in capturing the structural carry and in cross-market relative
value** (perp vs spot/dated, and cross-venue funding dispersion), net of fees — and even that is a
decaying risk premium (crypto carry Sharpe ran 6.45 → 4.06 → negative by 2025). **Build the
basis/premium nowcast first, not a standalone funding forecaster**: a good basis estimate
*subsumes* the funding forecast, and it is the only piece that transfers to FORTUNA's event-perp
case. The single biggest caveat: **true event-probability perpetuals do not yet exist as a live
product** — the entire serious treatment is two non-peer-reviewed May-2026 preprints by one
author, whose own framework fails three of five validation benchmarks — so the crypto
"funding ≈ basis ≈ carry" keystone breaks (no spot to carry against) and the valuation/risk layer
must be rebuilt, with commodity roll-yield and barrier-option math the better priors.

---

## 2. Research Objective

- **Topic.** Predictive modeling *and strategy design* for perpetual-futures funding-rate dynamics
  and basis / cross-market relative value — crypto perps as the deep evidence base, with a transfer
  assessment to event-probability perpetuals (FORTUNA context, kept light).
- **Decision supported.** Which signal to build first (funding forecast vs. relative-value basis);
  the features and data that matter; how to validate honestly; realistic edge net of fees and
  funding costs; capacity/crowding limits; the highest-leverage next research step.
- **Audience.** Xavier — senior quant building FORTUNA (propose-only model, deterministic gates,
  single-threaded deterministic core).
- **Time horizon.** Immediate build decision + 1-year strategy.
- **Output.** 12-section technical memo.
- **Success.** Reader can pick the first signal, knows the validation method and net-of-cost edge,
  and knows what to research next.
- **Assumptions stated to proceed.** Deep base = USD-margined linear crypto perps on major venues
  (Binance/Bybit/OKX/BitMEX/Hyperliquid/dYdX). Event-perp transfer gets its own section; FORTUNA
  integration kept light. "Edge" is judged net of fees + funding + slippage, not gross APR.

---

## 3. Perspective Analysis

**Practitioner — *what matters in the real world?***
- *Core thesis.* The model is the easy part; net edge is destroyed by fees, the negative-funding
  tail, and the operational surface of multi-venue collateral.
- *Strongest evidence.* Only 40% of ≥20 bps funding-spread "arbitrage" opportunities are profitable
  after costs and 95% are force-exited before convergence (MDPI *Mathematics* 2026, T1); pro
  delta-neutral books realized ~15–19%/yr with <2% DD only by running maker-only, multi-leg (Gate/
  PANews, T3).
- *Hidden assumptions.* That you can get maker fills and cross-margin both legs; that you can hold to
  convergence.
- *Blind spots.* Treats the signal as solved and execution as the whole game — understates how
  little incremental alpha the funding *forecast* itself carries.
- *Unique insight.* Quote every funding APR as **gross**; haircut by fees × roll frequency + the
  ~16–18% of days the raw perp leg pays negative.

**Academic — *what does formal evidence say?***
- *Core thesis.* Funding is the soft tether converging a never-expiring contract to spot; it is
  statistically predictable but only marginally beyond strong autocorrelation, and the carry is a
  risk premium, not an anomaly.
- *Strongest evidence.* He, Manela, Ross, von Wachter derive `Fₜ = [κ/(κ−(r−r'))]·Sₜ` (cost-of-carry
  anchor) and measure ~60–90%/yr *mean-absolute* deviation from no-arb with ~0 *mean* deviation, arb
  Sharpe 1.8–3.5 (arXiv 2212.06888, T1); BIS "Crypto Carry" frames the ~7%/yr (spiking >40%) carry as
  a *negative convenience yield* from leverage demand colliding with limits-to-arbitrage (BIS WP 1087,
  T1).
- *Hidden assumptions.* Stationary-enough relationships; that published in-sample Sharpes survive
  forward.
- *Blind spots.* Sample periods under-weight 2022-style blow-ups; "predictable" ≠ "profitable
  after costs."
- *Unique insight.* The carry is *compensation for crash/segmentation risk* — high Sharpe understates
  tail.

**Skeptic — *assume the consensus is wrong.***
- *Core thesis.* Funding is a near-random-walk-in-levels whose only robust property (autocorrelation)
  is already priced; the apparent edge is decaying and regime-fragile.
- *Strongest evidence.* Crypto carry Sharpe 6.45 (2020–25) → 4.06 (from 2024) → **negative in 2025**
  (arXiv 2510.14435 + practitioner notes, T2/T3); single-asset funding→T+1 price R² = 0.000
  (Presto Labs, Aug 2024, T2); predictability is explicitly *time-varying* (Inan 2025, T1).
- *Hidden assumptions.* That decay continues monotonically (it didn't in the 2025 bull, where funding
  *rose* ~50% vs 2024).
- *Blind spots.* Can't distinguish "arbitraged away" from "regime-masked."
- *Unique insight.* Make the **no-change forecast and the raw carry** your benchmarks; anything you
  build must beat *those*, net of cost — most published "alpha" doesn't.

**Economist — *follow the incentives.***
- *Core thesis.* Funding exists because retail wants leveraged long exposure and won't hold spot; the
  carry is the fee they pay the short side for that convenience.
- *Strongest evidence.* BIS: carry rises with net-long positioning of *smaller, trend-chasing*
  traders; dealers/leveraged funds take the short side (BIS WP 1087, T1). Default interest baseline is
  positive (0.01%/8h ≈ 11% APR "paid to short"), so longs pay even at zero premium.
- *Hidden assumptions.* The retail long-bias persists; venues keep the positive interest convention.
- *Blind spots.* Doesn't price the reflexive unwind (Ethena's ~$6.5B supply exodus when funding
  cooled, Oct 2025→early 2026, T3).
- *Unique insight.* You are being paid to **warehouse other people's leverage demand** — size to the
  tail, not the mean.

**Historian — *look for precedent.***
- *Core thesis.* Each cycle industrializes the carry and compresses it: 2021 offshore quarterly
  premium ~40% → <10% in weeks after the May crash; 2024 spot-ETF launch turned cash-and-carry into a
  crowded institutional trade (CME basis ~25% Feb-2024 → ~10% May-2025, briefly <0 Feb-2025).
- *Strongest evidence.* CoinDesk 2021 carry post-mortem (T3); CME OpenMarkets + CFBenchmarks 2025
  (T1/T2).
- *Hidden assumptions.* That history rhymes; that the next vehicle (event perps) follows the same
  crowding arc.
- *Blind spots.* Each episode's unwind was triggered by something not in the prior model.
- *Unique insight.* The trade is **front-loaded into euphoria and most crowded exactly when it looks
  best** — the basis is a momentum/sentiment object, not a constant.

**Engineer — *can it actually work?***
- *Core thesis.* What's buildable is constrained by data and by FORTUNA's architecture, not by model
  sophistication.
- *Strongest evidence.* Funding/mark/index history is abundant and ~free; historical OI (Binance ~30
  days native) and historical full L2 order book (no free reconstructable source) are the scarce/
  expensive classes (Stream F, venue docs T1). Order-book-imbalance and VPIN signals are real but
  evaporate after taker fees + latency for a non-colocated participant (TDS/Medium T3; Gould–Bonart
  arXiv 1512.03492 T1).
- *Hidden assumptions.* That you can read per-symbol funding interval correctly (1/2/4/8h CEX, hourly
  DEX) and segment the series at formula-change dates (OKX 2026-06-01).
- *Blind spots.* Survivorship (native APIs drop delisted perps) and index-basket drift silently bias
  backtests.
- *Unique insight.* FORTUNA is **structurally a slow, propose-only, gated system** — it cannot exploit
  the sub-minute microstructure signals that are crypto's most "predictive," so design for
  funding-interval/daily horizons, not ticks.

**Futurist — *second-order effects.***
- *Core thesis.* Event-probability perpetuals are the genuinely new instrument, and they are
  pre-product: the funding mechanic transfers but the valuation layer is greenfield.
- *Strongest evidence.* Kalshi BTCPERP (live 2026-06-03) and Polymarket perps are *crypto/asset-price*
  perps launched by prediction-market firms, not probability perps (Kalshi/CNBC, T1/T2, re-verified);
  the only design treatment is two May-2026 preprints by one author, explicitly non-deployable
  (arXiv 2605.10400 / 2605.10428, re-verified).
- *Hidden assumptions.* That event perps materialize at all / at scale.
- *Blind spots.* Could be building for an instrument class that never ships.
- *Unique insight.* If/when event perps exist, **funding becomes a pure demand-balancing fee against a
  constructed reference probability** — which is exactly a *belief-vs-market* edge (FORTUNA's core),
  not a carry arbitrage.

*Regulator/Policymaker lens — run light:* relevant only that Kalshi's crypto perps are under a live
"futures vs. swaps" classification debate (CoinDesk 2026-06-12, T3), which affects whether/where
event perps can be offered — but it does not change the modeling problem, so it is not load-bearing
here.

---

## 4. Evidence Matrix

| # | Claim | Supporting | Contradicting / caveat | Source | Type | Date | Tier | Conf | Quality |
|---|-------|-----------|------------------------|--------|------|------|------|------|---------|
| 1 | Funding = premium index + clamp(interest − premium, ±0.05%); outside calm regime funding ≈ basis | Binance/Bybit/BitMEX/OKX docs share the formula | `8/N` interval scaling differs; BitMEX computes interest dynamically | Binance/Bybit/OKX/BitMEX funding docs | Official | 2026-06 | T1 | 95% | high |
| 2 | Funding is extremely autocorrelated (AR(1) ≈ 0.966–0.998), mean-reverts slowly | 35.7M obs, 26 exchanges; AC > 0.80 at lag 8 | 8-day window; only ETH clearly stationary | MDPI *Mathematics* 14(2):346 | Peer-rev | 2026 | T1 | 80% | high |
| 3 | Funding is OOS-predictable: DAR beats no-change on error + direction | Diebold–Mariano vs no-change, BTC Binance/Bybit | Edge is mostly conditional *variance*; predictability time-varying; effect size not extractable | Inan, SSRN 5576424 (verified) | Working paper | 2025-10 | T1 | 70% | med |
| 4 | The basis/premium index is the dominant *feature* (funding is its clamped transform) | Amberdata; mechanical identity | Saturates to 0.01% floor when basis small; signal lives in tails | Amberdata; DerivaDEX | Industry | 2025 | T2 | 80% | med |
| 5 | Funding has structural positive bias (longs pay shorts ~92% of time) | BitMEX Q3-2025: BTC exactly 0.01% for 78% of qtr | Binance BTC mean only 0.0057%/8h — below nominal floor; flips deeply negative in stress | BitMEX Q3-2025 report; Ethena docs | Official | 2025 | T1 | 85% | high |
| 6 | Crypto carry is a *risk premium* (negative convenience yield), not free arbitrage | Carry ~7%/yr, >40% spikes; rises with small-trader net-long | Loads on equity-vol/crash risk; high Sharpe understates tail | BIS WP 1087; Fan et al. SSRN | Peer-rev/official | 2023–25 | T1 | 85% | high |
| 7 | Net edge ≪ gross APR; spread "arbs" mostly unprofitable after cost | 40% of ≥20bps spreads profitable; 95% forced exits | Maker-only VIP execution flips fee headwind to neutral | MDPI *Mathematics* 14(2):346 | Peer-rev | 2026 | T1 | 75% | high |
| 8 | Carry edge is decaying: Sharpe 6.45 → 4.06 → negative (2025) | Carry research series | 2025 bull *raised* funding ~50% vs 2024 — decay is regime-masked, not monotone | arXiv 2510.14435; practitioner | Working paper/T3 | 2025 | T2 | 55% | med |
| 9 | Ethena USDe = canonical scaled funding-capture; ~16–18% of days raw perp leg pays negative | ETH 17.5% neg days (+9.15%/yr avg); BTC 15.9% (+7.80%/yr); 1 of 12 qtrs net-neg | ~1% reserve fund is thin vs tail (Deribit ETH −370%/yr Sept-2022) | Ethena docs; LlamaRisk V2 | Primary/industry | 2024–26 | T1/T2 | 88% | high |
| 10 | Cross-venue funding dispersion is durable RV (fatter than single-venue carry) | HL-vs-Binance BTC funding avg ~11% APR gap; peaks 23–48% APR | Needs dual-venue collateral, double liquidation surface; peaks episodic | Pendle/Boros; BitMEX Q3-2025 | Industry/official | 2025 | T2/T1 | 78% | med |
| 11 | Order-book / taker imbalance predicts price sub-minute but not after costs/latency for takers | Queue-imbalance one-tick predictor (Gould–Bonart) | 10s moves <10bps vs ~10bps fees; profitable only for colocated maker | arXiv 1512.03492; TDS | Peer-rev/T3 | 2015–25 | T1/T3 | 75% | high |
| 12 | VPIN/order-flow toxicity predicts BTC *jumps* (not direction) | Peer-reviewed VAR; persistent VPIN + jumps | Forecasts vol tails, not signed return; bucketing-sensitive | ScienceDirect S0275531925004192 | Peer-rev | 2026 | T1 | 70% | high |
| 13 | Long/short *account* ratio & liquidation-cluster "magnets" = folklore; no OOS net-of-cost edge | Widely used dashboards | Zero backtest found; liquidation feeds throttled/incomplete → biased backtests | Binance API docs; CoinGlass | Official(defn)/T4(edge) | 2026 | T4 | 25% | low ⚠ |
| 14 | **True event-probability perpetuals are not a live product** | Kalshi/Polymarket perps are crypto/asset-price; Drift/HIP-4 are expiry binaries | — | Kalshi/CNBC/Drift docs (verified) | Primary/T2 | 2026 | T1/T2 | 88% | high |
| 15 | The only serious event-perp design = 2 non-peer-reviewed May-2026 preprints, **one author**, validation fails 3/5 benchmarks | PIRAP framework; 7-variant taxonomy; Polymarket 13,298 mkts | Single author; "explicit non-deployable status"; mixed results | arXiv 2605.10400 / .10428 (verified) | Preprint | 2026-05 | T2 | 85% | med ⚠ |
| 16 | For event perps the "funding ≈ basis ≈ carry" keystone breaks (no spot to carry) | Bounded [0,1], jump-to-resolution, tail-thin liquidity; basis-only funding fails near boundaries | Reasoned from 2 preprints + mechanism; thin empirics | arXiv 2605.10400; AHJ NBER w32936 | Preprint/working | 2026 | T2 | 80% | med |
| 17 | Funding/mark/index history is abundant & ~free; OI and L2 order-book are the scarce/expensive classes | Binance fundingRate ≤1000/call; OI native ~30d; no free reconstructable L2 | Aggregator (Velo $199/mo) normalizes cross-venue | Venue docs; Coinglass/Velo | Official/vendor | 2026-06 | T1 | 90% | high |
| 18 | Data correctness traps: per-symbol funding interval, formula regime breaks, survivorship, index-basket drift | OKX formula change 2026-06-01; delisted perps vanish from native APIs | Majors stayed 8h for years — hazard concentrates in alts | Venue docs; freqtrade #12583 | Official/T3 | 2026 | T1 | 88% | high |

⚠ Row 13 rests on Tier-4 evidence for the *edge* claim (definitions are T1; tradability is folklore).
⚠ Row 15 rests on a single non-peer-reviewed author — flagged as the central source-quality risk.

---

## 5. Contradiction Map

**Major contradictions**
- **Is funding predictable enough to trade?** *Predictable* (DAR beats no-change, Inan 2025, T1;
  basis is a near-deterministic driver) vs. *effectively a random walk in levels* (AR(1)≈0.99 makes
  no-change near-optimal; single-asset funding→T+1 price R²=0; carry Sharpe gone negative 2025).
  Evidence each side is T1/T2. **Resolves via:** a costed, walk-forward horse race of {no-change,
  AR/DAR, basis-nowcast} on *your* venues and horizon — the question is empirical and venue-specific.
- **Gross carry vs. net realized.** Headline 8–27% APR vs. only 40% of ≥20 bps spreads profitable
  after cost, 95% force-exited. **Resolves via:** modeling maker-vs-taker fills and roll frequency
  explicitly; the disagreement is mostly an execution-assumption gap.
- **Is the simple trade arbitraged away?** "Diminishing returns / Sharpe→negative" vs. "2025 funding
  *rose* ~50% as bull inflows widened it faster than arb capital compressed." **Resolves via:**
  separating decay by regime — the easy trade decays in flat/bear, re-inflates in euphoria.
- **Event-perp transfer.** "Funding plumbing + positioning intuition transfer" vs. "valuation/risk
  layer must be rebuilt; the carry keystone breaks." Both from the same thin source base. **Resolves
  via:** whichever event-perp product actually ships and how it defines its reference index.

**Minor contradictions**
- OI as predictor (leverage proxy, useful) vs. OI-matrix TA lore (weak formal backing).
- Order-book imbalance "predictive" (T1 LOB theory) vs. "not profitable" (T3 applied) — reconciled by
  *who* trades it (colocated maker yes, taker no).
- Reserve-fund adequacy: Ethena's ~1% buffer "historically sufficient" vs. "thin vs. a sustained deep
  negative-funding tail."

**Open questions / unknowns**
- Effect size of DAR-over-no-change (paper body not extractable) — is the OOS error reduction
  economically meaningful or just statistical?
- The real magnitude of snapshot funding-gaming (mechanism solid, magnitude under-documented).
- How any future event-perp will define its reference probability (TWAP vs. sister dated market vs.
  oracle) — under-specified even in the design papers.

**Consensus areas (genuine agreement across streams & sources)**
- Funding is *mechanically* a clamped transform of the (impact-price) basis/premium index.
- Funding is strongly autocorrelated and structurally positive-biased.
- The crypto carry is a *risk premium* with crash tails, not a free arbitrage.
- Net edge ≪ gross APR; fees + the negative-funding tail dominate realized return.
- No live event-probability perpetual exists today; the funding=basis=carry chain does not survive
  the absence of a tradeable spot.

---

## 6. Blind Spot Report

- **Stakeholders ignored.** Market-makers/keepers (who actually set the premium and earn the
  maker-fee edge) and venue risk teams (who set clamps/caps) — they capture much of the "edge" before
  a taker sees it.
- **Hidden incentives.** Exchange-published funding stats (BitMEX "92% positive") are marketing;
  aggregator vendors have an incentive to overstate coverage/uniqueness. Graded down accordingly.
- **Unchallenged assumptions.** That historical crypto funding behavior is the right prior for event
  perps (it is largely *not* — different underlying geometry). That backtests use survivor-bias-free,
  formula-segmented data (most don't).
- **Topics skipped / under-weighted.** Tax/financing of the spot leg; counterparty/venue solvency
  risk (FTX-grade); the legal classification of perps (futures vs. swaps) that gates *where* FORTUNA
  could trade event perps.
- **Data that was unavailable / uncheckable.** (1) **The event-perp instrument itself has no market
  data — it doesn't exist — so the FORTUNA-relevant target cannot be backtested at all.** (2) Most
  premium-vendor prices (Tardis/Kaiko/Amberdata/CoinAPI/CCData/Laevitas) are quote-only/unconfirmed;
  only Coinglass/Velo/Coinalyze have confirmed pricing. (3) DAR effect size (paywalled body). (4)
  Historical index-basket composition changes (no public changelog).

---

## 7. Red Team Assessment

**Attack 1 — "Build a funding forecaster" is the wrong first move.**
- *Logic.* Funding's AR(1)≈0.99 makes no-change near-optimal; the one OOS win is on variance, not
  level; and FORTUNA's actual target (event perps) has no funding-autocorrelation history to mine.
- *Impact.* Months spent beating a benchmark by a few bps that won't transfer.
- *Response.* Conceded — and it is *why* the recommendation is the **basis/premium nowcast**, not a
  standalone funding forecaster. The basis nowcast subsumes the funding forecast and is the piece that
  transfers (belief-vs-reference gap).
- *Residual risk.* Even the basis nowcast's *incremental* alpha over raw carry may be thin in calm
  regimes; value concentrates in tails/stress.

**Attack 2 — The whole event-perp thesis rests on one unpublished author.**
- *Logic.* Rows 15–16 trace to two May-2026 preprints by Maksym Nechepurenko, non-peer-reviewed, whose
  own framework fails 3 of 5 validation benchmarks.
- *Impact.* If the "funding=basis breaks / rebuild the valuation layer" claim is wrong, the transfer
  guidance is wrong.
- *Response.* The *structural* argument (bounded [0,1], jump-to-resolution, no spot to carry) is
  definitional and corroborated by general perp-pricing theory (AHJ, which assumes a tradeable spot)
  — it does not depend on the preprint's empirics. The preprints are evidence of *what's been tried*,
  not the load-bearing logic.
- *Residual risk.* The *specific* design fixes (composite index, jump-aware margin) are unproven and
  may be wrong in detail; treat them as hypotheses, not blueprints.

**Attack 3 — "Relative value is the edge" ignores that the easy money is gone and capacity is small.**
- *Logic.* Cross-venue dispersion needs multi-venue collateral and is capacity-/skill-constrained;
  carry Sharpe went negative in 2025.
- *Impact.* Real net edge for a small, slow, propose-only system could be ~0 after cost.
- *Response.* For a *small* operator, capacity/crowding is not the binding constraint — access,
  fees, and the negative-funding tail are. The durable RV is venue-dispersion and term structure,
  which a careful operator can still harvest at maker fees; but size to the tail.
- *Residual risk.* If FORTUNA can only reach taker fills, the net edge on pure funding RV may not
  clear costs — the honest base case is "marginal."

**Attack 4 — Survivorship/data-quality invalidates any backtest you run.**
- *Logic.* Native APIs drop delisted perps; funding formulas change prospectively; index baskets
  drift — all bias backtests optimistically.
- *Impact.* A "validated" edge that is a data artifact.
- *Response.* Mitigated by sourcing survivor-bias-free history (Tardis), segmenting at formula-change
  dates, and using a vendor-normalized cross-venue series (Velo). This is a known, fixable trap.
- *Residual risk.* Self-captured/free data will retain some bias; budget for it in confidence
  intervals.

---

## 8. Key Findings

1. **Funding is the basis, clamped.** Outside the calm regime, `funding ≈ time-averaged premium
   index`, so a live, depth-aware (impact-price) **basis/premium nowcast is the core predictor** — the
   0.01%/8h interest baseline only matters when the perp is near-pegged. *So what:* build the basis
   nowcast, not a funding model around it.

2. **Funding is predictable in the trivial sense and barely beyond it.** AR(1)≈0.97–0.99 makes
   no-change near-optimal; DAR's OOS win (Inan 2025) is real but mostly on *variance*, and
   single-asset funding→price is ~0 R². *So what:* benchmark everything against no-change + raw carry,
   net of cost; expect small incremental level-alpha.

3. **The money is in the carry and in relative value, and it's a decaying risk premium.** Structural
   long-pays-short bias (~7.8–9%/yr per Ethena's own 3-yr data) funds delta-neutral capture; the
   durable edge is cross-venue funding dispersion (~11% APR avg HL-vs-Binance, peaks 23–48%) and
   term structure. But carry Sharpe ran 6.45 → negative (2025), and net ≪ gross. *So what:* harvest
   carry/RV, size to the negative-funding tail, treat gross APR as a mirage.

4. **FORTUNA's architecture cannot exploit crypto's most "predictive" signals.** Order-book imbalance
   and VPIN are real but evaporate after taker fees + latency for a non-colocated, propose-only, gated
   system. *So what:* design for funding-interval/daily horizons; ignore tick microstructure.

5. **Event-probability perpetuals are pre-product, and the crypto carry keystone does not transfer.**
   No live event-perp exists; the only design treatment is two non-peer-reviewed May-2026 preprints by
   one author. With no spot to carry, "funding ≈ basis ≈ carry" breaks — funding becomes a
   demand-balancing fee against a *constructed reference probability*. *So what:* for FORTUNA the edge
   is **a better reference probability than the market's implied one** (a belief-vs-price RV), with
   commodity roll-yield / barrier-option math as the right priors — not the crypto funding literature.

---

## 9. Recommendations (ordered by leverage)

1. **Build the relative-value basis signal first, not a standalone funding forecaster** (Findings 1,
   2, 5). Concretely: a depth-aware **basis/premium nowcast** = (perp mark − reference index), where
   for crypto the reference is the spot index and for event perps the reference is *your own
   belief/model probability*. This subsumes the funding forecast (funding is its clamped transform)
   and is the only component that transfers to FORTUNA's event-perp case.

2. **Make the trade carry-capture + cross-venue RV, and validate on net-of-cost convergence, not
   gross APR** (Findings 3, and Evidence 7–10). Validation protocol: walk-forward, maker-vs-taker fee
   models, explicit negative-funding-day accounting, hold-to-convergence force-exit modeling, and a
   benchmark of {no-change funding, raw carry} that your signal must beat *after cost*. CLV-style
   "did the basis converge toward the nowcast" is the honest metric — not backtest ROI.

3. **Engineer for the funding-interval/daily horizon; do not chase ticks** (Finding 4). Skip
   order-book-imbalance/VPIN features unless and until a proven edge justifies the L2 data spend.
   Feature set that matters, in order: (a) basis/premium index (the driver), (b) funding's own lag
   (the AR baseline), (c) a DAR/GARCH variance layer for sizing/risk, (d) cross-venue funding
   lead-lag, (e) OI/leverage-ratio and funding-extreme flags as *regime/risk conditioners only*.

4. **Fix the data correctness traps before trusting any backtest** (Evidence 17–18; Red Team 4).
   Read per-symbol funding interval at each timestamp (never hard-code 8h), segment the series at
   formula-change dates (OKX 2026-06-01), source survivor-bias-free history, and use a normalized
   cross-venue series. Cheapest viable stack: **free venue APIs + Velo ($199/mo)**; defer Tardis L2
   until OB features are justified; skip Kaiko/Amberdata unless compliance forces it.

5. **Treat the event-perp valuation layer as greenfield R&D, not a port** (Finding 5; Red Team 2).
   Reuse only the funding-payment plumbing and the positioning-as-signal *intuition*. Rebuild: the
   reference-probability index, the mean-reversion-to-latent-probability model, jump-/resolution-aware
   margin, and boundary-liquidity handling. Prior art: commodity roll/convenience yield + barrier
   options, **not** BTC perp funding.

---

## 10. Confidence Scores

| Finding | Conf | Reasoning | Weaknesses | Raises confidence | Lowers confidence |
|---------|------|-----------|------------|-------------------|-------------------|
| F1 Funding ≈ clamped basis | 92% | Mechanical identity in T1 venue docs | Cross-venue formula differences | Reproduce per venue from docs | A venue with a non-standard formula |
| F2 Predictable only marginally beyond AR | 78% | T1 autocorrelation + T1 OOS (DAR) + T2 R²=0 | DAR effect size unextractable | Costed walk-forward on your data | DAR shows large economic OOS gain |
| F3 Edge = carry/RV, decaying risk premium | 75% | BIS + Ethena primary + MDPI cost study | Decay is regime-masked, not monotone | Multi-cycle net-of-cost backtest | A structural reason funding stays high |
| F4 FORTUNA can't use tick signals | 85% | Architecture + T1/T3 cost-of-latency evidence | Some daily OI signal may survive | Confirm fill/latency reality | A low-latency execution path appears |
| F5 Event perps pre-product; keystone breaks | 80% | 2 verified preprints + definitional logic + AHJ | Single author; thin empirics | A second independent design/product | A shipped event-perp that keeps funding=basis |

---

## 11. Open Questions

**Blocks the decision**
- What is FORTUNA's *actual* tradeable surface in the next 12 months — crypto perps (live, deep data)
  or event perps (no product yet)? The first signal to build depends on this. If event perps, the
  crypto funding work is a muscle-builder, not the product.
- Can FORTUNA reach **maker** fills? If only taker, the net edge on pure funding RV may not clear
  costs (Red Team 3) — this single fact swings the realistic edge from "marginal-positive" to "~0".

**Good to know**
- Effect size of DAR-over-no-change (is it economically meaningful?).
- How a future event-perp defines its reference probability (TWAP / sister market / oracle).
- The real magnitude of snapshot funding-gaming around settlement.

---

## 12. Research Frontier

- **+10 hours.** Pull free venue funding+basis history for BTC/ETH on 2–3 venues, fix the
  interval/formula-segmentation traps, and run the **costed horse race**: no-change vs AR/DAR vs
  basis-nowcast, scored on net-of-fee directional + level error. This directly answers "is there
  incremental edge beyond no-change/carry."
- **+100 hours.** Build the **delta-neutral carry + cross-venue dispersion** backtester with
  maker/taker fee models, negative-funding-day accounting, and hold-to-convergence force-exits, across
  ≥2 full regimes (incl. a stress window). Quantify realized net edge and its tail. In parallel,
  prototype the **event-perp reference-probability index** against Polymarket data (the only
  event-microstructure data that exists) and replicate the PIRAP failure modes.
- **Dedicated team.** A relative-value desk: live basis/funding nowcast across venues, an
  execution layer that captures maker rebates, a risk gate using VPIN/OI/leverage flags, and an
  event-perp valuation research line (reference-index design, jump/resolution margin) ready for when
  the product ships.
- **Highest-leverage single question.** *Does a basis/premium nowcast beat the no-change funding
  forecast and the raw carry, net of realistic (taker) fees, on your venues and horizon?* If yes,
  build the RV signal and the carry book. If no, the edge is access/execution (maker fills, venue
  selection), not prediction — and FORTUNA's real event-perp edge is its **belief model**, not any
  funding forecast.

---

### Source appendix (accessed / verified 2026-06-18)

**Peer-reviewed / working papers (T1–T2)**
- He, Manela, Ross, von Wachter, "Fundamentals of Perpetual Futures," arXiv 2212.06888.
- "Two-Tiered Structure of Cryptocurrency Funding Rate Markets," MDPI *Mathematics* 14(2):346 (2026).
- "Temporal Dynamics of Market Microstructure in Crypto Perpetual Futures," MDPI *JRFM* 14(5):103 (2026).
- Inan, "Predictability of Funding Rates," SSRN 5576424 (2025-10) — *verified*.
- Schmeling, Schrimpf, Todorov, "Crypto Carry," BIS WP 1087 (2023, rev. 2025).
- Fan, Jiao, Lu, Tong, "Risk and Return of Cryptocurrency Carry Trade," SSRN 4666425.
- "Bitcoin wild moves: order flow toxicity and price jumps," ScienceDirect S0275531925004192 (2026).
- Gould & Bonart, "Queue Imbalance as a One-Tick-Ahead Price Predictor," arXiv 1512.03492.
- Ackerer, Hugonnier, Jermann, "Perpetual Futures Pricing," NBER w32936 / *Math. Finance* (2026).
- Nechepurenko, "Resolution-Aware Perpetual Futures on Binary Prediction Markets" (PIRAP), arXiv
  2605.10400 (2026-05-11) — *verified; non-deployable; 3/5 benchmarks fail*.
- Nechepurenko, "A Taxonomy of Event-Linked Perpetual Futures," arXiv 2605.10428 (2026-05) — *verified*.

**Official / industry (T1–T2)**
- Binance / Bybit / OKX / BitMEX / Hyperliquid / dYdX funding & API docs (2026-06).
- BitMEX Q3-2025 Derivatives Report; Amberdata; CME OpenMarkets; CFBenchmarks (2025).
- Ethena "Funding Risk" docs; LlamaRisk "Reserve Fund Drawdown Methodology V2."
- Kalshi "What Are Perpetual Futures" / "Launches Perpetual Futures in America" (2026-05/06) —
  *verified: crypto-price perps, BTCPERP live 2026-06-03*.
- Data vendors: Coinglass ($29–$699), Velo ($199), Coinalyze (free) — confirmed pricing; Tardis/Kaiko/
  Amberdata/CoinAPI/CCData/Laevitas — quote-only/unconfirmed.

**News / practitioner (T3) and folklore (T4 — flagged)**
- CNBC/CoinDesk/Axios/Fortune on Kalshi & Polymarket perps (2026); Presto Labs funding study (2024);
  Pendle/Boros cross-exchange RV; Gate/PANews arbitrage notes. ⚠ Long/short-account-ratio and
  liquidation-heatmap "edge" claims are T4 folklore with no OOS net-of-cost support.
