"""Vig removal for a two-way over/under prop: fair P(over), P(under).

A posted strikeout prop is two-sided decimal odds whose inverse-probabilities sum
to >1 (the book's hold — typically 6-10% on props, much fatter than game lines).
You must compare your model to the FAIR price, not the posted one, or the vig
manufactures fake edge. Proportional normalization is the simple default; Shin
(1992) de-biases for the longshot tilt. Both return (p_over, p_under) summing to 1.
"""
from __future__ import annotations

import math


def implied(odds: float) -> float:
    """Raw inverse odds (includes the book's margin)."""
    return 1.0 / odds


def proportional(odds_over: float, odds_under: float) -> tuple[float, float]:
    """Normalize inverse odds to sum to 1. Simple, slightly favourite-biased."""
    a, b = implied(odds_over), implied(odds_under)
    s = a + b
    return a / s, b / s


def _shin_prob(pi: float, z: float, o: float) -> float:
    return (math.sqrt(z * z + 4.0 * (1.0 - z) * pi * pi / o) - z) / (2.0 * (1.0 - z))


def shin(odds_over: float, odds_under: float) -> tuple[float, float]:
    """Shin (1992) two-outcome devig via bisection on the insider fraction z.

    sum(p_i) is monotone decreasing in z (from sqrt(O) > 1 at z=0), so a root
    always exists for a vig book; we bisect rather than trust a fragile closed form.
    """
    pi1, pi2 = implied(odds_over), implied(odds_under)
    o = pi1 + pi2
    if o <= 1.0:  # no margin to remove
        return proportional(odds_over, odds_under)
    lo, hi = 0.0, 1.0 - 1e-12
    for _ in range(100):
        z = 0.5 * (lo + hi)
        s = _shin_prob(pi1, z, o) + _shin_prob(pi2, z, o)
        if s > 1.0:
            lo = z
        else:
            hi = z
    z = 0.5 * (lo + hi)
    p1, p2 = _shin_prob(pi1, z, o), _shin_prob(pi2, z, o)
    s = p1 + p2
    return p1 / s, p2 / s


def devig(odds_over: float, odds_under: float, method: str = "shin") -> tuple[float, float]:
    if method == "proportional":
        return proportional(odds_over, odds_under)
    if method == "shin":
        return shin(odds_over, odds_under)
    raise ValueError(f"unknown devig method: {method!r}")
