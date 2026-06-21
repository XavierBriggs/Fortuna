"""Three-way live capture: Kalshi mid vs sharp sportsbook line vs DEUCE.

The reframed edge thesis (see docs/research/2026-06-18-kalshi-tennis.md): the sharp
sportsbook consensus (devigged Pinnacle/eu books) is the best-available truth;
Kalshi is a US-retail venue where tennis is minor, so it may be SOFT. We log all
three per match — Kalshi, sharp, DEUCE — and watch where Kalshi diverges from the
sharp line (with DEUCE as an independent tiebreak). Accumulate over time to see if
the divergence is systematic and which side the close moves toward.

Kalshi market reads are PUBLIC (no auth). Order placement (signed creds) is
FORTUNA's job, not this research logger.
"""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import pandas as pd
import requests

from .config import data_dir
from .data.odds_api import OddsAPI, OddsAPIError
from .devig import devig
from .names import canonical_player

_KALSHI = "https://api.elections.kalshi.com/trade-api/v2"

# the only books that carry information in tennis; the rest lag them. The
# benchmark is the unweighted mean of these books' OWN-devigged probabilities
# (each book's overround removed from its own two prices). Exchanges (Betfair,
# Matchbook) have ~no vig, so devig is ~a no-op on them.
SHARP_BOOKS = ("betfair_ex_uk", "betfair_ex_eu", "pinnacle", "matchbook")

_GRASS = ("halle", "queen", "wimbledon", "mallorca", "eastbourne", "hertogenbosch",
          "stuttgart", "newport", "nottingham")
_CLAY = ("roland garros", "french open", "madrid", "rome", "italian", "monte",
         "barcelona", "hamburg", "bastad", "gstaad", "kitzbuhel", "umag", "estoril",
         "munich", "geneva", "lyon", "cordoba", "buenos aires", "rio", "santiago",
         "houston", "marrakech")
_SLAM = ("australian open", "roland garros", "french open", "wimbledon", "us open")


def _f(v) -> float:
    try:
        return float(v)
    except (TypeError, ValueError):
        return 0.0


def surface_from(tournament: str) -> str:
    t = tournament.lower()
    if any(g in t for g in _GRASS):
        return "grass"
    if any(c in t for c in _CLAY):
        return "clay"
    return "hard"


def best_of_from(tournament: str) -> int:
    t = tournament.lower()
    return 5 if any(s in t for s in _SLAM) else 3


# Surface/format from the matched Odds API sport key (e.g. tennis_atp_queens_club_champ),
# which carries the real tournament name — more reliable than the Kalshi rules string.
_GRASS_KEYS = ("halle", "queens", "wimbledon", "mallorca", "eastbourne", "hertogenbosch",
               "newport", "nottingham", "birmingham", "berlin", "bad_homburg")
_CLAY_KEYS = ("french_open", "roland", "madrid", "rome", "italian", "monte_carlo",
              "barcelona", "hamburg", "bastad", "gstaad", "kitzbuhel", "umag", "estoril",
              "munich", "geneva", "lyon", "cordoba", "buenos_aires", "santiago", "houston",
              "marrakech", "charleston")
_SLAM_KEYS = ("aus_open", "australian_open", "french_open", "wimbledon", "us_open")


def surface_from_sport_key(sport_key: str) -> str:
    k = sport_key.lower()
    if "stuttgart" in k:  # ATP Boss Open is grass; WTA Porsche is clay
        return "grass" if "_atp_" in k else "clay"
    if any(g in k for g in _GRASS_KEYS):
        return "grass"
    if any(c in k for c in _CLAY_KEYS):
        return "clay"
    return "hard"


def best_of_from_sport_key(sport_key: str) -> int:
    return 5 if any(s in sport_key.lower() for s in _SLAM_KEYS) else 3


def _parse_rules(rules: str) -> tuple[str, str]:
    """'...match in the 2026 ATP Halle Quarterfinal after...' -> ('2026 ATP Halle', 'Quarterfinal')."""
    marker = "match in the "
    i = rules.find(marker)
    if i < 0:
        return "", ""
    tail = rules[i + len(marker):]
    tail = tail.split(" after")[0].strip()
    rounds = ("Final", "Semifinal", "Quarterfinal", "Round", "Qualifying")
    for rnd in rounds:
        j = tail.find(rnd)
        if j >= 0:
            return tail[:j].strip(), tail[j:].strip()
    return tail, ""


