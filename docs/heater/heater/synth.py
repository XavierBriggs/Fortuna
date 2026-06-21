"""Deterministic synthetic starts so the harness runs with zero downloaded data.

The point of the demo is NOT to claim real edge — it's to prove the wiring and to
show the harness can detect a model that is genuinely better than the naive
baseline WHEN the data-generating truth actually contains opponent and leash
structure. We draw a true per-PA K rate (log5 of true pitcher/opponent rates x
park) and a true batters-faced count with real spread, then realize K from them.
The features the model sees are NOISY priors (trailing estimates), never the
truth or the realized result — leakage-safe by construction.

On real Statcast data (`data.statcast`) the same schema is produced from
as-of-date trailing rates; the backtest is source-agnostic.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from .config import KModelConfig
from .log5 import apply_park, matchup_k

_PARKS = np.array([0.92, 0.97, 1.0, 1.0, 1.03, 1.08])  # K-suppressing .. K-friendly


def make_starts(n: int = 4000, seasons: tuple[int, ...] = (2020, 2021, 2022, 2023), seed: int = 7) -> pd.DataFrame:
    """Generate `n` synthetic starts in the canonical, leakage-safe schema."""
    rng = np.random.default_rng(seed)
    cfg = KModelConfig()
    rows = []
    for i in range(n):
        season = seasons[i % len(seasons)]
        # --- truth (never exposed as a feature) ---
        pit_true = float(np.clip(rng.normal(0.235, 0.045), 0.12, 0.36))
        opp_true = float(np.clip(rng.normal(0.225, 0.025), 0.16, 0.30))
        park = float(_PARKS[rng.integers(0, len(_PARKS))])
        q_true = apply_park(matchup_k(pit_true, opp_true, cfg.league_k), park)
        # leash: better pitchers go deeper; real spread is the whole point
        bf_mean_true = float(np.clip(rng.normal(20 + 60 * (pit_true - 0.20), 0.0) + 0.0, cfg.bf_floor, cfg.bf_cap))
        realized_bf = int(np.clip(round(rng.normal(bf_mean_true, 4.0)), cfg.bf_floor, cfg.bf_cap))
        realized_k = int(rng.binomial(realized_bf, q_true))
        # --- features the model is allowed to see (noisy priors, no peeking) ---
        pit_k_prior = float(np.clip(pit_true + rng.normal(0, 0.022), 0.08, 0.40))
        opp_k_prior = float(np.clip(opp_true + rng.normal(0, 0.012), 0.14, 0.32))
        proj_bf_mean = float(np.clip(bf_mean_true + rng.normal(0, 1.5), cfg.bf_floor, cfg.bf_cap))
        rows.append(
            {
                "date": pd.Timestamp(f"{season}-06-01") + pd.Timedelta(days=i % 150),
                "season": season,
                "pitcher": f"SP{i % 220:03d}",
                "throws": "R" if i % 4 else "L",
                "opp_team": f"T{i % 30:02d}",
                "pit_k_prior": pit_k_prior,
                "opp_k_prior": opp_k_prior,
                "proj_bf_mean": proj_bf_mean,
                "proj_bf_sd": cfg.proj_bf_sd,
                "park_k_mult": park,
                "realized_bf": realized_bf,
                "realized_k": realized_k,
                "line": float("nan"),
                "over_odds": float("nan"),
                "under_odds": float("nan"),
            }
        )
    return pd.DataFrame(rows).sort_values("date").reset_index(drop=True)
