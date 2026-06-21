# DEUCE — tennis win-probability research harness

> Named for the score where it matters most — point importance peaks at 30–30/deuce.
> The base-rate oracle that prices matches for **FORTUNA**.

DEUCE v0 is a research MVP that answers one cheap, decisive question:

> **Are we even in the game?** — does a surface-weighted Elo model beat the
> *closing line* in log-loss, in any segment, after costs?

The expected (and still useful) answer is "we tie the market on liquid ATP." The
point of the MVP is to learn that for ~free before building anything heavier, and
to stand up the **CLV measurement** that actually proves edge later.

This is **research code** (Python, exploratory). It is intentionally *not* in the
Rust workspace and is *not* on any money path — FORTUNA's house rules (integer
cents, no-unwrap, `Clock` injection) govern the trading core, not this harness.
The eventual production pricing path would be a Rust port of the validated model.

## The three phases (only Phase A is in this MVP)

| Phase | Data | Tests | Status |
|-------|------|-------|--------|
| **A. History snapshot** | tennis-data.co.uk near-close Pinnacle odds (ATP 2000+) | Is DEUCE better-calibrated than the close? | **built** |
| **B. CLV from history** | The Odds API historical snapshots (5–10 min, 2020+) | Do flagged bets sit on the side the line moves toward? | client built, harness stubbed |
| **C. Forward paper-trade** | The Odds API live endpoint | Persistent positive CLV out-of-sample, real-time | client built |

Conflating A with CLV is the classic trap. A tests *calibration vs a near-efficient
market*; **CLV is only real in B and C.**

## Layout

```
deuce/
  config.py        # paths + model/backtest params, env loading
  elo.py           # surface-weighted WElo engine  (pure, tested)
  devig.py         # proportional + Shin vig removal (pure, tested)
  metrics.py       # log-loss, Brier, calibration, ECE, bootstrap CI (pure, tested)
  calibrate.py     # per-regime prequential recalibration: Platt | isotonic (pure, tested)
  names.py         # player/tournament name normalization (for the Sackmann join)
  join.py          # Sackmann <-> tennis-data join          (NEXT PHASE: serve/return features)
  backtest.py      # walk-forward Phase-A harness
  clv.py           # Odds-API CLV: entry snapshot vs close   (Phase B/C)
  cli.py           # entrypoints: backtest / clv-capture / fetch-sports
  data/
    tennisdata.py  # tennis-data.co.uk loader -> canonical, leakage-safe schema
    sackmann.py    # tennis_atp loader (serve/return stats)  (NEXT PHASE)
    odds_api.py    # The Odds API v4 client (live + historical)
tests/             # test_elo, test_devig, test_metrics
scripts/           # get_tennisdata.sh, get_sackmann.sh
```

## Quickstart

```bash
cd docs/deuce
python3 -m venv .venv && source .venv/bin/activate   # `python3` — pyenv has no bare `python` shim
pip install -e ".[dev]"
pytest                                  # the pure modules are tested first

# 1. get free historical results+odds (ATP 2000+)
./scripts/get_tennisdata.sh             # downloads into ../../data/deuce/tennisdata/

# 2. run the Phase-A "are we in the game" backtest
deuce backtest --tour atp --since 2010 --burnin-years 3

# 3. (optional) the CLV layer needs your key
export ODDS_API_KEY=...                 # or put it in .env.deuce (gitignored)
deuce fetch-sports                      # list tennis sport keys + your credit balance
```

## The canonical, leakage-safe schema

`data/tennisdata.py` collapses each match to a fixed player ordering **independent
of the result** (`p1` = alphabetically-first name), so nothing downstream can peek
at the winner through the odds columns:

| col | meaning |
|-----|---------|
| `date`, `tournament`, `series`, `surface`, `round`, `best_of` | match context |
| `p1`, `p2` | players, ordered by name (NOT winner/loser) |
| `y` | 1 if `p1` won, else 0 — the label |
| `rank1`, `rank2` | ATP ranks |
| `odds1`, `odds2` | decimal odds for p1 / p2 (Pinnacle, fallback Max/Avg/B365) |
| `w_games`, `l_games` | total games by winner/loser (for WElo margin weighting) |

## What "pass / fail / null" means

- **Phase-A gate:** keep only segments where `deuce_logloss < market_logloss` with
  bootstrap significance. Those are *candidates*, not a green light.
- **Phase-B/C gate:** forward CLV ≥ 0, persistent over ≥50–100 bets/segment, before
  any capital. (This is FORTUNA's I7 forward-validation discipline.)
- **A clean null** ("nothing beats the close after costs") is a successful, money-saving outcome.

Model selection runs on **log-loss, not ROI** — ROI overfits (see the memo at
`docs/research/2026-06-18-tennis-modeling.md`).

## Recalibration & tuning

- **Recalibration** (`calibrate.py`, on by default) is **per-regime** (default `best_of`,
  i.e. Bo3 vs Bo5) and **prequential** (refit each year on only prior-year, in-regime
  predictions — no leakage). Default method is **Platt**; a single global map over-shrinks
  the already-sharp regimes (Bo5 Slams, heavy favourites), which the per-regime split fixes.
  `--calib-method isotonic` is more flexible (fixes calibration *shape*) but log-loss-fragile
  at the extremes — opt-in. `--no-calibrate` shows raw Elo.
- **Tuning** (`deuce tune --since 2010 --split 2018`) grid-searches `surface_weight × K × margin`,
  **selecting on a train window and reporting on a holdout**. The first sweep moved the
  defaults to `K=150, surface_weight=0.35` (both were hitting the grid boundary, so the
  current grid reaches down to `K=75, surface_weight=0.2`).

