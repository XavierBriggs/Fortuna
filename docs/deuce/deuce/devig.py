"""Vig removal: convert two-way decimal odds into a fair probability.

The method matters in tennis because of favourite-longshot bias: naive
normalization over-shrinks favourites. Shin's method backs out an implied
"insider" fraction and de-biases accordingly. Both return (p1, p2) summing to 1.
"""
from __future__ import annotations

import math


def implied(odds: float) -> float:
    """Raw inverse odds (includes the book's margin)."""
    return 1.0 / odds


def proportional(odds1: float, odds2: float) -> tuple[float, float]:
    """Normalize inverse odds to sum to 1. Simple, slightly favourite-biased."""
    a, b = implied(odds1), implied(odds2)
    s = a + b
    return a / s, b / s


def _shin_prob(pi: float, z: float, o: float) -> float:
    return (math.sqrt(z * z + 4.0 * (1.0 - z) * pi * pi / o) - z) / (2.0 * (1.0 - z))


def shin(odds1: float, odds2: float) -> tuple[float, float]:
    """Shin (1992) two-outcome devig.

    With raw inverse odds pi_i and booksum O = sum(pi_i), recovered probability
        p_i(z) = ( sqrt(z^2 + 4(1-z) * pi_i^2 / O) - z ) / (2(1-z)),
    where the insider fraction z is solved so the p_i sum to 1. We bisect for z
    (robust for any book) rather than trust a fragile closed form: sum(p_i) is
    monotone decreasing in z, from sqrt(O) > 1 at z=0 to < 1 as z -> 1, so a root
    always exists for a vig book.
    """
    pi1, pi2 = implied(odds1), implied(odds2)
    o = pi1 + pi2
    if o <= 1.0:  # no margin to remove
        return proportional(odds1, odds2)
    lo, hi = 0.0, 1.0 - 1e-12
    for _ in range(100):
        z = 0.5 * (lo + hi)
        s = _shin_prob(pi1, z, o) + _shin_prob(pi2, z, o)
        if s > 1.0:
            lo = z  # too much mass -> raise z
        else:
            hi = z
    z = 0.5 * (lo + hi)
    p1, p2 = _shin_prob(pi1, z, o), _shin_prob(pi2, z, o)
    s = p1 + p2
    return p1 / s, p2 / s  # renormalize against float drift


def devig(odds1: float, odds2: float, method: str = "shin") -> tuple[float, float]:
    if method == "proportional":
        return proportional(odds1, odds2)
    if method == "shin":
        return shin(odds1, odds2)
    raise ValueError(f"unknown devig method: {method!r}")
