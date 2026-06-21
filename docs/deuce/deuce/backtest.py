"""Walk-forward Phase-A backtest: DEUCE (raw + recalibrated) vs the devigged close.

Chronological, no shuffling: for each match we PREDICT with only-past ratings,
record the scored row, THEN update. Ratings warm up during the burn-in seasons
(not scored). Recalibration is prequential (see calibrate.py). The report is
always per-segment — never one global number.
"""
from __future__ import annotations

from dataclasses import replace

import pandas as pd

from .calibrate import prequential_calibrate
from .config import BacktestConfig
from .devig import devig
from .elo import EloEngine
from .metrics import ece, log_loss, paired_logloss_delta_ci


def _walk_forward(df: pd.DataFrame, cfg: BacktestConfig) -> pd.DataFrame:
    """Run the Elo walk-forward over a preloaded frame; return scored rows.

    Scored = year >= since_year, valid odds, completed. Ratings still update on
    every completed match (incl. burn-in) so they are warm by the scoring window.
    """
    engine = EloEngine(cfg.elo)
    scored: list[dict] = []
    for r in df.itertuples(index=False):
        surface = r.surface or "unknown"
        p_model = engine.predict(r.p1, r.p2, surface)

        if r.year >= cfg.since_year and r.odds1 > 1.0 and r.odds2 > 1.0 and r.completed:
            p1_mkt, _ = devig(r.odds1, r.odds2, cfg.devig_method)
            scored.append(
                {
                    "date": r.date,
                    "year": r.year,
                    "surface": surface,
                    "series": r.series or "unknown",
                    # pandas coerces missing best_of to float NaN (NaN != NaN)
                    "best_of": int(r.best_of) if r.best_of == r.best_of and r.best_of is not None else 0,
                    "odds_bucket": r.odds_bucket,
                    "y": r.y,
                    "p_model": p_model,
                    "p_market": p1_mkt,
                }
            )

        # update AFTER predicting — winner/loser recovered from the label
        if r.completed and (r.w_games + r.l_games) > 0:
            winner, loser = (r.p1, r.p2) if r.y == 1 else (r.p2, r.p1)
            engine.update(winner, loser, surface, r.w_games, r.l_games)

    res = pd.DataFrame(scored)
    if res.empty:
        raise RuntimeError("No scored matches — check data range and odds availability.")
    return res


def run_backtest(cfg: BacktestConfig) -> tuple[pd.DataFrame, pd.DataFrame]:
    from .data.tennisdata import load_tennisdata

    df = load_tennisdata(tour=cfg.tour, since=cfg.since_year - cfg.burnin_years)
    res = _walk_forward(df, cfg)
    if cfg.calibrate:
        res = prequential_calibrate(
            res, cfg.calib_method, cfg.calib_min_fit, cfg.calib_regime or None
        )
    else:
        res = res.copy()
        res["p_cal"] = res["p_model"]
    return res, _report(res, cfg)


def _segment_row(name: str, value: str, g: pd.DataFrame, cfg: BacktestConfig) -> dict:
    y = g["y"].to_numpy(float)
    pr = g["p_model"].to_numpy(float)
    pc = g["p_cal"].to_numpy(float)
    pk = g["p_market"].to_numpy(float)
    delta, lo, hi = paired_logloss_delta_ci(y, pc, pk, cfg.bootstrap_n, cfg.seed)
    return {
        "segment": name,
        "value": value,
        "n": len(g),
        "raw_ll": round(log_loss(y, pr), 4),
        "cal_ll": round(log_loss(y, pc), 4),
        "mkt_ll": round(log_loss(y, pk), 4),
        "cal_vs_mkt": round(delta, 4),
        "cal_ci": f"[{lo:+.4f},{hi:+.4f}]",
        "beats_close": hi < 0,  # calibrated CI fully below 0 => beats the close
        "raw_ece": round(ece(y, pr), 4),
        "cal_ece": round(ece(y, pc), 4),
    }


def _report(res: pd.DataFrame, cfg: BacktestConfig) -> pd.DataFrame:
    rows = [_segment_row("ALL", "—", res, cfg)]
    for col in cfg.segment_cols:
        for value, g in res.groupby(col):
            if len(g) >= 100:  # don't report noise from tiny cells
                rows.append(_segment_row(col, str(value), g, cfg))
    return pd.DataFrame(rows)


def tune_params(
    base_cfg: BacktestConfig,
    split_year: int,
    surface_weights: list[float],
    k_bases: list[float],
    margin_scales: list[float],
) -> pd.DataFrame:
    """Grid-search Elo params. SELECT on train log-loss, REPORT on the holdout
    (>= split_year) so the choice can't peek at the evaluation window. Data is
    loaded once and reused across the grid.
    """
    from .data.tennisdata import load_tennisdata

    df = load_tennisdata(tour=base_cfg.tour, since=base_cfg.since_year - base_cfg.burnin_years)
    rows = []
    for sw in surface_weights:
        for kb in k_bases:
            for ms in margin_scales:
                elo = replace(base_cfg.elo, surface_weight=sw, k_base=kb, margin_scale=ms)
                res = _walk_forward(df, replace(base_cfg, elo=elo))
                train = res[res["year"] < split_year]
                hold = res[res["year"] >= split_year]
                if train.empty or hold.empty:
                    continue
                rows.append(
                    {
                        "surface_w": sw,
                        "k_base": kb,
                        "margin": ms,
                        "train_ll": round(log_loss(train["y"], train["p_model"]), 4),
                        "hold_ll": round(log_loss(hold["y"], hold["p_model"]), 4),
                        "hold_mkt": round(log_loss(hold["y"], hold["p_market"]), 4),
                        "n_hold": len(hold),
                    }
                )
    return pd.DataFrame(rows).sort_values("train_ll").reset_index(drop=True)


def format_report(report: pd.DataFrame) -> str:
    with pd.option_context("display.max_rows", None, "display.width", 220):
        return report.to_string(index=False)
