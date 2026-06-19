"""Integration: on synthetic truth that CONTAINS opponent + leash structure, the
matchup+leash model must beat the naive trailing-K baseline. If this fails, the
model wiring or the harness is wrong (the demo is rigged for the model to win)."""
import numpy as np

from heater.backtest import ablate, run_backtest
from heater.config import BacktestConfig, KModelConfig
from heater.model import baseline_pmf, heater_pmf
from heater.synth import make_starts


def test_heater_beats_baseline_on_synthetic_truth():
    df = make_starts(n=4000, seed=7)
    res, report = run_backtest(BacktestConfig(), df)
    all_row = report[report["segment"] == "ALL"].iloc[0]
    assert all_row["ll_delta"] < 0          # sharper over/under than baseline
    assert all_row["heater_rps"] < all_row["base_rps"]  # sharper full distribution


def test_report_is_per_segment_and_nonempty():
    df = make_starts(n=2000, seed=11)
    _, report = run_backtest(BacktestConfig(), df)
    assert len(report) >= 2
    assert {"segment", "n", "beats_base", "heater_ece"}.issubset(report.columns)


def test_all_toggles_off_reduces_to_baseline():
    # the ablation's "baseline" variant must equal baseline_pmf exactly
    off = KModelConfig(use_opponent=False, use_leash_spread=False, use_park=False)
    h = heater_pmf(0.26, 0.21, 23.0, 4.0, 1.07, off)
    b = baseline_pmf(0.26, 23.0, off)
    assert np.allclose(h, b, atol=1e-12)


def test_ablation_runs_and_baseline_row_is_first():
    df = make_starts(n=1500, seed=3)
    table = ablate(BacktestConfig(), df)
    assert table.iloc[0]["variant"] == "baseline"
    assert table.iloc[0]["rps_vs_base"] == 0.0  # baseline vs itself
    # the full opp+leash variant should not be worse than baseline on RPS here
    full = table[table["variant"] == "opp+leash"].iloc[0]
    assert full["rps_vs_base"] <= 0.0
