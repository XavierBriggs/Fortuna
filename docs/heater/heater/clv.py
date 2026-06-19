"""CLV measurement for a strikeout prop (Phase B/C) — the test that proves edge.

Phase A only tells you the model is well-calibrated and beats a naive baseline on
realized outcomes. It does NOT tell you that acting on the model's disagreements
gets you a better price than the closing line. That is CLV: bet a side (Over or
Under) at an entry snapshot, then compare your entry's fair probability to the
close's. Positive, persistent CLV over >=50-100 bets per segment — measured against
the DE-VIGGED close, not the posted price — is the only gate that matters before
capital (FORTUNA's I7 forward-validation discipline).

Phase B replays historical Odds API snapshots; Phase C reuses this on the live
endpoint going forward.
"""
from __future__ import annotations

from dataclasses import dataclass

from .data.odds_api import OddsAPI, pitcher_over_under
from .devig import devig


@dataclass(frozen=True)
class ClvResult:
    pitcher: str
    side: str           # "over" | "under"
    entry_price: float
    close_price: float
    clv_price: float    # entry/close - 1 ; >0 means you locked a longer price
    entry_prob: float   # devigged fair prob of YOUR side at entry
    close_prob: float   # devigged fair prob of YOUR side at close
    clv_prob: float     # close_prob - entry_prob ; >0 means the line moved your way


def measure_k_clv(
    api: OddsAPI,
    event_id: str,
    pitcher: str,
    entry_date_iso: str,
    close_date_iso: str,
    side: str,
    devig_method: str = "shin",
) -> ClvResult | None:
    """Compare the bet side's price at an entry snapshot vs a closing snapshot.

    Returns None if the pitcher's prop isn't quoted in either snapshot. `side` is
    'over' or 'under'. The two snapshots should bracket commence_time (e.g. T-3h
    entry and T-0 close).
    """
    side = side.lower()
    if side not in ("over", "under"):
        raise ValueError("side must be 'over' or 'under'")
    entry = api.historical_strikeout_props(event_id, entry_date_iso).get("data", {})
    close = api.historical_strikeout_props(event_id, close_date_iso).get("data", {})
    e = pitcher_over_under(entry, pitcher)
    c = pitcher_over_under(close, pitcher)
    if not e or not c:
        return None
    _, e_over, e_under = e
    _, c_over, c_under = c
    e_pover, e_punder = devig(e_over, e_under, devig_method)
    c_pover, c_punder = devig(c_over, c_under, devig_method)
    if side == "over":
        e_price, c_price, e_prob, c_prob = e_over, c_over, e_pover, c_pover
    else:
        e_price, c_price, e_prob, c_prob = e_under, c_under, e_punder, c_punder
    return ClvResult(
        pitcher=pitcher,
        side=side,
        entry_price=e_price,
        close_price=c_price,
        clv_price=e_price / c_price - 1.0,
        entry_prob=e_prob,
        close_prob=c_prob,
        clv_prob=c_prob - e_prob,
    )
