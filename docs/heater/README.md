# HEATER — pitcher-strikeout prop research harness

> A *heater* is a blazing fastball — and a gambler on a hot streak. Both meanings
> are the point: it turns pitch-level dominance into strikeout prices for **FORTUNA**.

HEATER v0 is a research MVP that answers one cheap, decisive question:

> **Are we even in the game?** — does adjusting for the opponent (log5) and treating
> the pitcher's *leash* as a distribution beat just projecting his trailing strikeout
> rate, in any segment, on free data?

Strikeouts are the most modelable thing in baseball — they stabilize fastest, are
skill-driven, and barely depend on who wins (see the memo at
`docs/research/2026-06-18-baseball-modeling.md`). So the first validated slice of a
single-game engine is the **pitcher-strikeout distribution**. The MVP stands that
up, scores it honestly, and wires the **CLV measurement** that proves real edge later.

This is **research code** (Python, exploratory). It is intentionally *not* in the
Rust workspace and is *not* on any money path — FORTUNA's house rules (integer cents,
no-unwrap, `Clock` injection) govern the trading core, not this harness. The eventual
production pricing path would be a Rust port of the validated model.

## The model in one breath

A start's strikeout total is a **compound count distribution**:

```
q   = log5(pitcher_K%, opponent_K%, league_K%) · park          # per-PA whiff prob
K | BF ~ Binomial(BF, q),   BF ~ leash distribution            # marginalize BF out
P(K=k) = Σ_bf  P(BF=bf) · Binomial(k; bf, q)
```

Letting **BF vary** (the manager's leash) instead of fixing it is what produces the
overdispersion real strikeout totals show — the practitioner route, not a curve-fit
negative binomial. The naive baseline drops both ideas (own rate, fixed BF); the
backtest is the gap between them.

## The three phases (only Phase A is in this MVP)

| Phase | Data | Test | Status |
|-------|------|------|--------|
| **A. Calibration vs a baseline** | free realized K (Statcast / synthetic) | Does matchup+leash beat trailing-K on RPS / log-loss? | **built** |
| **B. CLV from history** | The Odds API historical prop snapshots | Do flagged bets sit on the side the line moves toward? | client + clv built, fetch stubbed |
| **C. Forward paper-trade** | The Odds API live prop endpoint | Persistent positive CLV out-of-sample | client built |

Conflating A with CLV is the classic trap. A tests *distribution quality vs a
baseline*; **CLV is only real in B and C**, and only against the **de-vigged close**.

## Layout

```
heater/
  config.py      # paths + K-model & backtest params, env loading
  log5.py        # per-PA strikeout matchup (odds-ratio)            (pure, tested)
  kdist.py       # compound strikeout count distribution, P(over)   (pure, tested)
  devig.py       # proportional + Shin two-way over/under devig     (pure, tested)
  metrics.py     # log-loss, Brier, ECE, RPS, paired bootstrap CI   (pure, tested)
  model.py       # HEATER pmf vs the naive baseline pmf
  synth.py       # deterministic synthetic starts (the zero-data demo)
  backtest.py    # Phase-A walk-forward, per-segment, model vs baseline
  clv.py         # Odds-API strikeout-prop CLV: entry vs close      (Phase B/C)
  cli.py         # entrypoints: backtest / sports / clv
  data/
    statcast.py  # real starts.csv loader -> canonical leakage-safe schema
    odds_api.py  # The Odds API v4 client, MLB pitcher_strikeouts market
tests/           # log5, kdist, devig, metrics, backtest integration
scripts/         # get_statcast.py (pybaseball -> starts.csv; scaffold)
```

## Quickstart

```bash
cd docs/heater
python3 -m venv .venv && source .venv/bin/activate   # pyenv has no bare `python` shim
pip install -e ".[dev]"
pytest                                   # pure modules + the model-beats-baseline gate

# Phase-A backtest — runs out of the box on synthetic data (no downloads, no key)
heater backtest                          # add --n 8000 --seed 3 to vary the demo

# (later) real data: fill in scripts/get_statcast.py, then
pip install -e ".[data]" && python scripts/get_statcast.py
heater backtest --real

# (later) the CLV layer needs your key
export ODDS_API_KEY=...                   # or put it in .env.heater (gitignored)
heater sports                             # confirm MLB key + your credit balance
```

## The canonical, leakage-safe schema

One row per start; **every feature is an as-of-date prior**, never the realized
result — so nothing downstream can peek at the start it predicts.

| col | meaning |
|-----|---------|
| `date`, `season`, `pitcher`, `throws`, `opp_team` | start context |
| `pit_k_prior` | pitcher true-talent K% (trailing, regressed; lead with SwStr%/CSW%) |
| `opp_k_prior` | opponent team K% vs the pitcher's hand (trailing) |
| `proj_bf_mean`, `proj_bf_sd` | the **leash**: projected batters faced, mean + spread |
| `park_k_mult` | park strikeout multiplier (1.0 = neutral) |
| `realized_bf`, `realized_k` | labels |
| `line`, `over_odds`, `under_odds` | optional market (Phase B/C) |

## What "pass / fail / null" means

- **Phase-A gate:** keep segments where `heater_ll < base_ll` with the paired
  bootstrap CI fully below 0 (`beats_base=True`) **and** `heater_rps < base_rps`.
  Those are *candidates*, not a green light.
- **Phase-B/C gate:** forward CLV ≥ 0 vs the **de-vigged** close, persistent over
  ≥50–100 bets/segment, before any capital. (FORTUNA's I7 forward-validation rule.)
- **A clean null** ("nothing beats the baseline / the close after vig") is a
  successful, money-saving outcome.

Model selection runs on **RPS / log-loss, not ROI** — ROI overfits (see the memo).

## Honest limits (from the memo)

- The synthetic demo proves the **wiring and the thesis on controlled data**; it is
  not evidence of real edge. Real edge needs Phase B/C on actual prop lines.
- pybaseball is semi-dormant and its FanGraphs path is 403-blocked — the Statcast
  path still works; budget for that in `get_statcast.py`.
- The edge that matters lives in the **leash/BF model and the matchup overlays the
  median-priced book underweights** — and it is capacity-capped ($250–500 prop
  limits, 6–10% vig). The binding constraint is access, not accuracy.
```
