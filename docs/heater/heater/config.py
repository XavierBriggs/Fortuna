"""Paths and parameters. Data lives under the repo's gitignored data/ dir."""
from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path

from dotenv import load_dotenv

# docs/heater/heater/config.py -> repo root is three parents up; project dir is one up.
REPO_ROOT = Path(__file__).resolve().parents[3]
_PROJECT_DIR = Path(__file__).resolve().parents[1]
# load in order; load_dotenv does NOT override already-set vars, so first wins.
# .env.heater is the project's secret file (ODDS_API_KEY); plain .env also honored.
for _candidate in (
    _PROJECT_DIR / ".env.heater",
    _PROJECT_DIR / ".env",
    REPO_ROOT / ".env.heater",
    REPO_ROOT / ".env",
):
    load_dotenv(_candidate)  # all gitignored; harmless if absent


def data_dir() -> Path:
    d = os.environ.get("HEATER_DATA_DIR")
    return Path(d) if d else REPO_ROOT / "data" / "heater"


def odds_api_key() -> str | None:
    return os.environ.get("ODDS_API_KEY")


def kalshi_key_id(demo: bool = False) -> str | None:
    """Kalshi RSA-PSS access key id from fortuna/.env (loaded above). Read-only use."""
    return os.environ.get("KALSHI_API_DEMO_KEY_ID" if demo else "KALSHI_API_KEY_ID")


def kalshi_private_key_path(demo: bool = False) -> str | None:
    return os.environ.get("KALSHI_DEMO_PRIVATE_KEY_PATH" if demo else "KALSHI_PRIVATE_KEY_PATH")


@dataclass(frozen=True)
class KModelConfig:
    """The strikeout-rate matchup and the batters-faced (leash) distribution.

    `league_k` is the per-PA strikeout baseline the log5 odds-ratio regresses
    toward (MLB was ~0.225 in 2023-24). The leash params shape the batters-faced
    distribution — its SPREAD, not its mean, is the dominant source of strikeout
    variance (see docs/research/2026-06-18-baseball-modeling.md, finding 4).
    """
    league_k: float = 0.225        # MLB per-PA K rate baseline (2023-24)
    bf_floor: int = 8              # min batters faced modeled (early KO)
    bf_cap: int = 34               # max batters faced modeled (deep complete-ish game)
    proj_bf_mean: float = 23.0     # ~5.2 IP starter default
    proj_bf_sd: float = 4.0        # leash uncertainty; widen for short-leash arms
    park_k_mult: float = 1.0       # park strikeout multiplier (1.0 = neutral)
    # --- ablation toggles (which model components are active) ---
    use_opponent: bool = True      # log5 opponent adjustment (off => pitcher rate only)
    use_leash_spread: bool = True  # BF as a distribution (off => fixed batters-faced)
    # park OFF and prior=trailing by default: on 2024 OOS data the crude park factor and the
    # linear CSW->K map both HURT (they add noise where trailing K% is already well-estimated).
    # Kept as toggles for when a real park factor / small-sample CSW weighting is built.
    use_park: bool = False         # apply the park multiplier
    k_prior_source: str = "trailing"  # "trailing" | "csw" | "blend" — which K% prior to use


@dataclass(frozen=True)
class BacktestConfig:
    season_from: int = 2021        # first season to SCORE (after warm-up)
    warmup_seasons: int = 1        # seasons used only to seed trailing priors
    line_offset: float = 0.5       # synthetic O/U line sits at floor(E[K]) + offset
    bootstrap_n: int = 2000
    seed: int = 7
    # segments to report over (always per-segment, never one global number)
    segment_cols: tuple[str, ...] = ("k_tier", "bf_tier")
    model: KModelConfig = field(default_factory=KModelConfig)
