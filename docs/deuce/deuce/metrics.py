"""Scoring: log-loss, Brier, calibration, ECE, and bootstrap CIs.

These judge a probabilistic forecast against binary outcomes. The headline
number for Phase A is the DELTA in log-loss between DEUCE and the devigged
market on the same matches; the bootstrap CI tells you whether any apparent
edge is real or noise.
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np

_EPS = 1e-12


def _clip(p: np.ndarray) -> np.ndarray:
    return np.clip(p, _EPS, 1.0 - _EPS)


def log_loss(y: np.ndarray, p: np.ndarray) -> float:
    """Mean negative log-likelihood. Lower is better; perfect = 0."""
    y, p = np.asarray(y, float), _clip(np.asarray(p, float))
    return float(-np.mean(y * np.log(p) + (1.0 - y) * np.log(1.0 - p)))


def brier(y: np.ndarray, p: np.ndarray) -> float:
    """Mean squared error of the probability. Lower is better; perfect = 0."""
    y, p = np.asarray(y, float), np.asarray(p, float)
    return float(np.mean((p - y) ** 2))


def accuracy(y: np.ndarray, p: np.ndarray, threshold: float = 0.5) -> float:
    y, p = np.asarray(y, float), np.asarray(p, float)
    return float(np.mean((p >= threshold).astype(float) == y))


@dataclass(frozen=True)
class CalibrationBin:
    lo: float
    hi: float
    n: int
    mean_pred: float
    mean_obs: float


def calibration_table(y: np.ndarray, p: np.ndarray, bins: int = 10) -> list[CalibrationBin]:
    """Reliability table: predicted vs observed frequency per probability bin."""
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
    return float(
        sum(b.n * abs(b.mean_pred - b.mean_obs) for b in table if b.n > 0) / total
    )


def bootstrap_ci(
    metric_fn,
    y: np.ndarray,
    p: np.ndarray,
    n_boot: int = 2000,
    seed: int = 7,
    alpha: float = 0.05,
) -> tuple[float, float, float]:
    """Return (point, lo, hi) for metric_fn(y, p) via paired resampling.

    Deterministic given `seed` — no wall-clock entropy, so backtest results
    reproduce exactly (mirrors FORTUNA's determinism discipline).
    """
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
    p_market: np.ndarray,
    n_boot: int = 2000,
    seed: int = 7,
    alpha: float = 0.05,
) -> tuple[float, float, float]:
    """CI for (model_logloss - market_logloss) on the SAME matches.

    Negative and CI fully below 0 => DEUCE genuinely beats the close in that
    segment. This is the Phase-A gate.
    """
    y = np.asarray(y, float)
    pm, pk = _clip(np.asarray(p_model, float)), _clip(np.asarray(p_market, float))
    ll_model = -(y * np.log(pm) + (1 - y) * np.log(1 - pm))
    ll_market = -(y * np.log(pk) + (1 - y) * np.log(1 - pk))
    diff = ll_model - ll_market  # per-match paired difference
    rng = np.random.default_rng(seed)
    n = len(diff)
    point = float(diff.mean())
    if n == 0:
        return point, float("nan"), float("nan")
    boots = np.array([diff[rng.integers(0, n, n)].mean() for _ in range(n_boot)])
    lo, hi = np.quantile(boots, [alpha / 2, 1 - alpha / 2])
    return point, float(lo), float(hi)


def bootstrap_mean_ci(
    values: np.ndarray,
    n_boot: int = 2000,
    seed: int = 7,
    alpha: float = 0.05,
) -> tuple[float, float, float]:
    """(mean, lo, hi) of a 1-D sample via bootstrap. Used for CLV: mean CLV with
    its CI excluding 0 is the edge signal. Deterministic given `seed`.
    """
    v = np.asarray(values, float)
    n = len(v)
    if n == 0:
        return float("nan"), float("nan"), float("nan")
    rng = np.random.default_rng(seed)
    boots = np.array([v[rng.integers(0, n, n)].mean() for _ in range(n_boot)])
    lo, hi = np.quantile(boots, [alpha / 2, 1 - alpha / 2])
    return float(v.mean()), float(lo), float(hi)
