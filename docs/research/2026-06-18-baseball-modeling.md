# Predictive Modeling of MLB — Decision-Grade Research Memo

**Date:** 2026-06-18 · **Method:** deep-research-protocol (8-phase pipeline, 6 parallel web-grounded evidence agents + adversarial synthesis) · **For:** quant researcher + sabermetrician building a game-outcome and player-prop model (pitcher strikeouts, total bases, hits) to price markets and find edge vs the closing line (eventual FORTUNA feed; kept light here).

---

## 1. Executive Summary

Build **pitcher-strikeout props first**, as the first validated output of a **single-game Monte Carlo engine** (lineup → plate appearances → per-PA outcome via log5), and score it on **closing-line value (CLV), not win/loss**. Strikeouts are the most modelable target in baseball: K-rate stabilizes faster than any outcome stat (~60 PA for a hitter, ~70 batters-faced for a pitcher), the predictive features (SwStr%, CSW%) accumulate per-pitch and out-predict naïve past-K% early in a season, and Ks are high-count, skill-driven, low-luck, and largely independent of who wins the game. The uncomfortable headline — replicated across every rigorous source — is that **game-side markets are efficient and structurally hard to beat**: the practical accuracy ceiling on an MLB game is ~60–65%, the most sophisticated matchup models add only ~**one win per 162-game season** over simple ones, and the market already sits at that ceiling. Edge therefore does **not** live in moneylines; it lives in **softer, capacity-capped corners** — player props (median-priced, often outsourced by books) and the mispriceable inputs of totals/first-5 (umpire, weather, confirmed lineup/leash). Those edges are real but **small, high-vig (6–10% on props), and limit-capped ($250–500)**. **Confidence: high (85–90%)** on the build order, features, and validation method; **medium (50–60%)** on whether durable, scalable, after-cost edge actually exists once books limit you and prediction-market MLB liquidity is this thin. The single biggest risk is identical to the tennis track [[deuce-tennis-modeling]]: **a perfectly calibrated model can still make zero money because the market is already calibrated and caps your size the moment you're right — the binding constraint is access and capacity, not accuracy.**

---

## 2. Research Objective

- **Topic:** MLB game-outcome win probability and player props (pitcher Ks, total bases, hits) at accuracy sufficient to price markets and beat the closing line.
- **Decision supported:** Which target/model to build first; which features and data matter; how to validate honestly; the realistic edge; the highest-leverage next research.
- **Audience:** A senior quant + sabermetrician who will build it.
- **Time horizon:** Immediate build (1–6 months) with a 1-year edge-durability view.
- **Output:** Decision-grade memo.
- **Assumptions made to resolve ambiguity (challenge these):** (a) you can code and ingest data; (b) modest budget for paid data/odds if justified; (c) you care about *live-line edge*, not just academic accuracy; (d) the eventual venue is prediction-market style (Kalshi/Polymarket) and/or sportsbook props. Where (c) is false (you only want a good forecaster, not a profitable bettor), the bar drops and the edge sections become optional.

---

## 3. Perspective Analysis

**Academic — *what does formal evidence say?*** Core thesis: MLB single games are near-coin-flips, so model *class* matters far less than calibration, and the market is the benchmark. Strongest evidence: ML game-winner models cluster at **55–65% accuracy and "never exceeded 58%"** even with most of a season as training data (arXiv 2410.21484 review, 2024, **T2**; NCBI PMC8871522, **T1**); the most sophisticated hierarchical-Bayesian matchup model adds "**as much as one additional victory per 162-game season**" over simpler models (Mott et al., arXiv 2511.17733, 2025, **T1-preprint**). Hidden assumption: historical Brier/accuracy generalize forward through rule changes. Blind spot: novelty/publication bias (new methods get written up when they "win"). Unique insight: the predictable *fraction* of a game is small by construction — best teams win ~.600 — so a Brier near the ~0.24 coin-flip floor is the ceiling, not a failure.

**Practitioner — *what matters in the real world?*** Core thesis: on paper any decent pipeline prices a game close to market; in practice the constraints are **projected innings/leash** (a high-K pitcher pulled at 5 IP busts the over), **confirmed lineups** (post ~2–4h pre-game, with no clean "confirmed" flag in the feed), and **scratch/void risk**. Unique insight: the most modelable prop and the one with the cleanest feature set is **pitcher strikeouts**; hitter props (TB/hits) hinge on BABIP luck that doesn't stabilize within a single game.

