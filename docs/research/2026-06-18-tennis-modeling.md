# Predictive Modeling of Professional Tennis — Decision-Grade Research Memo

**Date:** 2026-06-18 · **Method:** deep-research-protocol (8-phase pipeline, 4 parallel web-grounded evidence agents + adversarial synthesis) · **For:** quant researcher + tennis statistician building a pre-match and in-match win-probability model to price markets and find edge vs the closing line (eventual FORTUNA feed; kept light here).

---

## 1. Executive Summary

Build **surface-weighted Elo first** as the rating spine, then a **Bayesian hierarchical serve/return point model** on top of it — that pairing is the empirically validated sweet spot, and only the point model can produce the *in-match* win probability the brief requires. The uncomfortable headline, replicated across every rigorous study, is that **no public tennis model durably beats the betting market**: the best academic models *tie* the closing line (~70% accuracy / ~0.196 Brier), and the market sits ~2 accuracy points and ~0.02 Brier ahead. Realistic edge therefore does **not** live in liquid ATP/WTA main draws; it lives at the *margins* — Challenger/ITF soft lines, in-play overreaction windows, and cross-book price dispersion — all of which are real but small, high-variance, and gated by execution reality (vig/commission, account limits, in-play latency). **Confidence: high (85–90%)** on the landscape and the build order; **medium (55–65%)** on whether durable, scalable edge exists after costs. The single biggest risk: a model can be beautifully calibrated and still make zero money because the market is already at least as calibrated *and* limits you the moment you're right — **the binding constraint is access, not accuracy.**

---

## 2. Research Objective

- **Topic:** Pre-match and in-match (live) professional-tennis win-probability modeling at accuracy sufficient to price markets and find edge vs the closing line.
- **Decision supported:** Which approach to build first; which features and data matter; how to validate honestly; where realistic edge is; highest-leverage next research.
- **Audience:** A senior quant + tennis statistician who will build it.
- **Time horizon:** Immediate build (1–6 months) with a 1-year edge-durability view.
- **Output:** Decision-grade memo.
- **Assumptions made to resolve ambiguity (challenge these):** (a) you can code and ingest data; (b) modest budget for paid data if justified; (c) you care about *live-line edge*, not just academic accuracy. Where (c) is false (you only want a good forecaster, not a profitable bettor), the bar drops dramatically and "edge vs market" sections become optional.

---

## 3. Perspective Analysis

**Academic — *what does formal evidence say?*** Core thesis: the field has converged — model *class* matters less than data quality and calibration, and the betting market is the benchmark. Strongest evidence: Kovalchik (2016, JQAS, **T1**) ranked 11 model families on 2,395 ATP matches; Wilkens (2021, J. Sports Analytics, **T1**) on ~39k ATP+WTA matches. Hidden assumption: historical Brier/accuracy generalize forward. Blind spot: publication bias toward novelty (new methods get published when they "win"). Unique insight: the iid-points assumption is formally false (Klaassen & Magnus 2001) yet "good enough" for forecasting.

**Practitioner — *what matters in the real world?*** Core thesis: on paper anything hits ~66–70%; in practice the constraints are missing/limited public serve stats, retirements/walkovers, and that the closing line already prices public information. Unique insight: the only positive ROI in the literature is earned from **price dispersion + favourite-longshot effects** ("best available odds"), not from a smarter probability.

**Skeptic — *assume the consensus is wrong.*** Core thesis: most "we beat the bookmaker" claims are leakage, survivorship, or odds-as-a-feature artifacts. What would break the consensus: a model showing **sustained positive closing-line value (CLV)** against Pinnacle/Betfair on a large held-out sample — which the literature has never produced. Strongest evidence: Gao & Kowalczyk's 83% "market-beating" RF is driven by in-match serve features (target leakage).

**Economist — *follow the incentives.*** Core thesis: the market is near-efficient because sharp money corrects it and books absorb informed flow into the price (Pinnacle/Betfair model); the model's job is the residual. Critical second point: the venues that *tolerate* winners (Betfair) claw edge back via commission and Premium Charge; the venues with soft lines (soft books, Challenger/ITF) **limit/ban you fastest**.

