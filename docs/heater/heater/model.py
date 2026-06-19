"""HEATER strikeout model vs the naive baseline — the two competing forecasts.

`heater_pmf` is the full model: a log5 opponent/park matchup rate fed through a
compound count distribution whose batters-faced spread encodes leash uncertainty.
`baseline_pmf` is deliberately naive — the pitcher's OWN trailing K rate, no
opponent/park adjustment, and a FIXED batters-faced count (no leash spread). The
backtest pits the two against each other, so the gap isolates exactly the two
theses worth money: (1) the matchup adjustment, (2) treating the leash as a
distribution rather than a point.
"""
from __future__ import annotations

import numpy as np

from .config import KModelConfig
from .kdist import bf_pmf, k_pmf
from .log5 import apply_park, matchup_k


def heater_pmf(
    pit_k: float,
    opp_k: float,
    proj_bf_mean: float,
    proj_bf_sd: float,
    park_mult: float,
    cfg: KModelConfig,
) -> np.ndarray:
    """Full model: log5(pitcher, opponent, league) x park, compounded over a
    batters-faced distribution with real leash spread.

    The three `use_*` toggles on `cfg` ablate each component independently; with
    all three off this reduces exactly to `baseline_pmf` (a league-average
    opponent leaves the log5 rate at the pitcher's own, and sd=0 fixes BF)."""
    eff_opp = opp_k if cfg.use_opponent else cfg.league_k
    q = matchup_k(pit_k, eff_opp, cfg.league_k)
    if cfg.use_park:
        q = apply_park(q, park_mult)
    sd = proj_bf_sd if cfg.use_leash_spread else 0.0
    bfs, w = bf_pmf(proj_bf_mean, sd, cfg.bf_floor, cfg.bf_cap)
    return k_pmf(q, bfs, w)


def baseline_pmf(pit_k: float, proj_bf_mean: float, cfg: KModelConfig) -> np.ndarray:
    """Naive baseline: trailing K rate, no opponent/park, FIXED batters faced."""
    bfs, w = bf_pmf(proj_bf_mean, 0.0, cfg.bf_floor, cfg.bf_cap)
    return k_pmf(pit_k, bfs, w)