**Skeptic — *assume the consensus is wrong.*** Core thesis: most "my model beats the line" claims are vig-blindness, leakage (using closing lines or final lineups as features), or regime-overfit. What would break the skeptic case: a strictly time-gated model showing **sustained positive CLV vs the de-vigged closing line** over ≥1,000 bets — which no public MLB source has shown. Strongest evidence: even good ML predictions lose money under naïve staking ("naïve use leads to significant losses," Allen & Savala, arXiv 2511.02815, 2025, **T1-preprint**); Statcast "expected" stats add **no meaningful predictive edge for pitchers** over FIP/DRA (Judge, BP "Siren Song of Statcast's Expected Metrics," **T2/T3**).

**Economist — *follow the incentives.*** Core thesis: the closing line is sharp because books absorb informed flow into the price; the model's job is the residual. Critical second point: the venues with **soft prop lines limit/ban winners fastest** (a winning prop bettor cut from $200 to $20/bet, Action Network, **T2**), and books **outsource props to Swish Analytics / price off the median** rather than finely shaping them (Unabated, **T2**) — so the inefficiency is real but the capacity to exploit it is deliberately throttled.

**Engineer — *can it actually work?*** Core thesis: a **pre-game** prop pipeline is fully buildable on the cheap stack; the moment you need **sub-second live** data or commercial-clean licensing, you hit a wall. Single points of failure: **latency** (public StatsAPI is ~12s; Savant pitch CSV is *next-morning*), **lineup confirmation** (no first-class flag — inferred from a populated `battingOrder`), and **regime breaks** (2020 Hawk-Eye, 2023 rules, 2025–26 ABS challenge live in the regular season, which structurally shrinks catcher-framing value). Enabling fact: **pitcher-K props are a pre-game market, so next-morning Statcast + ~12s StatsAPI is *enough*** — you don't need Sportradar to start.