@dataclass(frozen=True)
class KalshiMatch:
    event_ticker: str
    player_a: str
    player_b: str
    kalshi_prob_a: float  # overround-normalized mid for player_a
    spread_a: float       # ask - bid for player_a (liquidity proxy)
    open_interest: float
    tournament: str
    round: str


def parse_kalshi_atp(markets: list[dict]) -> list[KalshiMatch]:
    """Pair the two per-player markets in each event into one match."""
    events: dict[str, list[dict]] = {}
    for m in markets:
        events.setdefault(m.get("event_ticker", ""), []).append(m)
    out: list[KalshiMatch] = []
    for ev, mk in events.items():
        if len(mk) != 2:
            continue
        m1, m2 = mk
        a, b = m1.get("yes_sub_title", ""), m2.get("yes_sub_title", "")
        mid_a = (_f(m1.get("yes_bid_dollars")) + _f(m1.get("yes_ask_dollars"))) / 2
        mid_b = (_f(m2.get("yes_bid_dollars")) + _f(m2.get("yes_ask_dollars"))) / 2
        if mid_a + mid_b <= 0 or not a or not b:
            continue
        tourn, rnd = _parse_rules(m1.get("rules_primary", ""))
        out.append(
            KalshiMatch(
                event_ticker=ev,
                player_a=a,
                player_b=b,
                kalshi_prob_a=mid_a / (mid_a + mid_b),  # devig the exchange overround
                spread_a=_f(m1.get("yes_ask_dollars")) - _f(m1.get("yes_bid_dollars")),
                open_interest=_f(m1.get("open_interest_fp")),
                tournament=tourn,
                round=rnd,
            )
        )
    return out


def fetch_kalshi_atp(status: str = "open", max_pages: int = 10) -> list[KalshiMatch]:
    out: list[dict] = []
    cursor = None
    for _ in range(max_pages):
        params = {"series_ticker": "KXATPMATCH", "status": status, "limit": 200}
        if cursor:
            params["cursor"] = cursor
        r = requests.get(f"{_KALSHI}/markets", params=params, timeout=20)
        r.raise_for_status()
        d = r.json()
        out.extend(d.get("markets", []))
        cursor = d.get("cursor")
        if not cursor or not d.get("markets"):
            break
    return parse_kalshi_atp(out)


def per_book_probs(event: dict, devig_method: str = "shin") -> dict[str, dict[str, float]]:
    """{book_key: {player_name: own-devigged prob}} for the h2h market.

    Each book's overround is removed from ITS OWN two prices — never max-across
    books (which would track the most generous soft outlier, not the sharps)."""
    out: dict[str, dict[str, float]] = {}
    for bk in event.get("bookmakers", []):
        prices: dict[str, float] = {}
        for market in bk.get("markets", []):
            if market.get("key") != "h2h":
                continue
            for o in market.get("outcomes", []):
                name, price = o.get("name"), o.get("price")
                if name and price:
                    prices[name] = float(price)
        if len(prices) == 2:
            (n1, p1), (n2, p2) = list(prices.items())
            q1, q2 = devig(p1, p2, devig_method)
            out[bk.get("key", "")] = {n1: q1, n2: q2}
    return out


def benchmark_prob(
    by_book: dict[str, dict[str, float]], player_a: str, sharp_books
) -> tuple[float | None, int, float | None, int]:
    """(sharp_mean_prob_a, n_sharp, dispersion_all, n_all).

    sharp_mean = unweighted mean of player_a's devigged prob across the sharp
    books present; dispersion = max-min of player_a's prob across ALL books (a
    disagreement / soft-spot signal, not an input to the benchmark)."""
    ca = canonical_player(player_a)
    sharp_vals, all_vals = [], []
    for book, probs in by_book.items():
        pa = next((q for name, q in probs.items() if canonical_player(name) == ca), None)
        if pa is None:
            continue
        all_vals.append(pa)
        if book in sharp_books:
            sharp_vals.append(pa)
    bench = sum(sharp_vals) / len(sharp_vals) if sharp_vals else None
    disp = (max(all_vals) - min(all_vals)) if len(all_vals) >= 2 else None
    return bench, len(sharp_vals), disp, len(all_vals)


