"""Sackmann <-> tennis-data join  (NEXT PHASE — wires serve/return features to odds).

NOT used by the Phase-A backtest. When you build the point-based serve/return
model, this attaches Sackmann's serve stats to tennis-data's odds rows. The join
is the single most error-prone step in the project — it MUST report its match
rate, never silently drop unmatched rows (that's how survivorship sneaks in).
"""
from __future__ import annotations

import pandas as pd

from .names import canonical_player


def join_sackmann_odds(
    sackmann: pd.DataFrame,
    tennisdata: pd.DataFrame,
    date_tolerance_days: int = 2,
) -> tuple[pd.DataFrame, dict[str, float]]:
    """Return (joined_frame, diagnostics). Keyed on date(±tol) + player pair.

    diagnostics includes match_rate so you can SEE coverage loss rather than
    assume 100%.
    """
    s = sackmann.copy()
    s["k_pair"] = [
        frozenset({canonical_player(win), canonical_player(los)})
        for win, los in zip(s["winner_name"], s["loser_name"])
    ]
    s["k_date"] = pd.to_datetime(s["tourney_date"])

    t = tennisdata.copy()
    t["k_pair"] = [frozenset({canonical_player(a), canonical_player(b)}) for a, b in zip(t["p1"], t["p2"])]
    t["k_date"] = pd.to_datetime(t["date"])

    merged = t.merge(s, on="k_pair", how="left", suffixes=("", "_sk"))
    within = (merged["k_date_sk"] - merged["k_date"]).abs() <= pd.Timedelta(days=date_tolerance_days)
    joined = merged[within | merged["k_date_sk"].isna()].copy()

    matched = joined["winner_name"].notna().sum()
    total = len(t)
    diagnostics = {
        "tennisdata_rows": float(total),
        "matched_rows": float(matched),
        "match_rate": float(matched / total) if total else float("nan"),
    }
    return joined, diagnostics
