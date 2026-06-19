import numpy as np

from heater.kdist import binom_pmf, bf_pmf, expected_k, k_pmf, p_over, variance_k


def test_binom_pmf_normalizes_and_has_right_mean():
    n, q = 25, 0.26
    pmf = binom_pmf(n, q)
    assert abs(pmf.sum() - 1.0) < 1e-9
    mean = (np.arange(n + 1) * pmf).sum()
    assert abs(mean - n * q) < 1e-9


def test_bf_pmf_sums_to_one_and_centers():
    bfs, w = bf_pmf(23.0, 4.0, 8, 34)
    assert abs(w.sum() - 1.0) < 1e-12
    assert abs((bfs * w).sum() - 23.0) < 0.5  # near-symmetric within the window


def test_point_bf_collapses_to_binomial():
    # sd<=0 => fixed batters faced => the compound is exactly Binomial(bf, q)
    bfs, w = bf_pmf(24, 0.0, 8, 34)
    compound = k_pmf(0.25, bfs, w)
    direct = np.zeros_like(compound)
    direct[: 24 + 1] = binom_pmf(24, 0.25)
    assert np.allclose(compound, direct, atol=1e-12)


def test_leash_spread_adds_overdispersion():
    # the whole thesis: letting BF vary widens the strikeout distribution
    q = 0.25
    bfs_pt, w_pt = bf_pmf(23.0, 0.0, 8, 34)
    bfs_sp, w_sp = bf_pmf(23.0, 4.0, 8, 34)
    var_point = variance_k(k_pmf(q, bfs_pt, w_pt))
    var_spread = variance_k(k_pmf(q, bfs_sp, w_sp))
    assert var_spread > var_point


def test_expected_k_tracks_q_times_bf():
    bfs, w = bf_pmf(23.0, 4.0, 8, 34)
    pmf = k_pmf(0.24, bfs, w)
    assert abs(expected_k(pmf) - 0.24 * 23.0) < 0.4


def test_p_over_is_monotone_decreasing_in_the_line():
    bfs, w = bf_pmf(23.0, 4.0, 8, 34)
    pmf = k_pmf(0.26, bfs, w)
    assert p_over(3.5, pmf) > p_over(5.5, pmf) > p_over(8.5, pmf)
    assert abs(p_over(-1.0, pmf) - 1.0) < 1e-9