*Lenses lightly weighted:* **Historian** (precedent = horse-racing/efficient-market betting and the tennis track's "market is the ceiling" finding — folded into Economist), **Regulator** (MLBAM non-commercial terms, Kalshi data ToS forbidding use of its data to "develop financial instruments," post-2025 Cleveland-scandal pitch-prop limits — folded into Data/Blind-spots), **Futurist** (ABS full-automation, ML prop arms race — folded into Research Frontier).

---

## 4. Evidence Matrix

| # | Claim | Supporting evidence | Contradicting evidence | Source | Type | Date | Tier | Conf. | Quality |
|---|-------|--------------------|------------------------|--------|------|------|------|-------|---------|
| 1 | Sophisticated systems beat a trivial **Marcel** baseline by only **~4–6%** on aggregate rate stats, and Marcel often beats PECOTA/ZiPS outright. wOBA RMSE: PECOTA .0317 vs Marcel .0330 (2011). | FanGraphs projection-accuracy tests; BtBS grading 2015/2016 | "best system" is era-dependent (Steamer 2011–16; ATC/THE BAT X 2019–24) | FanGraphs (Swartz 2/2012); Beyond the Box Score (Druschel 1/2016–1/2017) | quality blog | 2012–17 | T3 | High | Med-High |
| 2 | Practical MLB **game-winner accuracy ceiling ≈ 60–65%**; "never exceeded 58%" in one review; best teams win ~.600 so most games sit in [.40,.60]. | XGBoost ~55.5%, SVM ~60%, logit 61.77% | refinements reach 64–65% with pitcher covariates | arXiv 2410.21484 (T2); Cui, Wharton thesis (T1-acad, 2020) | review + thesis | 2020–24 | T1/T2 | High | High |
| 3 | The most sophisticated matchup model adds **~1 win / 162 games** over simpler ones and "aligns with market expectations." | Hierarchical-Bayesian PA model | — (a stark ceiling signal) | Mott et al., arXiv 2511.17733 | preprint | 2025 | T1 | High | Med-High |
| 4 | MLB game markets are **broadly efficient**; closing line is the sharp benchmark; documented edges are **operational (speed/stale-line), not structural**. Reverse favorite-longshot bias. | Woodland & Woodland, *J. Finance* | newer work claims rejectable weak-form efficiency + exploitable longshot bias | Woodland & Woodland 1994 (T1); ECU/ResearchGate (T2) | peer-reviewed | 1994/recent | T1 | High | High |
| 5 | **CLV is the gold-standard edge test**: ~**1:1** CLV→ROI vs the de-vigged close; detects skill **~200× faster** than P/L; significance in as few as ~50 bets. | Worked example: 1,214 bets, +2.19% CLV → 5.73% profit, ~18.5 SD | "2% CLV → 4% ROI" and "Pinnacle study" claims are uncited (flagged) | bet2invest/Buchdahl (T2); Pinnacle (T2/3) | expert | ~2021–25 | T2 | High | Med-High |
| 6 | **Stabilization** (split-half r≈0.7): hitter K% **60 PA**, BB% 120, **BABIP ~820 BIP**, AVG 910 AB; pitcher K% **70 BF**, BB% 170, BABIP **2000 BIP**; SwStr% ~**400 pitches**; avg EV ~**50 batted balls**. | Carleton/BP; FanGraphs Library (Slowinski, upd. 8/2017) | stabilization = retrospective *reliability*, not predictive power (Carleton's own caveat) | Baseball Prospectus; FanGraphs Library | expert/primary | 2013–17 | T2 | High | High |
| 7 | **SwStr%/CSW% out-predict past K% early-season** and stabilize fast (CSW% ~10 starts; R²≈0.59–0.67 vs K%). SwStr%↔K% r≈0.85. | Pitcher List; AZ Snake Pit (2022) | CSW% vs SwStr%-alone for pure K prediction is disputed | Pitcher List (T2); RotoGraphs (T3) | quality blog | 2019–26 | T2/T3 | High | Med-High |
| 8 | **xwOBA is mostly descriptive, weakly predictive across a half-season** (1st-half xwOBA → 2nd-half wOBA r≈0.41 hitters, 0.35 pitchers); for pitchers it adds **no edge over FIP/DRA**. | RotoGraphs in-season study; Judge "Siren Song" | one hitter study gives xwOBA a *small* edge over wOBA | FanGraphs/RotoGraphs; BP (Judge) | quality blog | ~2018–20 | T2/T3 | High | Med-High |
| 9 | **K = projected K% × batters-faced**, opponent-adjusted via **log5** (`p ≈ pitcher-K% × team-K% / league-K%`); **projected innings/leash is the dominant variance driver**; per-batter **beta-binomial** beats curve-fitting Poisson/NB. | FanGraphs Library; Healey generalized-log5 (JSA 2015, T1) | no T1 study fits Poisson-vs-NB to pitcher-K counts directly | FanGraphs (T2); Healey (T1) | library + peer-reviewed | 2015–26 | T1/T2 | High | Med-High |
| 10 | **Catcher framing** ≈ **+3.9% K / −3.9% BB per framing run/game** (elite framer ~+30 runs/season); umpire zone and park measurably move K totals; weather effect small/ambiguous. | FanGraphs framing regression (3/2019); Mike Fast 2011 | ABS challenge (2026) structurally shrinks framing value going forward | FanGraphs; SIS | expert | 2011–19 | T2 | High | Med-High |
| 11 | **Prop economics:** hold **6–10%+** (vs ~4.5% on sides), limits **$250–500** (vs $10k+ sides), books **price props off the median / outsource to Swish**; props are softer but capacity-capped. SGP hold 15–25%+. | Wizard of Odds; Unabated; Action Network | "props are soft" is strong T2 consensus, not an academic fact | Wizard of Odds (T2); Unabated (T2) | expert | 2024–26 | T2 | High | Med-High |
| 12 | **Hits/TB are materially harder:** within-game BABIP is near-noise (820-BIP threshold), PA depends on lineup slot (1st 4.65 → 9th 3.77 PA/game), TB lines sit on lumpy discrete jumps, and a non-start **voids** the bet. | FanGraphs; RotoGraphs PA-by-slot | — (convergent) | FanGraphs Library; RotoGraphs; Action Network | expert/blog | 2016–26 | T2/T3 | High | High |
| 13 | **Data latency + licensing are the entangled binding constraint.** Public StatsAPI ~**12s**; Savant pitch CSV **next-morning** + revised; MLBAM terms permit "non-commercial, non-bulk" only; sub-second + legally-clean exists only via **Sportradar** (MLB exclusive through 2032). | gdx copyright.txt (live); MLBAM GUMBO; Sportradar/MLB PR (2/2025) | facts aren't copyrightable (NBA v. Motorola) — leverage is contract/ToS | MLB/MLBAM primary; GlobeNewswire | primary | 2025–26 | T1 | High | High |
| 14 | **pybaseball is semi-dormant** (last release 2.2.7, 9/2023; FanGraphs funcs broken by 403, issue #507 open 4/2026); successor **pybaseballstats** dropped FanGraphs over anti-scraping. | PyPI/GitHub primary | still works for Savant/Retrosheet pulls | jldbc/pybaseball; PyPI | primary | 2023–26 | T1 | High | High |
| 15 | **Prediction markets (Kalshi/Polymarket)** effective cost ~**2%** vs ~4.5% sportsbook vig, but **thin MLB per-game depth** (5–10¢ spreads erase edge); **sharper on liquid games, softer on thin ones**; MLB's deal is with **Polymarket**; Kalshi data ToS bars using its data to build financial instruments/ML w/o consent. | venue docs; deucescracked (2026) | Kalshi per-game MLB inventory unconfirmed (futures-heavy) | venue primary; Front Office Sports (T3) | primary/news | 2026 | T1/T3 | Med | Med |
| 16 | **Ensembling helps modestly:** ATC (weighted) won FantasyPros accuracy 2019–23; Depth Charts (50/50 Steamer+ZiPS) beats ZiPS alone; 2024 top 3 within 0.017. | FantasyPros 2024 results | weighted-vs-naïve-average not isolated in a controlled test | FantasyPros (2/2025); FanGraphs | quality blog | 2017–25 | T3 | Med-High | Med |

*No load-bearing row rests on Tier-4 evidence. Tout "edge size / win rate" claims (e.g., a vendor's "3%+ vig-removed edge") were encountered and **excluded** as T4 marketing.*

---

## 5. Contradiction Map

**Major — "Statcast expected stats predict the future" vs "they're descriptive only."** The popular framing treats xwOBA/xBA as forecasting tools; Judge's "Siren Song" (#8) and the in-season study show they're strongly *descriptive* (same-half r≈0.78–0.82) but **weakly predictive across a half** (r≈0.35–0.41) and add **no edge over FIP/DRA for pitchers**. **Resolved:** use xStats as a *within-season prior to regress toward*, not a forecaster; lead the K model with **plate-discipline/swing rates** (SwStr%, CSW%), which genuinely out-predict naïve K% early because they accumulate per-pitch. *What would resolve fully:* a held-out test of "K% prior + SwStr%/CSW% update" vs "naïve trailing K%" on CLV.

**Major — "props are soft / beatable" vs "markets are efficient."** Academic efficiency results (#4) sit against the practitioner consensus that props are median-priced and outsourced (#11). **Both true at different liquidity tiers:** liquid game sides/totals are efficient and high-limit; props are softer *and* low-limit. The inefficiency and the capacity cap are the same coin — books leave props soft *because* they cap the size at which anyone can punish them.

**Major — "the best team-strength model wins" vs "the market is the ceiling."** A sharper game model buys ~1 win/162 (#3) and lands you at ~65% accuracy — where the market already is (#2, #4). **Resolved:** structural game-side edge ≈ 0; the realistic game-side play is beating the *opening/intraday* line on totals/F5 via fast inputs (umpire/weather/lineup/leash), then measuring yourself in CLV. This is why the build-first target is a **prop**, not the moneyline.

**Minor.** (a) CSW% vs SwStr% as the single best K predictor (unresolved, both strong). (b) Whether weighted ensembles beat naïve averages (asserted, not controlled-tested). (c) Direction of the favorite-longshot bias across eras.

**Open questions / unknowns.** Exact MLB game Brier floor (the ~0.24 figure is *derived* from the .40–.60 talent band, not a single cited study); Kalshi/Polymarket per-game MLB liquidity (unconfirmed); the K%-by-times-through-order decline (asserted but under-published).

**Consensus areas.** Strikeouts are the most modelable prop; K-rate stabilizes fastest; projected innings/leash dominates K variance; game markets are efficient and edge is operational; CLV (vs de-vigged close) is the correct out-of-sample test; regime breaks must be respected in backtests.

---

## 6. Blind Spot Report

- **Stakeholders ignored:** the *book's risk desk* — your edge exists only until it flags you on CLV and cuts your limit; and the *market-maker* (Swish) whose model you're implicitly competing against on props.
- **Hidden incentives:** books keep props soft because low limits make fine-pricing unprofitable; that same softness is why your edge can't scale. MLBAM/Sportradar have a revenue incentive to gate clean low-latency data behind a paid feed.
- **Unchallenged assumptions:** that a model edge survives contact with **vig + limits + scratch voids**; that prediction-market MLB liquidity is deep enough to trade; that historical Statcast generalizes through the 2023 rules and 2026 ABS-challenge regime breaks.
- **Topics skipped / under-weighted:** bullpen/opener modeling (secondary but priced); same-game-parlay correlation (a different, harder product); minor-league/Statcast-AAA signal for rookies (where Marcel-class priors fail worst); injury/IL-feed latency (no first-class endpoint).
- **Data that was unavailable / uncheckable:** the **xwOBA model coefficients** (MLBAM proprietary — the feature is a black box you can't fully validate); **Sportradar pricing** (undisclosed); **Kalshi/Polymarket live MLB per-game depth** (must be measured empirically, not researched); two Nov-2025 arXiv PDFs whose internal Brier/log-loss tables were blocked (headline findings confirmed via abstracts only).

---

## 7. Red Team Assessment

**Attack 1 — "Strikeouts-first is wrong; the cheap path to money is game totals."**
*Logic:* totals carry mispriceable inputs (umpire, weather, park) and higher limits than props.
*Impact if true:* you'd build a totals model, not a prop engine.
*Response:* totals are still a *game-efficient*, high-limit market where you compete with sharp money and win only on speed; the evidence says structural edge there ≈ 0 and the realistic play is operational/intraday. Props are softer and the K model is the most calibratable thing in baseball. Totals are a *second* build, fed by the *same* simulation engine.
*Residual risk:* if your true comparative advantage is **latency/automation** (acting on lineup/umpire/weather faster than books), totals/F5 may actually out-earn props per unit effort. Worth an explicit capacity comparison before committing.

**Attack 2 — "CLV is necessary but not sufficient; you'll show CLV and still lose."**
*Logic:* books limit you before CLV converts to realized profit; CLV at a book you can't get size at is worthless.
*Impact:* the whole "prove it with CLV" plan validates a model you can't monetize.
*Response:* conceded as the central real-world risk — it's the same access-not-accuracy wall as the tennis track. CLV still correctly separates *model skill* from *variance* ~200× faster than P/L, so it's the right *modeling* signal; monetization is a separate, harder problem to be tested empirically against actual venue limits.
*Residual risk:* high. Positive CLV may never translate to a scalable book.

**Attack 3 — "Your features leak; the edge is a backtest artifact."**
*Logic:* using final lineups, closing lines, or revised Statcast as features is look-ahead bias; regime changes (2019 ball, 2023 rules, 2026 ABS) make a multi-year backtest fit three different sports.
*Impact:* a beautiful backtest that dies live.
*Response:* mitigated by the protocol — freeze features at scheduled first pitch, never feed closing lines, walk-forward by season respecting regime breaks, and treat Savant's retroactive revisions as next-day-only.
*Residual risk:* medium. Subtle leakage (e.g., a "probable pitcher" that was actually a late scratch in your training labels) is easy to introduce and hard to detect.

**Attack 4 — "Prediction-market MLB liquidity is too thin to matter, so the whole exercise is academic for FORTUNA."**
*Logic:* Kalshi MLB is futures-heavy; per-game depth shows 5–10¢ spreads that erase edge.
*Impact:* the model has nowhere to trade at size.
*Response:* genuinely unresolved — flagged as the highest-leverage open question. The cheap-stack pre-game prop model is still the right *first build* regardless of venue, because it's the most validatable slice; but venue capacity must be measured before claiming a business.
*Residual risk:* high and decision-relevant.

---

## 8. Key Findings

1. **Strikeouts are the most modelable target in baseball — build them first.** K-rate stabilizes fastest (hitter 60 PA, pitcher 70 BF), the predictive features (SwStr%, CSW%) accumulate per-pitch and out-predict naïve K% early-season, and Ks are high-count, skill-driven, and game-outcome-independent (#6, #7, #11). *So what:* it's the slice you can calibrate and prove fastest.
2. **Game-side modeling edge is ≈ 0; the market is the ceiling.** Best models add ~1 win/162 over simple ones and land at ~65% accuracy where the market already sits; edge there is operational, not structural (#2, #3, #4). *So what:* don't start with moneylines — start with a prop.
3. **The engine is one thing; the prop is the first output.** A single-game Monte Carlo (lineup → PA via log5 batter×pitcher×league → per-PA multinomial outcome) produces the K distribution *and* win prob *and* (later) TB/hits. Build the engine, validate the K slice. *So what:* strikeouts-first is not a dead-end — it's the first validated wedge of the general model.
4. **Projected innings/leash, not raw K%, dominates K-prop variance.** A high-K% starter pulled at 5 IP busts the over; opponent team-K% (the batter side) controls most of the rest; framing/umpire/park are real secondary overlays (#9, #10). *So what:* spend modeling effort on the leash/BF distribution and the matchup, not on a sharper season K-rate.
5. **The binding constraint is access and capacity, not accuracy.** Props carry 6–10% vig and $250–500 limits, books price them off the median and limit winners on CLV, and prediction-market MLB per-game depth is thin (#11, #13, #15). *So what:* a calibrated model is necessary but not sufficient — prove the *edge* with CLV, then prove the *capacity* against real venue limits before believing in a business.

---

## 9. Recommendations

1. **Build a single-game simulation engine and validate the pitcher-strikeout over/under first.** (Findings 1, 3.) Core: per-batter K probability via log5 (`pitcher-K% × team-K% / league-K%`, split by handedness), aggregated over a **modeled batters-faced / leash distribution** with a **beta-binomial** count, plus catcher-framing / umpire / park overlays. Lead the K-rate input with a **Marcel-class true-talent prior regressed toward, then updated with stabilized in-season SwStr%/CSW%** — not trailing raw K%.
2. **Make the leash/BF model a first-class component, not an afterthought.** (Finding 4.) It is the dominant variance driver and the most under-published — likely your largest source of differentiation.
3. **Score with proper scoring rules and CLV, never accuracy%.** (Finding 5, evidence #5.) Brier + log-loss + reliability diagram + ECE on held-out seasons; the bar to beat is the **de-vigged closing-line Brier**, not the 0.25 coin-flip baseline. Track CLV vs the de-vigged close in paper trading as the gold-standard skill test.
4. **Defer total bases and hits.** (Finding 1, evidence #12.) BABIP luck doesn't stabilize within a game, PA depends on lineup slot, TB lines sit on lumpy discrete jumps, and non-starts void the bet. Revisit as a simulation-heavy second build.
5. **Build the data stack on Retrosheet + Savant (batch/train) and StatsAPI + public WS prices (live/pre-game); design around the regime breaks.** (Evidence #13, #14.) Pre-game prop pricing tolerates next-morning Statcast and ~12s StatsAPI — you do **not** need Sportradar to start. Split backtests at 2019 / 2023 / 2026 boundaries; freeze features at scheduled first pitch; derive "confirmed lineup" from a populated `battingOrder`.
6. **Treat venue capacity as a gating experiment, not an assumption.** (Finding 5, evidence #15.) Measure Kalshi/Polymarket per-game MLB depth and sportsbook prop limits empirically before sizing anything; read the Kalshi API Developer Agreement and MLBAM terms before any commercial pull.

---

## 10. Confidence Scores

| Finding | Confidence | Reasoning | Weaknesses | Raises confidence | Lowers confidence |
|---------|-----------|-----------|------------|-------------------|-------------------|
| 1. Strikeouts most modelable → build first | **88%** | Convergent across stabilization data, feature studies, and prop consensus | "Most beatable" is T2 consensus, no audited edge table | A held-out CLV test of K model vs market | An audited study showing hitter props equally beatable |
| 2. Game-side edge ≈ 0; market is ceiling | **85%** | Multiple T1/T2 sources; ~1-win/162 result is stark | Newer papers claim *some* exploitable inefficiency | Replication of the ~1-win result | A credible sustained-CLV game-side model |
| 3. One engine, prop is first output | **80%** | Standard sim architecture; log5 is canonical | Engineering claim, not an empirical result | A working sim that reproduces market K lines | Sim mis-calibration vs market on backtest |
| 4. Leash/BF dominates K variance | **78%** | Practitioner consensus + TTO evidence | K%-by-TTO is under-published; partly inferred | A variance decomposition on real data | Data showing K% matters more than BF |
| 5. Access/capacity is the binding constraint | **75%** | Strong on limits/vig; thinner on PM liquidity | PM MLB depth unmeasured | Direct depth measurement | Surprisingly deep Kalshi/PM per-game books |

---

## 11. Open Questions

**Blocks the decision (resolve before betting real capital):**
- **Does the cheap pre-game stack produce a pitcher-K model with positive CLV vs the de-vigged closing line on a held-out season?** (The core modeling proof.)
- **Is there enough Kalshi/Polymarket per-game MLB liquidity — and what are the real prop limits — to trade the edge at meaningful size?** (The capacity proof; currently unconfirmed, evidence #15.)

**Good to know (refine, don't block):**
- CSW% vs SwStr% as the single best K input; the exact K%-by-times-through-order decline; whether weighted ensembles beat naïve averages; the empirical MLB game Brier floor; how much the 2026 ABS challenge has already eroded framing value this season.

---

## 12. Research Frontier

- **+10 hours:** (a) Pull Kalshi + Polymarket MLB market inventory and **measure live per-game depth/spread** — the binding unknown. (b) On one season of Savant data, sanity-check that a **"K% prior + SwStr%/CSW% update" beats trailing raw K%** out-of-sample on next-start K total. (c) Confirm pybaseball vs pybaseballstats for a clean Savant/Retrosheet pull given the FanGraphs 403.
- **+100 hours:** Build the single-game Monte Carlo engine; validate the **pitcher-K over/under slice on 2–3 held-out seasons by CLV** vs de-vigged closing lines, with the leash/BF distribution as a first-class component and framing/umpire/park overlays; produce reliability diagrams and an ECE per season.
- **Dedicated team:** Extend the engine to TB/hits and game win prob; add a bullpen/opener and pitcher-leash model; build a near-real-time pre-game pipeline (StatsAPI + lineup/weather/umpire ingestion); evaluate a Sportradar feed only if a live/in-play ambition or commercial-licensing need materializes.
- **Highest-leverage single question:** **Can a pre-game pitcher-strikeout model show positive CLV vs the de-vigged closing line on held-out seasons, AND is there enough prediction-market per-game liquidity to trade it at size?** That one combined unknown decides whether this is a business or an academic exercise — it pairs the *modeling* proof (CLV) with the *capacity* proof (depth/limits), and everything else is downstream of it.

---

### Source ledger (load-bearing, by tier)

**Tier 1 (peer-reviewed / primary):** Woodland & Woodland, *J. Finance* 1994 (MLB market efficiency); Cui, Wharton thesis 2020 (game accuracy); Mott et al. arXiv 2511.17733 2025 (~1 win/162); Allen & Savala arXiv 2511.02815 2025 (naïve ML loses); Healey, J. Sports Analytics 2015 (generalized log5); MLBAM GUMBO / gdx copyright.txt (latency + terms, live 2026-06-18); Sportradar–MLB PR (GlobeNewswire 2/2025); Retrosheet notice.txt (commercial license); jldbc/pybaseball + PyPI (maintenance state); Kalshi/Polymarket venue docs.

**Tier 2 (expert / industry):** Baseball Prospectus — Carleton stabilization, Judge "Siren Song"; FanGraphs Library — BaseRuns/RE24/projections/framing; Pitcher List — CSW%; Unabated / Wizard of Odds / Action Network — prop hold, limits, market-making; bet2invest/Buchdahl — CLV; arXiv 2410.21484 review.

**Tier 3 (quality blogs / news):** FanGraphs blog & RotoGraphs; Beyond the Box Score (projection grading); FantasyPros (2024 accuracy); VSiN (HFA); betstamp (totals/umpire); Front Office Sports (MLB–Polymarket); deucescracked (PM cost/liquidity).

**Excluded (Tier 4):** betting-tout win-rate / "vig-removed edge" claims; community scraping-etiquette conventions stated as official terms.
