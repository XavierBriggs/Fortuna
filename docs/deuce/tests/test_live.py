from deuce.live import (
    benchmark_prob,
    best_of_from,
    best_of_from_sport_key,
    capture_three_way,
    parse_kalshi_atp,
    per_book_probs,
    surface_from,
    surface_from_sport_key,
    _parse_rules,
)


def _odds_event(name1, name2, books):
    return {
        "bookmakers": [
            {"key": bk, "markets": [{"key": "h2h", "outcomes": [
                {"name": name1, "price": p1}, {"name": name2, "price": p2}]}]}
            for bk, (p1, p2) in books.items()
        ]
    }


def _mkt(event, player, yes_bid, yes_ask, oi, rules):
    return {
        "event_ticker": event,
        "yes_sub_title": player,
        "yes_bid_dollars": f"{yes_bid:.4f}",
        "yes_ask_dollars": f"{yes_ask:.4f}",
        "open_interest_fp": f"{oi:.2f}",
        "rules_primary": rules,
    }


RULES_A = "If Alexander Zverev wins the Zverev vs Collignon professional tennis match in the 2026 ATP Halle Quarterfinal after a ball has been played"


def test_parse_rules_extracts_tournament_and_round():
    tourn, rnd = _parse_rules(RULES_A)
    assert tourn == "2026 ATP Halle"
    assert rnd == "Quarterfinal"


def test_surface_and_best_of_heuristics():
    assert surface_from("2026 ATP Halle") == "grass"
    assert surface_from("2026 Roland Garros") == "clay"
    assert surface_from("2026 ATP Dubai") == "hard"
    assert best_of_from("2026 Wimbledon") == 5
    assert best_of_from("2026 ATP Halle") == 3


def test_parse_kalshi_pairs_two_markets_into_one_match():
    markets = [
        _mkt("EV1", "Alexander Zverev", 0.84, 0.85, 63000, RULES_A),
        _mkt("EV1", "Raphael Collignon", 0.16, 0.17, 13000,
             RULES_A.replace("Alexander Zverev wins", "Raphael Collignon wins")),
    ]
    matches = parse_kalshi_atp(markets)
    assert len(matches) == 1
    m = matches[0]
    assert m.player_a == "Alexander Zverev"
    assert m.player_b == "Raphael Collignon"
    # mid_a=0.845, mid_b=0.165 -> normalized ~0.8366
    assert 0.82 < m.kalshi_prob_a < 0.86
    assert abs(m.spread_a - 0.01) < 1e-9
    assert m.tournament == "2026 ATP Halle"


def test_parse_kalshi_skips_incomplete_event():
    markets = [_mkt("EV1", "Solo Player", 0.5, 0.51, 100, RULES_A)]  # only one side
    assert parse_kalshi_atp(markets) == []


def test_per_book_probs_devigs_each_book():
    ev = _odds_event("Aa Bb", "Cc Dd", {"pinnacle": (1.5, 2.6), "onexbet": (1.55, 2.5)})
    bp = per_book_probs(ev)
    assert set(bp) == {"pinnacle", "onexbet"}
    assert abs(sum(bp["pinnacle"].values()) - 1.0) < 1e-9
    assert bp["pinnacle"]["Aa Bb"] > bp["pinnacle"]["Cc Dd"]


def test_benchmark_prob_averages_only_sharp_books():
    by_book = {
        "pinnacle": {"Aa Bb": 0.80, "Cc Dd": 0.20},
        "betfair_ex_eu": {"Aa Bb": 0.78, "Cc Dd": 0.22},
        "onexbet": {"Aa Bb": 0.60, "Cc Dd": 0.40},  # soft outlier, must be ignored
    }
    bench, n_sharp, disp, n_all = benchmark_prob(by_book, "Aa Bb", ("pinnacle", "betfair_ex_eu"))
    assert n_sharp == 2 and n_all == 3
    assert abs(bench - 0.79) < 1e-9          # mean of the two sharp books only
    assert abs(disp - 0.20) < 1e-9           # 0.80 - 0.60 across ALL books


