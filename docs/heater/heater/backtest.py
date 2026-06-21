"""Phase-A backtest: the HEATER matchup+leash model vs the naive trailing-K
baseline, scored on free realized outcomes.

The cheap, decisive question (memo finding 1 & 4): does adjusting for the
opponent (log5) and treating the leash as a DISTRIBUTION beat just projecting a
pitcher's own trailing K rate at a fixed batters-faced count? We score two ways
per start — Ranked Probability Score on the full K distribution (no line needed)
and log-loss on a synthetic over/under — and report per-segment with a paired
bootstrap CI. No market column is required; CLV against real prop lines is Phase B
(`clv.py`). Selection is on RPS / log-loss, never ROI.
"""
from __future__ import annotations

import math
from dataclasses import replace
from math import floor

import numpy as np
import pandas as pd

from .config import BacktestConfig, KModelConfig
from .metrics import ece, log_loss, mean_rps, paired_logloss_delta_ci
from .model import baseline_pmf, heater_pmf


def _pick_prior(row, source: str) -> float:
    """Choose the pitcher K% prior column the model uses. Falls back to the
    trailing prior when an alternate (csw/blend) column is absent or NaN — so the
    synthetic demo (trailing only) and real Statcast data share one code path."""
    if source == "csw":
        v = getattr(row, "pit_k_csw", float("nan"))
        if isinstance(v, (int, float)) and not math.isnan(v):
            return float(v)
    elif source == "blend":
        v = getattr(row, "pit_k_blend", float("nan"))
        if isinstance(v, (int, float)) and not math.isnan(v):
            return float(v)
    return float(row.pit_k_prior)


def _k_tier(k: float) -> str:
    if k >= 0.28:
        return "power>=0.28"
    if k >= 0.23:
        return "above0.23-0.28"
    if k >= 0.19:
        return "avg0.19-0.23"
    return "contact<0.19"


def _bf_tier(bf: float) -> str:
    if bf >= 25:
        return "workhorse>=25"
    if bf >= 21:
        return "average21-25"
    return "shortleash<21"


def _score_rows(df: pd.DataFrame, cfg: BacktestConfig) -> pd.DataFrame:
    from .kdist import expected_k, p_over

    m = cfg.model
    scored: list[dict] = []
    h_pmfs: list[np.ndarray] = []
    b_pmfs: list[np.ndarray] = []
    for r in df.itertuples(index=False):
        if r.season < cfg.season_from:
            continue
        pit_k = _pick_prior(r, m.k_prior_source)
        h = heater_pmf(pit_k, r.opp_k_prior, r.proj_bf_mean, r.proj_bf_sd, r.park_k_mult, m)
        b = baseline_pmf(r.pit_k_prior, r.proj_bf_mean, m)
        # market-agnostic synthetic line: a "naive book" anchors on the baseline mean
        line = floor(expected_k(b)) + cfg.line_offset
        y_over = 1.0 if r.realized_k > line else 0.0
        scored.append(
            {
                "season": r.season,
                "k_tier": _k_tier(r.pit_k_prior),
                "bf_tier": _bf_tier(r.proj_bf_mean),
                "realized_k": int(r.realized_k),
                "line": line,
                "y_over": y_over,
                "p_over_heater": p_over(line, h),
                "p_over_base": p_over(line, b),
            }
        )
        h_pmfs.append(h)
        b_pmfs.append(b)
    if not scored:
        raise RuntimeError("No scored starts — check season_from vs the data range.")
    res = pd.DataFrame(scored)
    res["_h_pmf"] = h_pmfs
    res["_b_pmf"] = b_pmfs
    return res


def _segment_row(name: str, value: str, g: pd.DataFrame, cfg: BacktestConfig) -> dict:
    y = g["y_over"].to_numpy(float)
    ph = g["p_over_heater"].to_numpy(float)
    pb = g["p_over_base"].to_numpy(float)
    delta, lo, hi = paired_logloss_delta_ci(y, ph, pb, cfg.bootstrap_n, cfg.seed)
    ks = g["realized_k"].to_numpy(int)
    return {
        "segment": name,
        "value": value,
        "n": len(g),
        "base_ll": round(log_loss(y, pb), 4),
        "heater_ll": round(log_loss(y, ph), 4),
        "ll_delta": round(delta, 4),
        "ll_ci": f"[{lo:+.4f},{hi:+.4f}]",
        "beats_base": bool(hi < 0),  # CI fully below 0 => HEATER genuinely sharper
        "base_rps": round(mean_rps(ks, list(g["_b_pmf"])), 4),
        "heater_rps": round(mean_rps(ks, list(g["_h_pmf"])), 4),
        "heater_ece": round(ece(y, ph), 4),
    }


