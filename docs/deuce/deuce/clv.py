"""CLV measurement (Phase B/C) — the test that actually proves edge.

CLV = how much better your entry price was than the close. Phase A only tells
you whether DEUCE is calibrated vs a near-efficient market; this tells you
whether acting on its disagreements gets you a better price than the closing
line. Positive, persistent CLV over >=50-100 bets per segment is the gate.

Phase B uses The Odds API HISTORICAL snapshots (entry vs close on past matches);
Phase C reuses the same code on the LIVE endpoint going forward.
"""
from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass

import numpy as np

from .data.odds_api import OddsAPI, h2h_prices
from .devig import devig
from .metrics import bootstrap_mean_ci
from .names import canonical_player

# a pricer maps (p1, p2, surface, best_of) -> P(p1 wins) or None if unknown
Pricer = Callable[[str, str, str, int], "float | None"]


@dataclass(frozen=True)
class ClvResult:
    player: str
    entry_price: float
    close_price: float
    clv_price: float   # entry/close - 1 ; >0 means you beat the close
    entry_prob: float  # devigged
    close_prob: float
    clv_prob: float    # close_prob - entry_prob ; >0 means you beat the close


def _find_event(snapshot_data: list[dict], p1: str, p2: str) -> dict | None:
    want = {canonical_player(p1), canonical_player(p2)}
    for ev in snapshot_data:
        got = {canonical_player(ev.get("home_team", "")), canonical_player(ev.get("away_team", ""))}
        if got == want:
            return ev
    return None


def measure_clv(
    api: OddsAPI,
    sport_key: str,
    entry_date_iso: str,
    close_date_iso: str,
    p1: str,
    p2: str,
    bet_player: str,
    devig_method: str = "shin",
) -> ClvResult | None:
    """Compare the bet side's price at an entry snapshot vs a closing snapshot.

    entry/close dates are ISO8601 UTC (e.g. T-12h and T-0 around commence_time).
    Returns None if the event/prices aren't found in either snapshot.
    """
    entry_snap = api.get_historical_odds(sport_key, entry_date_iso)
    close_snap = api.get_historical_odds(sport_key, close_date_iso)
    entry_ev = _find_event(entry_snap.get("data", []), p1, p2)
    close_ev = _find_event(close_snap.get("data", []), p1, p2)
    if not entry_ev or not close_ev:
        return None

    ep, cp = h2h_prices(entry_ev), h2h_prices(close_ev)
    # match the two outcome names to our players via canonical key
    def price_for(prices: dict[str, float], player: str) -> tuple[float, float] | None:
        ck = canonical_player(player)
        for name, pr in prices.items():
            if canonical_player(name) == ck:
                other = [v for k, v in prices.items() if canonical_player(k) != ck]
                return (pr, other[0]) if other else None
        return None

    e = price_for(ep, bet_player)
    c = price_for(cp, bet_player)
    if not e or not c:
        return None
    e_bet, e_other = e
    c_bet, c_other = c
    e_prob, _ = devig(e_bet, e_other, devig_method)
    c_prob, _ = devig(c_bet, c_other, devig_method)
    return ClvResult(
        player=bet_player,
        entry_price=e_bet,
        close_price=c_bet,
        clv_price=e_bet / c_bet - 1.0,
        entry_prob=e_prob,
        close_prob=c_prob,
        clv_prob=c_prob - e_prob,
    )


# --------------------------------------------------------------------------- #
# Snapshot-driven CLV harness (Phase B): flag DEUCE's edges, measure CLV       #
# --------------------------------------------------------------------------- #
def _pair_key(ev: dict) -> frozenset:
    return frozenset(
        {canonical_player(ev.get("home_team", "")), canonical_player(ev.get("away_team", ""))}
    )


def _prices_for(prices: dict[str, float], p1: str, p2: str) -> tuple[float, float] | None:
    c1, c2 = canonical_player(p1), canonical_player(p2)
    o1 = o2 = None
    for name, pr in prices.items():
        cn = canonical_player(name)
        if cn == c1:
            o1 = pr
        elif cn == c2:
            o2 = pr
    return (o1, o2) if (o1 and o2) else None


@dataclass(frozen=True)
class ClvBet:
    p1: str
    p2: str
    bet_player: str
    edge: float          # model_prob - entry_market_prob, on the bet side (>0)
    entry_price: float
    close_price: float
    clv_price: float     # entry/close - 1 ; >0 = beat the close
    clv_prob: float      # close_devig - entry_devig (bet side) ; >0 = beat the close


def clv_bets_from_snapshots(
    entry_data: list[dict],
    close_data: list[dict],
    pricer: Pricer,
    *,
    surface: str,
    best_of: int = 3,
    threshold: float = 0.05,
    devig_method: str = "shin",
) -> list[ClvBet]:
    """For every match present in BOTH snapshots, price it with DEUCE; if the
    model disagrees with the entry-devigged market by >= threshold, "bet" the
    underpriced side and record its CLV (entry vs close). Pure given `pricer`,
    so testable without the API."""
    close_idx = {_pair_key(ev): ev for ev in close_data}
    bets: list[ClvBet] = []
    for ev in entry_data:
        close_ev = close_idx.get(_pair_key(ev))
        if close_ev is None:
            continue
        p1, p2 = ev.get("home_team"), ev.get("away_team")
        if not p1 or not p2:
            continue
        model_p1 = pricer(p1, p2, surface, best_of)
        if model_p1 is None:
            continue
        ep = _prices_for(h2h_prices(ev), p1, p2)
        cp = _prices_for(h2h_prices(close_ev), p1, p2)
        if ep is None or cp is None:
            continue
        (e1, e2), (c1, c2) = ep, cp
        entry_p1, entry_p2 = devig(e1, e2, devig_method)
        close_p1, close_p2 = devig(c1, c2, devig_method)
        edge1 = model_p1 - entry_p1
        if abs(edge1) < threshold:
            continue
        if edge1 > 0:  # model says p1 underpriced
            bet, ein, cl, ep_b, cp_b, edge = p1, e1, c1, entry_p1, close_p1, edge1
        else:
            bet, ein, cl, ep_b, cp_b, edge = p2, e2, c2, entry_p2, close_p2, -edge1
        bets.append(
            ClvBet(
                p1=p1, p2=p2, bet_player=bet, edge=edge,
                entry_price=ein, close_price=cl,
                clv_price=ein / cl - 1.0, clv_prob=cp_b - ep_b,
            )
        )
    return bets


def summarize_clv(bets: list[ClvBet], n_boot: int = 2000, seed: int = 7) -> dict:
    """Mean CLV with a bootstrap CI. beats_close = CI on mean clv_price fully > 0
    — the Phase-B/C edge gate (needs >=50-100 bets per segment to trust)."""
    n = len(bets)
    if n == 0:
        return {"n": 0, "mean_clv_price": float("nan"), "ci": (float("nan"), float("nan")),
                "pct_positive": float("nan"), "mean_clv_prob": float("nan"), "beats_close": False}
    cprice = np.array([b.clv_price for b in bets])
    cprob = np.array([b.clv_prob for b in bets])
    mean_p, lo, hi = bootstrap_mean_ci(cprice, n_boot, seed)
    return {
        "n": n,
        "mean_clv_price": mean_p,
        "ci": (lo, hi),
        "pct_positive": float(np.mean(cprice > 0)),
        "mean_clv_prob": float(cprob.mean()),
        "beats_close": lo > 0,
    }
