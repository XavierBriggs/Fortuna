"""Scoring: log-loss / Brier / ECE for binary over-unders, RPS for the count.

Two kinds of forecast get scored here. (1) A binary over/under call against the
realized result — log-loss, Brier, calibration, ECE, plus a paired bootstrap CI
for the model-minus-baseline log-loss delta (the Phase-A gate). (2) The full
strikeout COUNT distribution against the realized K total — the Ranked
Probability Score, the natural proper score for an ordinal count, which needs no
betting line at all. Model selection runs on these, never on ROI (ROI overfits;
see the memo).
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np

_EPS = 1e-12


def _clip(p: np.ndarray) -> np.ndarray:
    return np.clip(p, _EPS, 1.0 - _EPS)


def log_loss(y: np.ndarray, p: np.ndarray) -> float:
    """Mean negative log-likelihood of a binary outcome. Lower is better."""
    y, p = np.asarray(y, float), _clip(np.asarray(p, float))
    return float(-np.mean(y * np.log(p) + (1.0 - y) * np.log(1.0 - p)))


def brier(y: np.ndarray, p: np.ndarray) -> float:
    y, p = np.asarray(y, float), np.asarray(p, float)
    return float(np.mean((p - y) ** 2))


def ranked_probability_score(y_int: int, pmf: np.ndarray) -> float:
    """RPS for an ordinal count: sum_t (F_pred(t) - 1[y<=t])^2 over t.

    F_pred is the predicted CDF; the observed CDF is a step at the realized K.
    Perfect (all mass on y) scores 0; a more diffuse distribution scores higher.
    This is the count analogue of Brier and the cleanest no-line model score.
    """
    pmf = np.asarray(pmf, float)
    cdf_pred = np.cumsum(pmf)
    t = np.arange(len(pmf))
    cdf_obs = (t >= y_int).astype(float)
    return float(np.sum((cdf_pred - cdf_obs) ** 2))


def mean_rps(y_ints: np.ndarray, pmfs: list[np.ndarray]) -> float:
    return float(np.mean([ranked_probability_score(int(y), p) for y, p in zip(y_ints, pmfs)]))


@dataclass(frozen=True)
class CalibrationBin:
    lo: float
    hi: float
    n: int
    mean_pred: float
    mean_obs: float


def calibration_table(y: np.ndarray, p: np.ndarray, bins: int = 10) -> list[CalibrationBin]:
    y, p = np.asarray(y, float), np.asarray(p, float)
    edges = np.linspace(0.0, 1.0, bins + 1)
    out: list[CalibrationBin] = []
    for i in range(bins):
        lo, hi = edges[i], edges[i + 1]
        mask = (p >= lo) & (p < hi) if i < bins - 1 else (p >= lo) & (p <= hi)
        n = int(mask.sum())
        if n == 0:
            out.append(CalibrationBin(lo, hi, 0, float("nan"), float("nan")))
        else:
            out.append(CalibrationBin(lo, hi, n, float(p[mask].mean()), float(y[mask].mean())))
    return out


def ece(y: np.ndarray, p: np.ndarray, bins: int = 10) -> float:
    """Expected Calibration Error: n-weighted mean |pred - obs| across bins."""
    table = calibration_table(y, p, bins)
    total = sum(b.n for b in table)
    if total == 0:
        return float("nan")
    return float(sum(b.n * abs(b.mean_pred - b.mean_obs) for b in table if b.n > 0) / total)


def bootstrap_ci(metric_fn, y, p, n_boot: int = 2000, seed: int = 7, alpha: float = 0.05):
    """(point, lo, hi) for metric_fn(y, p) via deterministic paired resampling."""
    y, p = np.asarray(y, float), np.asarray(p, float)
    rng = np.random.default_rng(seed)
    n = len(y)
    point = metric_fn(y, p)
    if n == 0:
        return point, float("nan"), float("nan")
    stats = np.empty(n_boot)
    for b in range(n_boot):
        idx = rng.integers(0, n, n)
        stats[b] = metric_fn(y[idx], p[idx])
    lo, hi = np.quantile(stats, [alpha / 2, 1 - alpha / 2])
    return point, float(lo), float(hi)


def paired_logloss_delta_ci(
    y: np.ndarray,
    p_model: np.ndarray,
    p_base: np.ndarray,
    n_boot: int = 2000,
    seed: int = 7,
    alpha: float = 0.05,
) -> tuple[float, float, float]:
    """CI for (model_logloss - baseline_logloss) on the SAME starts.

    Negative with the CI fully below 0 => the matchup+leash model genuinely beats
    the naive trailing-K baseline in that segment. This is HEATER's Phase-A gate.
    """
    y = np.asarray(y, float)
    pm, pk = _clip(np.asarray(p_model, float)), _clip(np.asarray(p_base, float))
    ll_model = -(y * np.log(pm) + (1 - y) * np.log(1 - pm))
    ll_base = -(y * np.log(pk) + (1 - y) * np.log(1 - pk))
    diff = ll_model - ll_base
    rng = np.random.default_rng(seed)
    n = len(diff)
    point = float(diff.mean())
    if n == 0:
        return point, float("nan"), float("nan")
    boots = np.array([diff[rng.integers(0, n, n)].mean() for _ in range(n_boot)])
    lo, hi = np.quantile(boots, [alpha / 2, 1 - alpha / 2])
    return point, float(lo), float(hi)
