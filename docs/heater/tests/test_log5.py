import math

from heater.log5 import apply_park, matchup_k


def test_league_average_opponent_returns_pitcher_rate():
    # an opponent at league average tells you nothing beyond the pitcher's own rate
    assert math.isclose(matchup_k(0.27, 0.225, 0.225), 0.27, abs_tol=1e-9)


def test_symmetric_in_pitcher_and_opponent():
    a = matchup_k(0.30, 0.20, 0.225)
    b = matchup_k(0.20, 0.30, 0.225)
    assert math.isclose(a, b, abs_tol=1e-12)


def test_monotone_and_bounded():
    base = matchup_k(0.25, 0.225, 0.225)
    hotter = matchup_k(0.25, 0.28, 0.225)   # tougher (whiff-prone) opponent
    colder = matchup_k(0.25, 0.17, 0.225)   # contact opponent
    assert colder < base < hotter
    assert 0.0 < colder and hotter < 1.0


def test_high_k_pitcher_vs_whiffy_lineup_exceeds_both():
    q = matchup_k(0.32, 0.27, 0.225)
    assert q > 0.32 and q > 0.27


def test_park_multiplier_moves_the_right_way():
    q = 0.25
    assert apply_park(q, 1.08) > q > apply_park(q, 0.92)
    assert math.isclose(apply_park(q, 1.0), q, abs_tol=1e-9)