def test_capture_computes_three_way_divergences():
    markets = [
        _mkt("EV1", "Alexander Zverev", 0.84, 0.85, 63000, RULES_A),
        _mkt("EV1", "Raphael Collignon", 0.16, 0.17, 13000,
             RULES_A.replace("Alexander Zverev wins", "Raphael Collignon wins")),
    ]
    km = parse_kalshi_atp(markets)
    sharp_idx = {
        frozenset({"zverev_a", "collignon_r"}): {
            "by_book": {
                "pinnacle": {"Alexander Zverev": 0.80, "Raphael Collignon": 0.20},
                "betfair_ex_eu": {"Alexander Zverev": 0.81, "Raphael Collignon": 0.19},
                "onexbet": {"Alexander Zverev": 0.70, "Raphael Collignon": 0.30},  # soft
            }
        }
    }
    rows = capture_three_way(
        km, sharp_idx, lambda a, b, s, bo: 0.82, asof="t0",
        sharp_books=("pinnacle", "betfair_ex_eu"),
    )
    assert len(rows) == 1
    r = rows[0]
    assert r["n_sharp"] == 2
    assert abs(r["sharp"] - 0.805) < 1e-9    # mean of sharp pair, ignores 0.70 soft
    assert r["deuce"] == 0.82
    assert r["k_minus_sharp"] > 0            # kalshi ~0.837 > 0.805
    assert r["book_disp"] is not None and r["book_disp"] > 0.10  # 0.81 - 0.70


def test_surface_and_best_of_from_sport_key():
    assert surface_from_sport_key("tennis_atp_queens_club_champ") == "grass"  # the bug case
    assert surface_from_sport_key("tennis_atp_halle_open") == "grass"
    assert surface_from_sport_key("tennis_atp_french_open") == "clay"
    assert surface_from_sport_key("tennis_atp_dubai") == "hard"
    assert surface_from_sport_key("tennis_atp_stuttgart_open") == "grass"
    assert surface_from_sport_key("tennis_wta_stuttgart_open") == "clay"
    assert best_of_from_sport_key("tennis_atp_wimbledon") == 5
    assert best_of_from_sport_key("tennis_atp_queens_club_champ") == 3


def test_capture_surface_comes_from_sport_key_not_kalshi_string():
    # Kalshi says "ATP London" (heuristic -> hard); sport key says Queen's (grass)
    rules = ("If A B wins the A B vs C D professional tennis match in the "
             "2026 ATP London Quarterfinal after a ball has been played")
    markets = [
        _mkt("EV1", "A B", 0.55, 0.56, 1000, rules),
        _mkt("EV1", "C D", 0.44, 0.45, 1000, rules.replace("A B wins", "C D wins")),
    ]
    km = parse_kalshi_atp(markets)
    captured = {}

    def pricer(p1, p2, surface, bo):
        captured["surface"] = surface
        return 0.5

    sharp_idx = {
        frozenset({"b_a", "d_c"}): {
            "by_book": {"pinnacle": {"A B": 0.55, "C D": 0.45}},
            "sport": "tennis_atp_queens_club_champ",
        }
    }
    rows = capture_three_way(km, sharp_idx, pricer, "t0", sharp_books=("pinnacle",))
    assert captured["surface"] == "grass"   # from the sport key, not "ATP London" -> hard
    assert rows[0]["surface"] == "grass"


def test_capture_handles_unmatched_sharp_and_unknown_deuce():
    markets = [
        _mkt("EV1", "Aa Bb", 0.5, 0.51, 100, RULES_A),
        _mkt("EV1", "Cc Dd", 0.49, 0.50, 100,
             RULES_A.replace("Alexander Zverev wins", "Cc Dd wins")),
    ]
    km = parse_kalshi_atp(markets)
    rows = capture_three_way(km, {}, lambda *_: None, asof="t0")  # no sharp, unknown deuce
    assert rows[0]["sharp"] is None
    assert rows[0]["deuce"] is None
    assert rows[0]["k_minus_sharp"] is None
