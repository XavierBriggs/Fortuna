"""RatingBook — DEUCE as a live pricer.

The backtest runs Elo forward over tennis-data; to price an UPCOMING match we
need current ratings plus a calibrator fit on all history, keyed by CANONICAL
player name so The Odds API's "Carlos Alcaraz" resolves to tennis-data's
"Alcaraz C.". Unknown players (no rating history) return None — we never bet a
coinflip.
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np
import pandas as pd

from .calibrate import _fit_calibrator
from .config import BacktestConfig
from .elo import EloEngine
from .names import canonical_player


def _asof_naive(asof) -> pd.Timestamp:
    ts = pd.Timestamp(asof)
    if ts.tzinfo is not None:
        ts = ts.tz_convert("UTC").tz_localize(None)
    return ts


@dataclass
class RatingBook:
    engine: EloEngine
    calibrators: dict  # best_of(int) -> fitted calibrator
    cfg: BacktestConfig

    @classmethod
    def build(cls, cfg: BacktestConfig, asof=None) -> "RatingBook":
        """Walk all tennis-data (<= asof) to current ratings, keyed canonically,
        and fit per-regime calibrators on the both-known predictions."""
        from .data.tennisdata import load_tennisdata

        df = load_tennisdata(tour=cfg.tour, since=None)
        if asof is not None:
            df = df[df["date"] <= _asof_naive(asof)]
        engine = EloEngine(cfg.elo)
        praw: dict[int, list[float]] = {}
        ys: dict[int, list[float]] = {}
        for r in df.itertuples(index=False):
            surface = r.surface or "unknown"
            k1, k2 = canonical_player(r.p1), canonical_player(r.p2)
            p = engine.predict(k1, k2, surface)
            bo = int(r.best_of) if r.best_of == r.best_of and r.best_of is not None else 0
            # only both-known predictions (real, not coinflips) feed the calibrator
            if r.completed and engine.has_player(k1) and engine.has_player(k2):
                praw.setdefault(bo, []).append(p)
                ys.setdefault(bo, []).append(float(r.y))
            if r.completed and (r.w_games + r.l_games) > 0:
                winner, loser = (k1, k2) if r.y == 1 else (k2, k1)
                engine.update(winner, loser, surface, r.w_games, r.l_games)

        calibrators: dict[int, object] = {}
        if cfg.calibrate:
            for bo, plist in praw.items():
                if len(plist) >= cfg.calib_min_fit:
                    calibrators[bo] = _fit_calibrator(
                        cfg.calib_method, np.array(plist), np.array(ys[bo])
                    )
        return cls(engine, calibrators, cfg)

    def predict(self, name1: str, name2: str, surface: str, best_of: int = 3) -> float | None:
        """Calibrated P(name1 beats name2). None if either player is unknown."""
        k1, k2 = canonical_player(name1), canonical_player(name2)
        if not (self.engine.has_player(k1) and self.engine.has_player(k2)):
            return None
        p = self.engine.predict(k1, k2, surface or "unknown")
        cal = self.calibrators.get(int(best_of))
        if cal is not None:
            p = float(cal.transform(np.array([p]))[0])
        return p
