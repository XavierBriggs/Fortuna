"""log5 / odds-ratio matchup math for per-PA strikeout probability.

The chance a given pitcher strikes out a given opponent isn't the pitcher's rate
or the opponent's rate — it's the two combined against the league baseline. Bill
James' log5 (the odds-ratio method) does exactly that:

    q = (K_pit * K_opp / K_lg) / ( K_pit*K_opp/K_lg + (1-K_pit)*(1-K_opp)/(1-K_lg) )

Properties this guarantees (all tested): a league-average opponent returns the
pitcher's own rate; the form is symmetric in pitcher/opponent; q is monotone in
both inputs and always in (0,1). A park multiplier nudges the odds afterward.
This module is pure and deterministic.
"""
from __future__ import annotations

_EPS = 1e-9


def _clip(p: float) -> float:
    return min(max(p, _EPS), 1.0 - _EPS)


def matchup_k(k_pitcher: float, k_opponent: float, k_league: float) -> float:
    """Per-PA P(strikeout) for this pitcher vs this opponent (log5 odds-ratio).

    All three inputs are per-PA strikeout rates in (0,1). When `k_opponent ==
    k_league` the result is exactly `k_pitcher` (a league-average bat tells you
    nothing beyond the pitcher); symmetric in the two rates.
    """
    kp, ko, kl = _clip(k_pitcher), _clip(k_opponent), _clip(k_league)
    num = (kp * ko) / kl
    den = num + ((1.0 - kp) * (1.0 - ko)) / (1.0 - kl)
    return num / den


def apply_park(q: float, park_mult: float) -> float:
    """Nudge a per-PA K probability by a park multiplier, in odds space.

    Multiplying the odds (not the probability) keeps the result in (0,1) for any
    positive multiplier. park_mult > 1 is a strikeout-friendly park.
    """
    q = _clip(q)
    odds = q / (1.0 - q) * max(park_mult, _EPS)
    return odds / (1.0 + odds)
