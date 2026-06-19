import numpy as np

from heater.metrics import (
    brier,
    ece,
    log_loss,
    paired_logloss_delta_ci,
    ranked_probability_score,
)


def test_perfect_binary_forecasts_score_zero():
    y = np.array([1, 0, 1, 0])
    p = np.array([1.0, 0.0, 1.0, 0.0])
    assert log_loss(y, p) < 1e-6
    assert brier(y, p) < 1e-12


def test_confident_and_wrong_is_punished():
    y = np.array([0.0])
    assert log_loss(y, np.array([0.99])) > log_loss(y, np.array([0.51]))


def test_rps_zero_for_a_point_mass_on_truth():
    pmf = np.zeros(10)
    pmf[6] = 1.0
    assert ranked_probability_score(6, pmf) < 1e-12


def test_rps_rewards_concentration_near_truth():
    sharp = np.zeros(10)
    sharp[5], sharp[4], sharp[6] = 0.6, 0.2, 0.2
    diffuse = np.full(10, 0.1)
    assert ranked_probability_score(5, sharp) < ranked_probability_score(5, diffuse)


def test_calibration_of_well_calibrated_stream_is_good():
    rng = np.random.default_rng(0)
    p = rng.uniform(0, 1, 50_000)
    y = (rng.uniform(0, 1, 50_000) < p).astype(float)
    assert ece(y, p, bins=10) < 0.02


def test_paired_delta_negative_when_model_is_sharper():
    rng = np.random.default_rng(4)
    truth = rng.uniform(0.05, 0.95, 5000)
    y = (rng.uniform(0, 1, 5000) < truth).astype(float)
    baseline = 0.5 + 0.5 * (truth - 0.5)  # blurred toward 0.5
    delta, lo, hi = paired_logloss_delta_ci(y, truth, baseline, n_boot=500, seed=5)
    assert delta < 0
