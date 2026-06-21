import math

from deuce.devig import devig, proportional, shin


def test_proportional_sums_to_one_and_orders_favourite():
    p1, p2 = proportional(1.5, 2.5)  # 1.5 is the favourite
    assert math.isclose(p1 + p2, 1.0, abs_tol=1e-12)
    assert p1 > p2


def test_shin_sums_to_one_and_orders_favourite():
    p1, p2 = shin(1.5, 2.5)
    assert math.isclose(p1 + p2, 1.0, abs_tol=1e-9)
    assert p1 > p2


def test_symmetric_book_is_fifty_fifty_both_methods():
    for method in (proportional, shin):
        p1, p2 = method(1.90, 1.90)  # vig book, symmetric
        assert math.isclose(p1, 0.5, abs_tol=1e-9)
        assert math.isclose(p2, 0.5, abs_tol=1e-9)


def test_devig_removes_margin():
    # raw implied for 1.90/1.90 is 0.526 each (sum 1.052); fair must be < raw
    p1, _ = devig(1.90, 1.90, "proportional")
    assert p1 < 1 / 1.90


def test_shin_corrects_longshot_bias_toward_favourite():
    # Shin should lift the favourite's fair prob ABOVE naive proportional
    # (longshots are overbet), while staying a valid distribution.
    for o1, o2 in [(1.5, 2.5), (1.2, 4.5), (1.05, 11.0)]:
        sp1, sp2 = shin(o1, o2)
        pp1, _ = proportional(o1, o2)
        assert math.isclose(sp1 + sp2, 1.0, abs_tol=1e-9)
        assert sp1 >= pp1  # favourite (o1) lifted, not lowered


def test_devig_dispatch_unknown_raises():
    try:
        devig(2.0, 2.0, "nope")
    except ValueError:
        return
    raise AssertionError("expected ValueError for unknown method")
