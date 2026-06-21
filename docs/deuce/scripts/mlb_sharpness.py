"""One-off probe: how sharp is Kalshi MLB vs the sharp sportsbook line?

Off-topic from the tennis build — reuses the same benchmark (per-book devig, mean
of {Betfair UK/EU, Pinnacle, Matchbook}) to compare Kalshi KXMLBGAME mids to the
sharp consensus on today's MLB slate. No model leg (no MLB DEUCE) — just venue vs venue.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import pandas as pd  # noqa: E402
import requests  # noqa: E402

from deuce.data.odds_api import OddsAPI  # noqa: E402
from deuce.live import SHARP_BOOKS, per_book_probs  # noqa: E402

KALSHI = "https://api.elections.kalshi.com/trade-api/v2"


def _f(x) -> float:
    try:
        return float(x)
    except (TypeError, ValueError):
        return 0.0


def kalshi_mlb_games() -> list[dict]:
    out, cursor = [], None
    for _ in range(20):
        params = {"series_ticker": "KXMLBGAME", "status": "open", "limit": 200}
        if cursor:
            params["cursor"] = cursor
        r = requests.get(f"{KALSHI}/markets", params=params, timeout=20)
        r.raise_for_status()
        d = r.json()
        out += d.get("markets", [])
        cursor = d.get("cursor")
        if not cursor or not d.get("markets"):
            break
    events: dict[str, list] = {}
    for m in out:
        events.setdefault(m["event_ticker"], []).append(m)
    games = []
    for mk in events.values():
        if len(mk) != 2:
            continue
        m1, m2 = mk
        a, b = m1.get("yes_sub_title", ""), m2.get("yes_sub_title", "")
        mid_a = (_f(m1.get("yes_bid_dollars")) + _f(m1.get("yes_ask_dollars"))) / 2
        mid_b = (_f(m2.get("yes_bid_dollars")) + _f(m2.get("yes_ask_dollars"))) / 2
        if mid_a + mid_b <= 0 or not a or not b:
            continue
        games.append({
            "a": a, "b": b,
            "kalshi_a": mid_a / (mid_a + mid_b),
            "spread_a": _f(m1.get("yes_ask_dollars")) - _f(m1.get("yes_bid_dollars")),
            "oi": _f(m1.get("open_interest_fp")) + _f(m2.get("open_interest_fp")),
        })
    return games


def _team_parse(s: str) -> tuple[str, str | None]:
    s = s.strip().lower()
    parts = s.split()
    if len(parts) >= 2 and len(parts[-1]) == 1:  # disambiguator letter (NY M/Y, Chi C/W)
        return " ".join(parts[:-1]), parts[-1]
    return s, None


def team_match(kalshi_team: str, odds_name: str) -> bool:
    city, dis = _team_parse(kalshi_team)
    o = odds_name.strip().lower()
    if not o.startswith(city):
        return False
    return dis is None or o[len(city):].strip().startswith(dis)


def main() -> None:
    api = OddsAPI()
    odds = api.get_odds("baseball_mlb", regions="eu,uk", markets="h2h")
    odds_idx = [(ev, per_book_probs(ev)) for ev in odds]
    games = kalshi_mlb_games()

    rows = []
    for g in games:
        match = None
        for ev, bp in odds_idx:
            home, away = ev.get("home_team", ""), ev.get("away_team", "")
            if (team_match(g["a"], home) and team_match(g["b"], away)) or (
                team_match(g["a"], away) and team_match(g["b"], home)
            ):
                match = bp
                break
        if not match:
            continue
        sharp_vals, all_vals = [], []
        for book, probs in match.items():
            pa = next((q for name, q in probs.items() if team_match(g["a"], name)), None)
            if pa is None:
                continue
            all_vals.append(pa)
            if book in SHARP_BOOKS:
                sharp_vals.append(pa)
        if not sharp_vals:
            continue
        sharp = sum(sharp_vals) / len(sharp_vals)
        rows.append({
            "game": f'{g["a"]} v {g["b"]}',
            "kalshi": round(g["kalshi_a"], 4),
            "sharp": round(sharp, 4),
            "k_minus_sharp": round(g["kalshi_a"] - sharp, 4),
            "n_sharp": len(sharp_vals),
            "disp": round(max(all_vals) - min(all_vals), 4) if len(all_vals) >= 2 else None,
            "k_spread": round(g["spread_a"], 3),
            "oi": round(g["oi"], 0),
        })

    rows.sort(key=lambda r: -abs(r["k_minus_sharp"]))
    df = pd.DataFrame(rows)
    print(f"\nKalshi MLB vs sharp line — {len(rows)} matched games (of {len(games)} open Kalshi games)\n")
    if not rows:
        print("(no matches — odds API only carries near-term games; thin far-out Kalshi games dropped)")
        return
    with pd.option_context("display.width", 200):
        print(df.to_string(index=False))
    liquid = df[df["k_spread"] <= 0.05]
    print(f"\nall matched:          mean|k-sharp| = {df['k_minus_sharp'].abs().mean():.4f}  (n={len(df)})")
    if len(liquid):
        print(f"liquid (spread<=5c):  mean|k-sharp| = {liquid['k_minus_sharp'].abs().mean():.4f}  (n={len(liquid)})")
    print(f"median book_disp = {df['disp'].dropna().median():.4f}")
    print(f"credits left: {api.requests_remaining}")


if __name__ == "__main__":
    main()
