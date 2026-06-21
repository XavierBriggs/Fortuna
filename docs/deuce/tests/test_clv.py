import numpy as np

from deuce.clv import ClvBet, clv_bets_from_snapshots, summarize_clv
from deuce.metrics import bootstrap_mean_ci


def _event(home, away, price_home, price_away):
    return {
        "home_team": home,
        "away_team": away,
        "bookmakers": [
            {"markets": [{"key": "h2h", "outcomes": [
                {"name": home, "price": price_home},
                {"name": away, "price": price_away},
            ]}]}
        ],
    }


def test_bet_flagged_and_clv_positive_when_line_moves_your_way():
    # entry 2.0/2.0 (market 50/50); model loves A (0.70); A shortens to 1.5 by close
    entry = [_event("Roger Federer", "Rafael Nadal", 2.0, 2.0)]
    close = [_event("Roger Federer", "Rafael Nadal", 1.5, 2.7)]
    bets = clv_bets_from_snapshots(
        entry, close, lambda p1, p2, s, bo: 0.70, surface="hard", threshold=0.05
    )
    assert len(bets) == 1
    b = bets[0]
    assert b.bet_player == "Roger Federer"   # the side model liked
    assert b.clv_price > 0                    # got 2.0, closed 1.5 -> beat the close
    assert b.clv_prob > 0                     # close prob for A > entry prob


def test_no_bet_below_threshold():
    entry = [_event("A B", "C D", 2.0, 2.0)]
    close = [_event("A B", "C D", 1.9, 2.1)]
    bets = clv_bets_from_snapshots(
        entry, close, lambda *_: 0.52, surface="hard", threshold=0.05  # edge 0.02 < 0.05
    )
    assert bets == []


def test_unknown_player_is_skipped():
    entry = [_event("A B", "C D", 2.0, 2.0)]
    close = [_event("A B", "C D", 1.5, 2.7)]
    bets = clv_bets_from_snapshots(entry, close, lambda *_: None, surface="hard")
    assert bets == []


def test_match_absent_from_close_is_skipped():
    entry = [_event("A B", "C D", 2.0, 2.0)]
    close = [_event("E F", "G H", 1.5, 2.7)]
    bets = clv_bets_from_snapshots(entry, close, lambda *_: 0.9, surface="hard")
    assert bets == []


def test_summarize_reports_mean_and_positive_fraction():
    bets = [
        ClvBet("A", "B", "A", 0.1, 2.0, 1.8, 2.0 / 1.8 - 1, 0.03),
        ClvBet("A", "B", "A", 0.1, 2.0, 2.2, 2.0 / 2.2 - 1, -0.02),
    ]
    s = summarize_clv(bets, n_boot=200)
    assert s["n"] == 2
    assert s["pct_positive"] == 0.5
    assert s["ci"][0] <= s["mean_clv_price"] <= s["ci"][1]


def test_bootstrap_mean_ci_deterministic_and_brackets():
    v = np.array([0.01, -0.02, 0.05, 0.0, 0.03])
    a = bootstrap_mean_ci(v, n_boot=500, seed=1)
    b = bootstrap_mean_ci(v, n_boot=500, seed=1)
    assert a == b
    mean, lo, hi = a
    assert lo <= mean <= hi
