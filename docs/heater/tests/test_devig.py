import math

from heater.devig import devig, proportional, shin


def test_proportional_sums_to_one_and_removes_margin():
    p_over, p_under = proportional(1.90, 1.90)  # ~5.3% hold, symmetric
    assert math.isclose(p_over + p_under, 1.0, abs_tol=1e-12)
    assert math.isclose(p_over, 0.5, abs_tol=1e-9)


def test_shin_sums_to_one():
    p_over, p_under = shin(1.55, 2.45)
    assert math.isclose(p_over + p_under, 1.0, abs_tol=1e-9)
    assert p_over > p_under  # shorter price => higher fair prob


def test_shin_pushes_mass_to_favourite_vs_proportional():
    # Favourite-longshot bias: proportional over-shrinks the favourite; Shin de-biases
    # by raising the favourite's fair prob (and lowering the longshot's).
    po_prop, pu_prop = proportional(1.30, 3.60)
    po_shin, pu_shin = shin(1.30, 3.60)
    assert po_shin > po_prop   # favourite fair prob rises
    assert pu_shin < pu_prop   # longshot fair prob falls


def test_devig_dispatch_and_no_margin_passthrough():
    assert devig(2.0, 2.0, "proportional") == (0.5, 0.5)
    # a fair book (inverse odds already sum to 1) passes through
    p = shin(2.0, 2.0)
    assert math.isclose(p[0], 0.5, abs_tol=1e-9)
