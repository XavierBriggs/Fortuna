"""Paths and parameters. Data lives under the repo's gitignored data/ dir."""
from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path

from dotenv import load_dotenv

# docs/deuce/deuce/config.py -> repo root is three parents up; project dir is one up.
REPO_ROOT = Path(__file__).resolve().parents[3]
_PROJECT_DIR = Path(__file__).resolve().parents[1]
# load in order; load_dotenv does NOT override already-set vars, so first wins.
# .env.deuce is the project's secret file (ODDS_API_KEY); plain .env also honored.
for _candidate in (
    _PROJECT_DIR / ".env.deuce",
    _PROJECT_DIR / ".env",
    REPO_ROOT / ".env.deuce",
    REPO_ROOT / ".env",
):
    load_dotenv(_candidate)  # all gitignored; harmless if absent


def data_dir() -> Path:
    d = os.environ.get("DEUCE_DATA_DIR")
    return Path(d) if d else REPO_ROOT / "data" / "deuce"


def odds_api_key() -> str | None:
    return os.environ.get("ODDS_API_KEY")


@dataclass(frozen=True)
class EloConfig:
    init_rating: float = 1500.0
    k_base: float = 150.0          # 538-style numerator (tuned down from 250)
    k_shape: float = 0.4           # decay exponent
    k_offset: float = 5.0          # softens early volatility
    surface_weight: float = 0.35   # blend: w*surface + (1-w)*overall (tuned from 0.5)
    margin_weighting: bool = True  # WElo: scale update by scoreline dominance
    margin_scale: float = 0.6      # how hard dominance bends the update (bounded)


@dataclass(frozen=True)
class BacktestConfig:
    tour: str = "atp"
    since_year: int = 2010         # first season to SCORE (after burn-in)
    burnin_years: int = 3          # seasons to warm Elo, not scored
    devig_method: str = "shin"     # "proportional" | "shin"
    bootstrap_n: int = 2000
    seed: int = 7
    calibrate: bool = True         # prequential recalibration of p_model
    # per-regime Platt is the robust default: fixes the Bo5/Slam over-shrink that a
    # GLOBAL map caused, with no log-loss fragility. isotonic is opt-in — more
    # flexible (fixes shape, e.g. heavy favourites) but log-loss-fragile at extremes.
    calib_method: str = "platt"    # "platt" (a,b) | "temperature" (a only) | "isotonic"
    calib_min_fit: int = 1000      # min prior-year in-regime matches before recalibrating
    calib_regime: str = "best_of"  # per-regime calibration key ("" => single global map)
    # segments to report metrics over (always per-segment, never one global number)
    segment_cols: tuple[str, ...] = ("surface", "series", "odds_bucket")
    elo: EloConfig = field(default_factory=EloConfig)
