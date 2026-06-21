import numpy as np

from deuce.calibrate import (
    IsotonicCalibrator,
    LogisticCalibrator,
    _pava,
    logit,
    prequential_calibrate,
)
from deuce.metrics import ece, log_loss
import pandas as pd


def _well_calibrated(n, seed):
    rng = np.random.default_rng(seed)
    q = rng.uniform(0.02, 0.98, n)        # true probabilities
    y = (rng.uniform(0, 1, n) < q).astype(float)
    return q, y


def test_identity_on_well_calibrated_data():
    q, y = _well_calibrated(40_000, 0)
    cal = LogisticCalibrator.fit(q, y, "platt")
    assert abs(cal.a - 1.0) < 0.1      # ~no scaling needed
    assert abs(cal.b) < 0.1


def test_shrinks_overconfident_and_improves_logloss():
    q, y = _well_calibrated(40_000, 1)
    overconf = 1.0 / (1.0 + np.exp(-1.6 * logit(q)))  # sharpen logits -> overconfident
    cal = LogisticCalibrator.fit(overconf, y, "platt")
    assert cal.a < 1.0                                 # learns to shrink toward 0.5
    p_cal = cal.transform(overconf)
    assert log_loss(y, p_cal) < log_loss(y, overconf)  # recalibration helps


def test_transform_is_valid_and_monotonic():
    cal = LogisticCalibrator(a=0.7, b=0.1)
    grid = np.linspace(0.01, 0.99, 50)
    out = cal.transform(grid)
    assert np.all((out > 0) & (out < 1))
    assert np.all(np.diff(out) > 0)                    # monotone increasing


def test_temperature_has_zero_intercept():
    q, y = _well_calibrated(20_000, 2)
    overconf = 1.0 / (1.0 + np.exp(-1.5 * logit(q)))
    cal = LogisticCalibrator.fit(overconf, y, "temperature")
    assert cal.b == 0.0
    assert cal.a < 1.0


def test_pava_pools_violators_and_preserves_monotone():
    # decreasing input gets pooled to its mean; already-increasing is untouched
    assert np.allclose(_pava(np.array([3.0, 1.0, 2.0])), 2.0)
    inc = np.array([0.1, 0.2, 0.9])
    assert np.allclose(_pava(inc), inc)


def test_isotonic_improves_overconfident():
    # overconfident but OFF the 0/1 boundary, where isotonic is log-loss-safe
    q, y = _well_calibrated(60_000, 7)
    overconf = 1.0 / (1.0 + np.exp(-1.5 * logit(q)))
    iso = IsotonicCalibrator.fit(overconf, y)
    p_iso = iso.transform(overconf)
    assert ece(y, p_iso) < ece(y, overconf)
    assert log_loss(y, p_iso) < log_loss(y, overconf)


def test_isotonic_transform_valid_and_monotone():
    q, y = _well_calibrated(20_000, 8)
    raw = np.clip(0.5 + 1.4 * (q - 0.5), 0.001, 0.999)
    iso = IsotonicCalibrator.fit(raw, y)
    grid = np.linspace(0.02, 0.98, 100)
    out = iso.transform(grid)
    assert np.all((out > 0) & (out < 1))
    assert np.all(np.diff(out) >= -1e-9)  # non-decreasing


def test_per_regime_calibrators_differ():
    # two regimes with opposite miscalibration must get different maps
    rng = np.random.default_rng(11)
    n = 6000
    q = rng.uniform(0.05, 0.95, 2 * n)
    y = (rng.uniform(0, 1, 2 * n) < q).astype(float)
    over = 1.0 / (1.0 + np.exp(-1.6 * logit(q[:n])))   # regime 3: overconfident
    under = 1.0 / (1.0 + np.exp(-0.6 * logit(q[n:])))  # regime 5: underconfident
    df = pd.DataFrame(
        {
            "date": pd.to_datetime(["2010-01-01"] * (2 * n)),
            "year": [2010] * n + [2010] * n,
            "best_of": [3] * n + [5] * n,
            "p_model": np.concatenate([over, under]),
            "y": y,
        }
    )
    # add a second year so the first year's regimes train the calibrators
    df2 = df.copy()
    df2["year"] = 2011
    df2["date"] = pd.to_datetime("2011-01-01")
    out = prequential_calibrate(pd.concat([df, df2], ignore_index=True),
                                method="isotonic", min_fit=500, regime_col="best_of")
    y2 = out[out["year"] == 2011]
    bo3 = y2[y2["best_of"] == 3]
    bo5 = y2[y2["best_of"] == 5]
    # overconfident regime should be shrunk; underconfident regime sharpened
    assert not np.allclose(bo3["p_cal"], bo3["p_model"])
    assert not np.allclose(bo5["p_cal"], bo5["p_model"])


def test_prequential_is_passthrough_below_min_fit():
    # two years, tiny -> below min_fit -> p_cal == p_model untouched
    df = pd.DataFrame(
        {
            "date": pd.to_datetime(["2010-01-01", "2011-01-01"]),
            "year": [2010, 2011],
            "p_model": [0.7, 0.4],
            "y": [1, 0],
        }
    )
    out = prequential_calibrate(df, "platt", min_fit=1000)
    assert np.allclose(out["p_cal"], out["p_model"])
