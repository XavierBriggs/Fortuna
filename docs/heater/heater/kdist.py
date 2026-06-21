"""Strikeout count distribution for a single start.

A pitcher's strikeout total is a COMPOUND distribution: K | BF ~ Binomial(BF, q),
where BF (batters faced) is itself uncertain because the leash — when the manager
pulls him — is the dominant driver of strikeout variance. We model BF as a
distribution and marginalize it out:

    P(K=k) = sum_bf  P(BF=bf) * Binomial(k; bf, q)

Letting BF vary (instead of fixing it) is what produces the overdispersion real
strikeout totals show — the practitioner-preferred route over curve-fitting a
negative binomial (see docs/research/2026-06-18-baseball-modeling.md, finding 4).
Everything here is pure, deterministic, and depends only on numpy.
"""
from __future__ import annotations

from math import comb

import numpy as np

_EPS = 1e-12


def binom_pmf(n: int, q: float) -> np.ndarray:
    """P(K=k) for k=0..n under K~Binomial(n, q). Exact; normalized to guard drift."""
    q = min(max(q, _EPS), 1.0 - _EPS)
    ks = np.arange(n + 1)
    vals = np.array([comb(n, int(k)) * q**k * (1.0 - q) ** (n - k) for k in ks], float)
    s = vals.sum()
    return vals / s if s > 0 else vals


def bf_pmf(mean: float, sd: float, lo: int, hi: int) -> tuple[np.ndarray, np.ndarray]:
    """Discrete batters-faced distribution over integers [lo, hi].

    A truncated Gaussian kernel for sd > 0; a clamped point mass for sd <= 0
    (which collapses the compound back to a plain Binomial — used in tests and as
    the naive baseline). Returns (bf_values, weights) with weights summing to 1.
    """
    bfs = np.arange(lo, hi + 1)
    if sd <= 0:
        point = int(round(min(max(mean, lo), hi)))
        w = (bfs == point).astype(float)
    else:
        w = np.exp(-0.5 * ((bfs - mean) / sd) ** 2)
    total = w.sum()
    return bfs, (w / total if total > 0 else np.ones_like(w) / len(w))


def k_pmf(q: float, bfs: np.ndarray, bf_w: np.ndarray) -> np.ndarray:
    """Compound strikeout pmf over k=0..max(bf): sum_bf w[bf] * Binomial(k; bf, q)."""
    max_bf = int(bfs.max())
    out = np.zeros(max_bf + 1)
    for bf, w in zip(bfs, bf_w):
        bf = int(bf)
        out[: bf + 1] += w * binom_pmf(bf, q)
    s = out.sum()
    return out / s if s > 0 else out


def p_over(line: float, pmf: np.ndarray) -> float:
    """P(K > line). For the usual X.5 prop line there is no push mass."""
    ks = np.arange(len(pmf))
    return float(pmf[ks > line].sum())


def expected_k(pmf: np.ndarray) -> float:
    ks = np.arange(len(pmf))
    return float((ks * pmf).sum())


def variance_k(pmf: np.ndarray) -> float:
    ks = np.arange(len(pmf))
    mu = float((ks * pmf).sum())
    return float(((ks - mu) ** 2 * pmf).sum())
