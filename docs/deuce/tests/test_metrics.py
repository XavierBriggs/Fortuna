import numpy as np

from deuce.metrics import (
    bootstrap_ci,
    brier,
    ece,
    log_loss,
    paired_logloss_delta_ci,
)


def test_perfect_forecasts_score_zero():
    y = np.array([1, 0, 1, 0])
    p = np.array([1.0, 0.0, 1.0, 0.0])
    assert log_loss(y, p) < 1e-6
    assert brier(y, p) < 1e-12


def test_confident_and_wrong_is_punished():
    y = np.array([0.0])
    assert log_loss(y, np.array([0.99])) > log_loss(y, np.array([0.51]))


def test_calibration_of_well_calibrated_stream_is_good():
    rng = np.random.default_rng(0)
    p = rng.uniform(0, 1, 50_000)
    y = (rng.uniform(0, 1, 50_000) < p).astype(float)  # outcomes match probs
    assert ece(y, p, bins=10) < 0.02


def test_bootstrap_is_deterministic_and_brackets_point():
    rng = np.random.default_rng(1)
    p = rng.uniform(0, 1, 1000)
    y = (rng.uniform(0, 1, 1000) < p).astype(float)
    a = bootstrap_ci(log_loss, y, p, n_boot=500, seed=42)
    b = bootstrap_ci(log_loss, y, p, n_boot=500, seed=42)
    assert a == b  # same seed -> identical
    point, lo, hi = a
    assert lo <= point <= hi


def test_paired_delta_zero_when_model_equals_market():
    rng = np.random.default_rng(2)
    p = rng.uniform(0.05, 0.95, 2000)
    y = (rng.uniform(0, 1, 2000) < p).astype(float)
    delta, lo, hi = paired_logloss_delta_ci(y, p, p, n_boot=500, seed=3)
    assert abs(delta) < 1e-9
    assert lo <= 0 <= hi


def test_paired_delta_negative_when_model_is_sharper():
    # model = truth, market = blurred toward 0.5 -> model must score better (delta<0)
    rng = np.random.default_rng(4)
    truth = rng.uniform(0.05, 0.95, 5000)
    y = (rng.uniform(0, 1, 5000) < truth).astype(float)
    market = 0.5 + 0.5 * (truth - 0.5)  # less confident
    delta, lo, hi = paired_logloss_delta_ci(y, truth, market, n_boot=500, seed=5)
    assert delta < 0
