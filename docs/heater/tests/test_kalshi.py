"""Pure (no-network) tests for the Kalshi read-only helpers: ticker parsing,
per-pitcher ladder grouping, and two-sided devig."""
import math

from heater.data.kalshi import (
    market_p_yes,
    pitcher_key,
    pitcher_options,
    strikeout_ladder,
)

# two pitchers share one game event; each has a 2-rung ladder
_MARKETS = [
    {"ticker": "KXMLBKS-26JUN-DETTSKUBAL29-8", "event_ticker": "KXMLBKS-26JUN",
     "floor_strike": 7.5, "yes_sub_title": "Tarik Skubal: 8+",
     "yes_bid_dollars": "0.32", "yes_ask_dollars": "0.35",
     "no_bid_dollars": "0.65", "no_ask_dollars": "0.68", "yes_bid_size_fp": 127.0},
    {"ticker": "KXMLBKS-26JUN-DETTSKUBAL29-6", "event_ticker": "KXMLBKS-26JUN",
     "floor_strike": 5.5, "yes_sub_title": "Tarik Skubal: 6+",
     "yes_bid_dollars": "0.65", "yes_ask_dollars": "0.70",
     "no_bid_dollars": "0.30", "no_ask_dollars": "0.35", "yes_bid_size_fp": 144.0},
    {"ticker": "KXMLBKS-26JUN-CWSEFEDDE47-5", "event_ticker": "KXMLBKS-26JUN",
     "floor_strike": 4.5, "yes_sub_title": "Erick Fedde: 5+",
     "yes_bid_dollars": "0.28", "yes_ask_dollars": "0.32",
     "no_bid_dollars": "0.68", "no_ask_dollars": "0.72", "yes_bid_size_fp": 50.0},
]


def test_pitcher_key_drops_threshold():
    assert pitcher_key("KXMLBKS-26JUN-DETTSKUBAL29-8") == "KXMLBKS-26JUN-DETTSKUBAL29"


def test_pitcher_options_separates_both_starters_in_one_event():
    opts = pitcher_options(_MARKETS)
    assert len(opts) == 2  # Skubal and Fedde, NOT collapsed into one event
    assert opts["KXMLBKS-26JUN-DETTSKUBAL29"] == "Tarik Skubal"


def test_ladder_is_one_pitcher_sorted_by_line():
    lad = strikeout_ladder(_MARKETS, "KXMLBKS-26JUN-DETTSKUBAL29")
    assert [r["line"] for r in lad] == [5.5, 7.5]  # Fedde's 4.5 rung excluded, sorted
    assert lad[0]["n"] == 6 and lad[1]["n"] == 8


def test_market_p_yes_two_sided_devig():
    # yes_mid=0.335, no_mid=0.665 -> fair = 0.335/(0.335+0.665) = 0.335
    p = market_p_yes(strikeout_ladder(_MARKETS, "KXMLBKS-26JUN-DETTSKUBAL29")[1])
    assert math.isclose(p, 0.335 / (0.335 + 0.665), abs_tol=1e-9)


def test_market_p_yes_one_sided_fallback():
    row = {"yes_bid": 0.40, "yes_ask": 0.44, "no_bid": None, "no_ask": None}
    assert math.isclose(market_p_yes(row), 0.42, abs_tol=1e-9)