**Engineer — *can it actually work?*** Core thesis: a pre-match pipeline is fully buildable on free data; a *live* pipeline is gated by paid low-latency feeds and an unavoidable latency disadvantage. Single point of failure for live models: **mid-match retirements** (an exogenous, discontinuous jump the scoring model is structurally blind to) and **feed latency** (you are adversely selected by anyone faster to the score).

*Lenses lightly weighted:* Historian (precedent = horse-racing/efficient-market betting history, folded into Economist), Regulator (courtsiding legality, folded into Engineer/Data), Futurist (GNN/tracking-data frontier, folded into Research Frontier).

---

## 4. Evidence Matrix

| # | Claim | Supporting evidence | Contradicting evidence | Source | Type | Date | Tier | Conf. | Quality |
|---|-------|--------------------|------------------------|--------|------|------|------|-------|---------|
| 1 | In the canonical head-to-head, the bookmaker consensus won every metric (72% acc, 0.55 log-loss); 538 Elo was best non-market model (70%/0.59); **no model beat the market**. | 11 models, 2,395 ATP matches, 2014 | — (top-line independently corroborated) | Kovalchik, "Searching for the GOAT of tennis win prediction," JQAS 12(3) | peer-reviewed | 2016 | T1 | High | High |
| 2 | ML accuracy ceiling ≈70%; ML does **not** beat odds-implied forecasts; odds are the single most informative feature; betting returns volatile/mostly negative. | 25,204 ATP + 13,755 WTA matches 2010–19; RF/SVM/ANN ≈69–70% = odds | — | Wilkens, J. Sports Analytics 7(2) | peer-reviewed | 2021 | T1 | High | High |
| 3 | Market benchmark to beat ≈ **69% acc / 0.196 Brier**; std Elo 65.8%/0.215, Weighted Elo 66.4%/0.212 — Pinnacle superior on all surfaces. | 8,375 matches 2023–25 | — | "Intransitive Player Dominance…GNN," arXiv 2510.20454 | preprint | 2025 | T2 | Med-High | Med |
| 4 | Best published model only **ties** the bookmaker on Slams: RF 0.7641 acc vs bookmaker 0.7653; bookmaker 0.196 Brier beats WElo (0.212) and Elo (0.215). | VU thesis backtest | — | Dryja, "Data-Driven Prediction of ATP…," VU Amsterdam | thesis | ~2022 | T2 | High | Med-High |
| 5 | "ML smashes the bookmaker" 83% RF is **target leakage** — top features are in-match serve outcomes; its probabilistic score is *worse* than the odds. | Paper's own feature list; rank/surface/H2H found "irrelevant" (leakage signature) | Authors present it as a real edge | Gao & Kowalczyk, J. Sports Analytics 7(4) / arXiv 1910.03203 | peer-reviewed (flagged) | 2021 | T1 | High (that it's leaky) | High |
| 6 | A Bayesian hierarchical serve/return point model **beats the point-based baseline** (+2.5 acc, −0.049 log-loss) and roughly **ties Elo** (68.8% vs 538 Elo 69.5%), while adding in-play/any-score win prob. | 2014 ATP out-of-sample | Does not *beat* Elo on accuracy | Ingram, "A point-based Bayesian hierarchical model…," JQAS 15(4) | peer-reviewed | 2019 | T1 | High | High |
| 7 | Surface adjustment helps **~+0.5–2 acc pts**, concentrated on grass/clay; a **50/50 blend** of overall+surface Elo beats pure single-surface ratings everywhere. | ~50k ATP matches: grass +2.2, clay +0.9, hard +0.6 | Hard courts (most of tour) gain ≈0 | Sackmann, Tennis Abstract; Gorgi-Koopman-Lit, JRSS-A 182(4) 2019 (T1) | blog + peer-reviewed | 2017/2019 | T1/T3 | High | High |
| 8 | Best-of-5 systematically raises the favourite's win prob vs Bo3, **peaking ~+5 pts** for clear-but-competitive favourites. | Set-favourite→match conversion math; ATP Bo3 upset ~29% vs Bo5 ~24% | — (math-forced) | Newton & Keller, Studies in Applied Math 2005 (T1) | peer-reviewed | 2005 | T1 | High | High |
| 9 | Points are **non-iid** (prev-point + big-point effects) but deviations are small → iid is a good forecasting approximation; relaxing it buys ~1 pt. | ~90k Wimbledon points; "7th game" & "new balls" are myths | Pollard 2006, Kovalchik-Ingram 2016 find real set-to-set dependence | Klaassen & Magnus, JASA 96(454) | peer-reviewed | 2001 | T1 | High | High |
| 10 | **Momentum is contested**: the largest, best-measured tennis effect (+7% next-point, +15% at deuce) is **strategic effort allocation**, not psychological — and already priced by in-play markets. | Hawk-Eye 3,000+ matches | Miller-Sanjurjo 2018 bias correction rehabilitates *small* hot hand; ML papers claim 84–99.9% (circular/leaky) | Gauriot & Page, Economic Journal 2019 (T1) | peer-reviewed | 2019 | T1 | High (on strategic) | High |
| 11 | Mid-match **retirements end ~2.5–3.3% of matches** (ATP 3.30%, WTA 2.73%), cluster right after set 1; standard in-play models have **no retirement hazard term**. | 851k+ matches | — | Palau et al., PLOS One 2024 (T1); Breznik & Batagelj 2012 (T1) | peer-reviewed | 2024 | T1 | High | High |
| 12 | **Head-to-head adds ~no signal** beyond ratings for winner-picking (ranking right 56.5% when it disagrees with H2H). | Sackmann; Kovalchik 2016 | Narrow exception: intransitive matchups may be market-mispriced (+3.26% ROI niche, preprint) | Sackmann, Tennis Abstract; arXiv 2510.20454 | blog + preprint | 2014/2025 | T3/T2 | Med-High | Med |
| 13 | **CLV (did you beat the closing price?) is the gold-standard edge test** — proves edge in ~50–100 bets vs thousands for raw ROI. | Variance argument; "Wisdom of Crowd" 20k bets, 3.4% actual vs 4.0% expected | Not all genuine edge shows immediate CLV | Buchdahl, via Pinnacle Odds Dropper | expert blog | ~2021 | T3 | High | Med-High |
| 14 | **Even a genuinely winning strategy gets shut down**: real-money +8.5% over 5 months, then accounts severely limited. | 265 bets, +$957; replicated narrative | — (this is the robust adversarial finding) | Kaunitz, Zhong & Kreiner, "Beating the bookies…" 2017 (T1); MIT Tech Review | paper + news | 2017 | T1/T3 | High | High |
| 15 | **Free data suffices for pre-match + market backtest**; live in-play **requires paid feeds**. Sackmann tennis_atp/wta (results 1968+, serve stats 1991+, CC-BY-NC-SA), MCP point-by-point (5,000+ matches), tennis-data.co.uk (Bet365+Pinnacle near-close odds, ATP 2000+/WTA 2007+). | Repos/sites fetched directly | tennis-data odds are near-close, NOT certified closing (need Betfair BSP) | Sackmann GitHub/blog; tennis-data.co.uk notes.txt | primary docs | 2015–26 | T1 | High | High |
| 16 | Favourite-longshot bias is real but **mostly a margin artifact**, not free money; the exploitable residue sits at the low-ranked/low-tier end (where liquidity/limits hurt). | Forrest & McHale; Erasmus thesis; Betfair-exchange FLB study | — | Forrest & McHale 2007 (T1); Abinzano et al. 2016 (T1) | peer-reviewed | 2007–16 | T1 | Med-High | Med |

