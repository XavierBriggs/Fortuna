"""Probability recalibration — per-regime, prequential.

The Phase-A backtest showed DEUCE is OVERCONFIDENT, but a single global Platt map
over-shrinks the regimes that were already sharp (best-of-5 Slams, heavy
favourites) while fixing the toss-ups. Two fixes here:

  1. PER-REGIME: fit a separate calibrator per regime (default best_of, i.e.
     Bo3 vs Bo5), since the favourite's true edge genuinely differs by format.
  2. ISOTONIC option: a monotone non-parametric map (PAV) that can be flat in the
     middle and steep at the extremes — so it can de-bias overconfident toss-ups
     WITHOUT distorting well-calibrated heavy favourites, which Platt's rigid
     logit-linear form cannot.

Applied PREQUENTIALLY (refit each year on only prior-year, in-regime predictions),
it never sees the matches it scores — no leakage. None of this BEATS the close;
it sharpens the model so it doesn't fabricate fake "value" before the CLV test.
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np
import pandas as pd

_EPS = 1e-12


def sigmoid(z: np.ndarray) -> np.ndarray:
    return 1.0 / (1.0 + np.exp(-z))


def logit(p: np.ndarray) -> np.ndarray:
    p = np.clip(p, _EPS, 1.0 - _EPS)
    return np.log(p / (1.0 - p))


# --------------------------------------------------------------------------- #
# Platt / temperature (logit-linear)                                          #
# --------------------------------------------------------------------------- #
def _newton_logistic(
    x: np.ndarray, y: np.ndarray, fit_intercept: bool, l2: float, iters: int
) -> tuple[float, float]:
    """1-D logistic fit by Newton-Raphson, ridge-regularized toward identity (a=1,b=0)."""
    cols = [x, np.ones_like(x)] if fit_intercept else [x]
    big_x = np.column_stack(cols)
    k = big_x.shape[1]
    theta = np.array([1.0, 0.0])[:k]   # init at identity
    target = np.array([1.0, 0.0])[:k]  # regularize toward identity, not zero
    for _ in range(iters):
        p = sigmoid(big_x @ theta)
        w = np.clip(p * (1.0 - p), 1e-9, None)
        grad = big_x.T @ (p - y) + l2 * (theta - target)
        hess = (big_x.T * w) @ big_x + l2 * np.eye(k)
        step = np.linalg.solve(hess, grad)
        theta = theta - step
        if np.max(np.abs(step)) < 1e-9:
            break
    a = float(theta[0])
    b = float(theta[1]) if fit_intercept else 0.0
    return a, b


@dataclass(frozen=True)
class LogisticCalibrator:
    a: float = 1.0
    b: float = 0.0

    @classmethod
    def fit(
        cls, p_raw: np.ndarray, y: np.ndarray, method: str = "platt",
        l2: float = 1e-3, iters: int = 50,
    ) -> "LogisticCalibrator":
        x = logit(np.asarray(p_raw, float))
        y = np.asarray(y, float)
        fit_intercept = method != "temperature"  # temperature = scale only (b=0)
        a, b = _newton_logistic(x, y, fit_intercept, l2, iters)
        return cls(a, b)

    def transform(self, p_raw: np.ndarray) -> np.ndarray:
        return sigmoid(self.a * logit(np.asarray(p_raw, float)) + self.b)


# --------------------------------------------------------------------------- #
# Isotonic (monotone, non-parametric) via Pool Adjacent Violators            #
# --------------------------------------------------------------------------- #
def _pava(y: np.ndarray) -> np.ndarray:
    """Pool Adjacent Violators: nearest non-decreasing fit (unit weights).

    Input y must already be ordered by the predictor x ascending. Returns the
    fitted non-decreasing values aligned to that order.
    """
    means: list[float] = []
    weights: list[float] = []
    sizes: list[int] = []
    for yi in y.astype(float):
        cm, cw, cs = float(yi), 1.0, 1
        while means and means[-1] >= cm:
            pm, pw, ps = means.pop(), weights.pop(), sizes.pop()
            cm = (pm * pw + cm * cw) / (pw + cw)
            cw += pw
            cs += ps
        means.append(cm)
        weights.append(cw)
        sizes.append(cs)
    fitted = np.empty(len(y))
    pos = 0
    for m, s in zip(means, sizes):
        fitted[pos:pos + s] = m
        pos += s
    return fitted


@dataclass
class IsotonicCalibrator:
    xs: np.ndarray  # sorted unique raw probabilities (interpolation knots)
    ys: np.ndarray  # fitted calibrated probabilities (non-decreasing)

    @classmethod
    def fit(cls, p_raw: np.ndarray, y: np.ndarray) -> "IsotonicCalibrator":
        x = np.asarray(p_raw, float)
        yv = np.asarray(y, float)
        order = np.argsort(x, kind="mergesort")
        xs, ys_sorted = x[order], yv[order]
        fitted = _pava(ys_sorted)
        xu, first = np.unique(xs, return_index=True)
        return cls(xu, fitted[first])

    def transform(self, p_raw: np.ndarray) -> np.ndarray:
        # np.interp clamps to the endpoints, which is the safe extrapolation here
        out = np.interp(np.asarray(p_raw, float), self.xs, self.ys)
        return np.clip(out, 1e-6, 1.0 - 1e-6)


def _fit_calibrator(method: str, p_raw: np.ndarray, y: np.ndarray):
    if method == "isotonic":
        return IsotonicCalibrator.fit(p_raw, y)
    if method in ("platt", "temperature"):
        return LogisticCalibrator.fit(p_raw, y, method)
    raise ValueError(f"unknown calibration method: {method!r}")


# --------------------------------------------------------------------------- #
# Prequential, per-regime application                                         #
# --------------------------------------------------------------------------- #
def prequential_calibrate(
    res: pd.DataFrame,
    method: str = "isotonic",
    min_fit: int = 1000,
    regime_col: str | None = "best_of",
) -> pd.DataFrame:
    """Add `p_cal`: each year's predictions calibrated by a per-regime map fit on
    only STRICTLY PRIOR years, in the same regime. Passthrough until a regime has
    min_fit prior matches. regime_col=None collapses to a single global map.
    """
    res = res.sort_values("date").reset_index(drop=True)
    years = res["year"].to_numpy()
    p_raw = res["p_model"].to_numpy(float)
    y = res["y"].to_numpy(float)
    p_cal = p_raw.copy()
    if regime_col and regime_col in res.columns:
        regimes = res[regime_col].to_numpy()
    else:
        regimes = np.zeros(len(res))

    cur: dict = {}  # regime -> fitted calibrator
    for yr in sorted(set(years.tolist())):
        prior = years < yr
        year_mask = years == yr
        for rg in sorted(set(regimes[year_mask].tolist())):
            rg_prior = prior & (regimes == rg)
            if int(rg_prior.sum()) >= min_fit:
                cur[rg] = _fit_calibrator(method, p_raw[rg_prior], y[rg_prior])
            if rg in cur:
                m = year_mask & (regimes == rg)
                p_cal[m] = cur[rg].transform(p_raw[m])
    res["p_cal"] = p_cal
    return res
