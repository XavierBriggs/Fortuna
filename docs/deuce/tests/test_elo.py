import math

from deuce.config import EloConfig
from deuce.elo import EloEngine, expected_score, margin_multiplier


def test_equal_ratings_are_coinflip():
    assert math.isclose(expected_score(1500, 1500), 0.5)


def test_expected_score_is_symmetric():
    a, b = 1700.0, 1500.0
    assert math.isclose(expected_score(a, b) + expected_score(b, a), 1.0, abs_tol=1e-12)
    assert expected_score(a, b) > 0.5


def test_winning_raises_your_probability():
    eng = EloEngine(EloConfig())
    before = eng.predict("A", "B", "hard")
    assert math.isclose(before, 0.5)
    eng.update("A", "B", "hard", winner_games=12, loser_games=4)
    after = eng.predict("A", "B", "hard")
    assert after > before
    # winner's surface+overall both rose; loser's fell
    assert eng.blended("A", "hard") > eng.blended("B", "hard")


def test_margin_multiplier_rewards_dominance():
    cfg = EloConfig()
    bagel = margin_multiplier(12, 0, cfg)
    tight = margin_multiplier(13, 11, cfg)
    assert bagel > tight >= 1.0


def test_margin_multiplier_off_and_degenerate():
    off = EloConfig(margin_weighting=False)
    assert margin_multiplier(12, 0, off) == 1.0
    assert margin_multiplier(0, 0, EloConfig()) == 1.0


def test_prediction_only_sees_the_past():
    # an unseen player is a coinflip vs another unseen player
    eng = EloEngine(EloConfig())
    assert math.isclose(eng.predict("X", "Y", "clay"), 0.5)