*No row rests primarily on Tier-4 evidence. The two weakest threads — a self-reported tipster's "win without CLV" claim and courtsider income anecdotes — are explicitly T4 and excluded from load-bearing conclusions.*

---

## 5. Contradiction Map

**Major — "ML beats the market" vs "the market is the ceiling."** The headline market-beating result (Gao & Kowalczyk, 83%, #5) conflicts with the dominant finding (#1, #2, #3, #4). **Resolved:** the 83% rests on in-match serve statistics used as predictors — the variables that encode who won — and the paper's own calibration score is worse than the odds. The contradiction dissolves into a methodology artifact. *What would genuinely resolve the broader question:* a large-sample, strictly time-gated model showing **positive CLV vs the closing line** — absent from the literature.

**Major — in-play is exploitable (overreaction) vs in-play is structurally tilted against you (latency).** Bettors demonstrably overreact to recent points (arXiv 2202.10085), creating mispricing — but the official feed runs seconds behind the court and the fastest party picks off everyone slower (courtsiding economics; Betfair's 1–9s in-play delay). **Both true:** the inefficiency is real but the window is short and contested; capturing it needs speed a newcomer likely lacks. This is the central tension for any live-trading ambition.

**Minor — surface-Elo vs ML vs market ordering.** Bunker et al. (2024) found ML edged Elo and *matched* odds; Kovalchik/Wilkens found Elo ≈ best non-market model and ML ≈ odds. **Resolved:** they agree on the top line (best model only *ties* the market); the ML-vs-Elo ordering is dataset/era noise. Treat ML and Elo as roughly interchangeable.

**Minor — does the *specific* common-opponent model add edge?** Knottenbelt et al. reported +3.8% ROI (2011); Kovalchik's independent 2014 test scored it 63% — identical to naive iid and *behind* simpler opponent-adjusted Barnett-Clarke. **Resolved:** opponent *adjustment* helps; the specific common-opponent formulation is data-sparse and does not generalize.

**Open questions / unknowns:** Does any public model beat the *closing* line? How much of reported ROI survives against a single sharp book vs best-of-N book-shopping? Is WTA systematically less efficiently priced (a real edge) or just thinner and noisier? In-play Brier/log-loss vs live markets is genuinely under-benchmarked.

**Consensus (what everyone agrees on):** (a) ~70% accuracy / ~0.196 Brier ceiling for field-wide pre-match prediction; (b) the bookmaker consensus is the best single "model"; (c) surface-weighted Elo is the strongest *simple* non-market model and is calibration-competitive; (d) calibration > raw accuracy for betting; (e) exotic features beyond rating/serve/surface/format add little; (f) ROI is an unstable model-selection metric — use log-loss.

---

## 6. Blind Spot Report

- **Stakeholders under-weighted:** WTA (most headline numbers are ATP men's — the women's game is higher-variance, less-modeled, possibly less-efficiently priced); Challenger/ITF tiers (where edge plausibly lives but data is thin and limits are tightest).
- **Hidden incentives:** Pinnacle's "we're efficient" whitepaper benefits Pinnacle; betting-syndicate and bookmaker models are black boxes — the public literature is structurally a few steps behind the sharpest private models.
- **Unchallenged assumptions:** that historical-odds backtests proxy the *closing* line (they usually don't — tennis-data.co.uk is near-close, not BSP); that accuracy implies edge (it doesn't if the market is equally calibrated); that backtested ROI implies bankable profit (it doesn't, after limits/commission/slippage).
- **Topics skipped / light:** doubles (essentially unmodeled publicly); match-fixing/integrity signals in low tiers (a confound *and* a risk); tax/legal/KYC realities of operating accounts; the FORTUNA execution layer itself (out of scope by request).
- **Data that was unavailable / uncheckable:** all enterprise feed pricing (Sportradar/Genius/Betfair Advanced-Pro are opaque; $500–$10k+/mo figures are third-party guesses); Hawk-Eye tracking data (owned by tennis properties, **not licensed to outsiders at any price** — MCP is the only partial free proxy); precise official-feed latency SLA (only "ultra-low latency" qualitatively); exact current MCP match count and its coverage bias (undocumented).

---

## 7. Red Team Assessment

**Attack 1 — "You're too quick to crown the market; most studies benchmark against *historical* odds, so the market might be even harder, or the comparison unfair."**
*Logic:* Comparisons rarely test the true closing line; CLV is almost never reported. *Impact:* If true, it *strengthens* the cautious conclusion (market even harder to beat), not weakens it. *Response:* Concede the gap — it makes "find edge vs the close" harder, not easier. *Residual risk:* the literature may *understate* how hard the close is; plan accordingly.

**Attack 2 — "Angelini's Weighted-Elo and Sipko's NN show positive ROI (~3.5–4.4%) — you *can* beat the market."**
*Logic:* Published, peer-reviewed positive returns exist. *Impact:* Would refute "no edge." *Response:* Both require betting at the *best available* odds across many books — they exploit cross-book dispersion and favourite-longshot effects, not superior probability vs a single sharp close; against Pinnacle's close the edge likely vanishes, and +3–4% is inside the vig/commission/slippage band. *Residual risk:* moderate — price-shopping is a real (if fragile, rapidly-arbitraged, limit-prone) edge; worth testing, not assuming.

**Attack 3 — "A point-based model is more work than Elo for the same pre-match accuracy — just ship Elo."**
*Logic:* Ingram shows the hierarchical model only ties Elo pre-match. *Impact:* Would simplify the build. *Response:* Correct *for pre-match alone* — but the brief explicitly wants **in-match** win probability, match-length distributions, and interpretable serve/return skills, none of which Elo can produce. The point model is the deliverable; Elo is its input. *Residual risk:* low — but if in-match is later descoped, Elo-only is the right call.

**Attack 4 — "Retirements/non-stationarity/tanking will quietly wreck any live model."**
*Logic:* ~3% of matches end in retirement, clustering right after set 1 exactly where exchange void-protection ends; players tank dead rubbers and rest before Slams. *Impact:* Blows up in-play positions and pollutes training data. *Response:* Real and serious — must be modeled explicitly (surface-conditioned retirement hazard ~1.5/1000 games, settlement-cliff term at set boundaries, exposure caps near set completion) and motivation/withdrawal flags filtered from training. *Residual risk:* high if ignored, manageable if engineered in from day one.

**Attack 5 — "You get limited/banned where you're right (Kaunitz), so even a real edge doesn't scale."**
*Logic:* The most robust empirical finding in the whole space. *Impact:* Caps realizable profit regardless of model quality. *Response:* No rebuttal — it's true. The implication is to model *capacity* honestly (Betfair-centric, expect Premium Charge; treat soft-book edge as small and short-lived) and to value the system as a calibrated *pricing* engine first, a *profit* engine second. *Residual risk:* this is the core risk to the entire profit thesis.

---

## 8. Key Findings

1. **The market is the un-beaten benchmark; the best public models only tie it.** Gap ≈ 2 accuracy points / ~0.02 Brier (market ~0.196 vs Elo ~0.212–0.215). (#1–#4) — *So what:* design to beat the *closing line in CLV*, not to maximize accuracy; if you can't beat the close, don't trade the liquid markets.
2. **Build order is settled by the evidence: surface-weighted Elo spine → Bayesian hierarchical serve/return point model.** Elo is the best simple non-market base rate; the point model ties it pre-match *and* uniquely unlocks in-match win probability, and can even ingest Elo-induced serve probabilities. (#6, #7) — *So what:* this is your "build first."
3. **A short, high-value feature set dominates; most of the rest is folklore.** Real signal: serve-point-win *differential* (the core driver), surface (blend, don't replace), format (Bo3/Bo5, ~+5 pts), opponent-adjustment of serve/return stats, and an explicit retirement hazard for in-play. Overrated: fatigue/rest, exploitable momentum, H2H, age/handedness/height as separate features (already inside Elo). (#7–#12) — *So what:* spend effort on the five that matter; ignore the folklore.
4. **Free data is enough to build and market-backtest a pre-match model; live in-play is a different, paid, latency-disadvantaged regime.** (#15) — *So what:* start free; only buy feeds once a pre-match CLV edge is proven and an in-match product is actually scoped.
5. **Edge, if any, is at the margins and capped by access, not accuracy.** Candidates: Challenger/ITF soft lines, in-play overreaction, cross-book dispersion, possibly WTA. All are small, high-variance, and limited fast (Kaunitz). (#14, #16) — *So what:* treat realizable edge as a capacity-constrained, fragile thing; validate it lives *after* commission/limits before believing it.

---

## 9. Recommendations (ordered by leverage)

1. **Build the surface-weighted Elo spine first (≈1–2 weeks).** 538-style career Elo + margin/score-weighted updates (Angelini WElo) + 50/50 surface blend. Reproduce the literature benchmark (~70% acc / ~0.21 Brier, calibrated) as your acceptance test. *Rests on #1, #3, #7.* This is the cheapest path to a trustworthy base rate and the input to everything else.
2. **Build the Bayesian hierarchical serve/return point model on top of it (the real deliverable).** Estimate per-player serve and return point-win skills (opponent- and surface-adjusted), feed in **Elo-induced serve probabilities** so it inherits Elo's accuracy, and run the point→game→set→match recursion for **both** pre-match and **any-score in-match** win probability. *Rests on #6.* This is what Elo cannot do and what FORTUNA needs.
3. **Validate on CLV and log-loss, never on backtested ROI.** Devigged-closing-prob minus your entry prob, tracked per segment (tier × surface × tour × pre/in-play); promote only on **persistent positive CLV over ≥50–100 bets per segment**. Always benchmark Brier/log-loss + a reliability diagram **against the devigged market**, not a coin flip. *Rests on #4, #13.* (Note: this maps directly onto FORTUNA's I7 forward-validation discipline.)
4. **Engineer the in-match failure modes in from day one.** Surface-conditioned retirement hazard (~1.5/1000 games, clay/hard ≈2× grass), a settlement-cliff term at set boundaries, exposure caps near set completion, and motivation/withdrawal/tanking filters on training data. *Rests on #11.* Skipping this is how live models blow up.
5. **Guard against the leakage trap and the ROI-overfit trap.** Strictly time-gate every feature (never let the predicted match's own in-play stats leak into a pre-match model — the Gao/Kowalczyk failure); select models on log-loss, not ROI. *Rests on #5, #9.*
6. **Hunt edge only at the margins, and prove it survives costs.** Probe Challenger/ITF mispricing, in-play overreaction, cross-book best-available-odds, and a WTA-specific calibration study — but subtract vig/commission (~2–5%) and model account-limit capacity *before* believing any edge. *Rests on #14, #16.* Treat any apparent edge on liquid main-draw closing lines as spurious until CLV says otherwise.

---

## 10. Confidence Scores

| Finding | Conf. | Reasoning | Weaknesses | Raises confidence | Lowers confidence |
|---------|-------|-----------|------------|-------------------|-------------------|
| Market is the un-beaten benchmark; best models tie it | **88%** | Replicated across 4+ independent T1 studies | Few test the true *closing* line | A large CLV-tested study | A credible sustained-positive-CLV model |
| Build order: surface-Elo spine → hierarchical serve/return point model | **85%** | Ingram + Kovalchik + the in-match requirement converge | "Ties Elo" pre-match makes it look optional | A clean in-match benchmark showing the point model's edge | If in-match is descoped, Elo-only wins |
| ~70% acc / ~0.196 Brier field-wide ceiling | **90%** | Robust across studies and eras | Inflated on easy (top-30) subsets | More multi-era replications | Measuring only on liquid favourites |
| Short feature set dominates; rest is folklore | **80%** | Strong on format/surface/serve/H2H/retirement; momentum contested | Momentum/intransitivity may hide small real edges | Bias-corrected (Miller-Sanjurjo) tennis momentum test | A clean exploitable-momentum or H2H result |
| Free data suffices for pre-match; live needs paid feeds | **90%** | Repos/sites fetched directly; licensing explicit | Live-feed costs opaque | — | A cheap official low-latency feed appearing |
| Durable, scalable edge exists after costs | **58%** | FLB/dispersion/in-play edges are real but small | Limits (Kaunitz), commission, latency, slippage | A proven post-cost CLV segment | Faster confirmation that limits cap capacity |
| Retirements/non-stationarity are a top live-model risk | **85%** | T1 base rates + structural model blindness | Magnitude varies by venue rules | — | Venue auto-void rules more protective than assumed |

---

## 11. Open Questions

**Blocks the decision (must answer before risking capital):**
- Does the model beat the **closing line (positive CLV)** on a large held-out sample, in *which* segments (tier × surface × tour × pre/in-play)? Everything downstream hinges on this.
- After vig/commission *and* realistic account limits, what is the **capacity** of any edge segment — is it a hobby or a business?

**Good to know (shapes the build, doesn't block it):**
- How much of reported ROI survives vs a *single sharp book* rather than best-of-N shopping?
- Is WTA systematically less-efficiently priced, or just thinner and noisier?
- How much does momentum/point-importance modeling actually add to in-match calibration vs an iid point core (and does the Miller-Sanjurjo bias correction, never yet applied to tennis, change the answer)?
- Can the intransitive-matchup (GNN) niche edge be reproduced out-of-sample, or is it a preprint artifact?

---

## 12. Research Frontier

- **+10 hours:** Pull the exact per-model Brier/log-loss tables from Kovalchik, Wilkens, Ingram, and Angelini; lock down the precise field-wide vs top-30 accuracy split; finalize the data-ingestion plan (Sackmann + tennis-data.co.uk + MCP) and the segment grid.
- **+100 hours:** Build the reproducible backtest the literature keeps skipping — Sackmann results + serve stats + tennis-data.co.uk odds — that measures **CLV** (ideally vs Betfair BSP) for surface-Elo, WElo, and an XGBoost baseline, **stratified by tier/surface/tour**. Then build the hierarchical serve/return point model and benchmark its in-match calibration. Replicate the Gao/Kowalczyk pipeline with strict time-gating to demonstrate the accuracy collapse from ~83% to ~66% (a cheap, convincing leakage check).
- **Dedicated team:** Live in-play system — point-based Markov core + dynamic Bayesian updating (Kovalchik-Reid) + retirement hazard + momentum/leverage terms — benchmarked against live exchange odds, with an honest latency and capacity model; plus a WTA-specific calibration and edge study; plus an investigation of tracking-data (Hawk-Eye/MCP) features for any signal the public rating misses.
- **Highest-leverage single question:** ***Can the model beat the closing line (positive CLV), in which segments, and at what capacity after costs?*** It determines whether to trade at all, where, and how much — and it is the exact gap the existing literature most consistently leaves open.

---

### Source appendix (accessed 2026-06-18)
Kovalchik 2016 JQAS (vuir.vu.edu.au/34652) · Wilkens 2021 J. Sports Analytics (JSA-200463) · Gao & Kowalczyk 2021 (arXiv 1910.03203) · Angelini-Candila-De Angelis 2022 EJOR (S0377221721003234) · Williams/Liu et al. 2021 JQAS (jqas-2019-0110) · Ingram 2019 JQAS (martiningram.github.io/papers/bayes_point_based.pdf) · Sipko/Knottenbelt 2015 Imperial thesis · Cornman/Spellman/Wright 2017 Stanford CS229 · Kovalchik & Reid 2019 Int. J. Forecasting (S0169207017301395) · Klaassen & Magnus 2001 JASA / 2003 EJOR (janmagnus.nl/papers/JRM065.pdf) · O'Malley 2008 JQAS · Newton & Keller 2005 / Newton & Aslam 2006 SIAM Review · Gorgi-Koopman-Lit 2019 JRSS-A · Bunker et al. 2024 Proc. IMechE Part P · Gauriot & Page 2019 Economic Journal · Miller & Sanjurjo 2018 Econometrica · Palau et al. 2024 PLOS One · Breznik & Batagelj 2012 J. Sports Sci. Med. · Knottenbelt-Spanias-Madurska 2012 Comput. Math. Appl. (S0898122112002106) · Dryja VU thesis · "Intransitive…GNN" 2025 (arXiv 2510.20454) · "Bettors' reaction to match dynamics" (arXiv 2202.10085) · Kaunitz-Zhong-Kreiner 2017 + MIT Tech Review · Buchdahl "CLV demystified" (Pinnacle Odds Dropper) · Forrest & McHale 2007 (MPRA 47905) · Abinzano-Muga-Santamaria 2016 Applied Economics Letters · Pinnacle ATP-efficiency study · Sackmann tennis_atp/tennis_wta/tennis_MatchChartingProject (GitHub, CC-BY-NC-SA) · tennis-data.co.uk (notes.txt) · ultimatetennisstatistics.com / mcekovic/tennis-crystal-ball · oncourt.info · Sportradar/Stats Perform/Hawk-Eye/IBM rights (Sportico, SportsPro, WTA, CNBC, IBM Newsroom) · Goalserve tennis API · Betfair historical data · Wikipedia "Courtsiding" + Bet Angel in-play delay.

*Caveats carried from evidence collection: several publisher PDFs were paywalled (numbers from those flagged at slightly lower confidence); enterprise feed prices are opaque guesses; three prompt premises were corrected with sources — Sportradar was NOT the historical ATP distributor (IMG Arena was, 2020–23; Sportradar won 2024–29 via TDI), the WTA partner is Stats Perform (to 2030), and IBM is an analytics partner, not a data-rights seller.*