def build_sharp_index(api: OddsAPI, regions: str = "eu,uk", devig_method: str = "shin") -> dict:
    """{canonical-pair -> {'by_book': {book: {name: prob}}, 'sport': key}}.

    `regions='eu,uk'` covers both Betfair exchanges + Pinnacle + Matchbook; cost
    is (#regions) credits per active sport (live odds)."""
    idx: dict[frozenset, dict] = {}
    for s in api.tennis_sports():
        if not s.get("active"):
            continue
        try:
            events = api.get_odds(s["key"], regions=regions, markets="h2h")
        except OddsAPIError:
            continue
        for ev in events:
            bp = per_book_probs(ev, devig_method)
            if not bp:
                continue
            names: set[str] = set()
            for d in bp.values():
                names |= set(d.keys())
            if len(names) != 2:
                continue
            key = frozenset(canonical_player(n) for n in names)
            idx[key] = {"by_book": bp, "sport": s["key"]}
    return idx


def capture_three_way(
    kalshi: list[KalshiMatch],
    sharp_idx: dict,
    pricer,
    asof: str,
    sharp_books=SHARP_BOOKS,
) -> list[dict]:
    """One row per Kalshi match: kalshi / sharp-benchmark / deuce prob for player_a
    and the divergences, plus n_sharp and cross-book dispersion.
    `pricer(p1,p2,surface,best_of)->prob|None` (RatingBook.predict)."""
    rows = []
    for km in kalshi:
        s = sharp_idx.get(frozenset({canonical_player(km.player_a), canonical_player(km.player_b)}))
        # surface/format from the matched sport key when available (reliable),
        # else fall back to the Kalshi rules string
        if s and s.get("sport"):
            surface = surface_from_sport_key(s["sport"])
            bo = best_of_from_sport_key(s["sport"])
        else:
            surface = surface_from(km.tournament)
            bo = best_of_from(km.tournament)
        deuce = pricer(km.player_a, km.player_b, surface, bo)
        sharp = n_sharp = disp = None
        if s:
            sharp, n_sharp, disp, _ = benchmark_prob(s["by_book"], km.player_a, sharp_books)
        rows.append(
            {
                "asof": asof,
                "match": f"{km.player_a} v {km.player_b}",
                "tournament": km.tournament,
                "surface": surface,
                "kalshi": round(km.kalshi_prob_a, 4),
                "sharp": round(sharp, 4) if sharp is not None else None,
                "deuce": round(deuce, 4) if deuce is not None else None,
                "k_minus_sharp": round(km.kalshi_prob_a - sharp, 4) if sharp is not None else None,
                "deuce_minus_sharp": round(deuce - sharp, 4)
                if (deuce is not None and sharp is not None) else None,
                "n_sharp": n_sharp,
                "book_disp": round(disp, 4) if disp is not None else None,
                "k_spread": round(km.spread_a, 3),
                "open_interest": round(km.open_interest, 0),
            }
        )
    return rows


def format_capture(rows: list[dict]) -> str:
    if not rows:
        return "(no matches captured — no live Kalshi ATP markets, or none matched)"
    df = pd.DataFrame(rows)
    # surface the biggest Kalshi-vs-sharp gaps first — those are the candidate edges
    df["_absdiv"] = df["k_minus_sharp"].abs()
    df = df.sort_values("_absdiv", ascending=False, na_position="last").drop(columns=["_absdiv", "asof"])
    with pd.option_context("display.max_rows", None, "display.width", 220):
        return df.to_string(index=False)


def append_capture(rows: list[dict]) -> Path:
    """Append rows to a CSV time series (one file accumulates across runs)."""
    out = data_dir() / "live"
    out.mkdir(parents=True, exist_ok=True)
    path = out / "atp_three_way.csv"
    df = pd.DataFrame(rows)
    df.to_csv(path, mode="a", header=not path.exists(), index=False)
    return path