def _report(res: pd.DataFrame, cfg: BacktestConfig) -> pd.DataFrame:
    rows = [_segment_row("ALL", "—", res, cfg)]
    for col in cfg.segment_cols:
        for value, g in res.groupby(col):
            if len(g) >= 100:
                rows.append(_segment_row(col, str(value), g, cfg))
    return pd.DataFrame(rows)


def run_backtest(cfg: BacktestConfig, df: pd.DataFrame) -> tuple[pd.DataFrame, pd.DataFrame]:
    """Score a preloaded frame (synthetic or Statcast); return (scored, report)."""
    res = _score_rows(df, cfg)
    return res, _report(res, cfg)


def format_report(report: pd.DataFrame) -> str:
    with pd.option_context("display.max_rows", None, "display.width", 220):
        return report.to_string(index=False)


# --- ablation: decompose which model components carry the win ---

ABLATION_VARIANTS: list[tuple[str, dict]] = [
    ("baseline", dict(use_opponent=False, use_leash_spread=False, use_park=False)),
    ("leash_only", dict(use_opponent=False, use_leash_spread=True, use_park=False)),
    ("opponent_only", dict(use_opponent=True, use_leash_spread=False, use_park=False)),
    ("opp+leash", dict(use_opponent=True, use_leash_spread=True, use_park=False)),
    ("opp+leash+park", dict(use_opponent=True, use_leash_spread=True, use_park=True)),
    ("+csw_blend", dict(use_opponent=True, use_leash_spread=True, use_park=True, k_prior_source="blend")),
    ("csw_prior", dict(use_opponent=True, use_leash_spread=True, use_park=True, k_prior_source="csw")),
]


def ablate(base_cfg: BacktestConfig, df: pd.DataFrame) -> pd.DataFrame:
    """Score each model variant on the SAME starts and the SAME (baseline-anchored)
    over/under line, so the deltas isolate each component's marginal contribution.
    RPS is line-independent; the fixed line makes the log-loss column comparable too.
    """
    from .kdist import expected_k, p_over

    m0 = base_cfg.model
    rows = [r for r in df.itertuples(index=False) if r.season >= base_cfg.season_from]
    if not rows:
        raise RuntimeError("No scored starts — check season_from vs the data range.")
    # fixed line + realized over/under, from the (variant-independent) baseline pmf
    lines, ys, ks = [], [], []
    for r in rows:
        b = baseline_pmf(r.pit_k_prior, r.proj_bf_mean, m0)
        line = floor(expected_k(b)) + base_cfg.line_offset
        lines.append(line)
        ys.append(1.0 if r.realized_k > line else 0.0)
        ks.append(int(r.realized_k))
    y = np.array(ys, float)
    ks_arr = np.array(ks, int)

    out = []
    for name, over in ABLATION_VARIANTS:
        mcfg: KModelConfig = replace(m0, **over)
        ph, pmfs = [], []
        for r, line in zip(rows, lines):
            pit_k = _pick_prior(r, mcfg.k_prior_source)
            pm = heater_pmf(pit_k, r.opp_k_prior, r.proj_bf_mean, r.proj_bf_sd, r.park_k_mult, mcfg)
            ph.append(p_over(line, pm))
            pmfs.append(pm)
        out.append(
            {
                "variant": name,
                "n": len(rows),
                "ll": round(log_loss(y, np.array(ph)), 4),
                "rps": round(mean_rps(ks_arr, pmfs), 4),
                "ece": round(ece(y, np.array(ph)), 4),
            }
        )
    res = pd.DataFrame(out)
    base = res.iloc[0]
    res["ll_vs_base"] = (res["ll"] - base["ll"]).round(4)
    res["rps_vs_base"] = (res["rps"] - base["rps"]).round(4)
    return res
