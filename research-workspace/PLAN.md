# PLAN — Perpetual Futures Funding-Rate & Basis Modeling/Strategy (decision-grade)

## Restated query
Build a predictive model + strategy stack for perpetual futures: funding-rate dynamics
and basis / cross-market relative value. Accurate enough to forecast funding and find edge.
Eventually feeds FORTUNA (event perpetuals — bounded 0/1, funding anchored to a reference
probability, no spot to carry) — keep that context light, focus on the modeling/strategy problem.
Emphasis on STRATEGIES, not only predictive models.

## Decision
Which signal to build FIRST (funding forecast vs relative-value basis); features + data that
matter; how to validate honestly (OOS funding accuracy, basis-convergence backtests, edge net
of fees+funding, capacity/crowding); realistic edge after costs; highest-leverage next research.

## Query type
Breadth-first — 6 independent slices to parallel web-research subagents. Each returns GRADED
evidence blocks: claim · supporting · contradicting · source (name/date/URL) · TIER (T1 official/
peer-reviewed; T2 industry/whitepaper; T3 news/blog; T4 forum/anecdote) · confidence. Hunt
contradicting evidence. Flag Tier-4-only claims. Be honest about gaps and what couldn't be verified.

## Subagent streams (wave 1, parallel)
- A. Funding & basis MECHANICS + funding↔basis relationship: per-venue funding formula
  (Binance/Bybit/OKX/BitMEX/Hyperliquid/dYdX) — premium index + interest component, clamps,
  intervals (8h/1h/continuous), settlement-timing effects, mark vs index, basis = perp−index;
  no-arbitrage / cost-of-carry anchor; empirical magnitude + persistence of funding.
- B. Funding-rate FORECASTING — models & OOS performance: AR/mean-reversion, autocorrelation/
  half-life, predictive features (basis/premium, OI, OB imbalance, long/short, leverage, vol
  regime), ML, regime dependence. What actually moves OOS accuracy; how thin public OOS evidence is.
- C. Funding-CAPTURE & basis/cash-and-carry STRATEGIES + net edge + capacity: delta-neutral
  funding farming, cash-and-carry, perp vs dated vs spot RV; historical returns (2021 carry,
  2024 basis, Ethena USDe), fees/borrow/funding drag, liquidation risk, crowding/capacity, decay.
- D. Order-flow / OI / POSITIONING / liquidations microstructure signals: OI dynamics, long/short
  ratio, taker imbalance, liquidation cascades, funding as crowding/sentiment → reversion/squeeze.
- E. EVENT PERPETUALS / prediction-market perps (novel core): what real products exist; funding
  anchored to a reference probability; bounded 0/1, no spot carry; how much crypto literature
  transfers vs needs rebuilding. Skeptical sourcing; flag nascent/T4 heavily.
- F. DATA REALITY: funding history / mark+index / OI / L2 order-book — venue APIs + vendors
  (Coinglass/Coinalyze, Kaiko, Amberdata, Tardis.dev, Laevitas, CoinAPI, CCData). Depth,
  granularity, latency, free vs paid, reliability. Which gaps constrain what's buildable.

## Output
12-section deep-research memo → docs/research/2026-06-18-perpetual-futures-modeling.md
Then synthesis phases (perspectives, contradiction map, blind spots, red team, confidence,
frontier) done by main loop, verifying subagent claims.
