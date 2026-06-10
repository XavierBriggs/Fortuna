//! T0.7 tests: conservative marking policy (spec 5.14). Written BEFORE
//! implementation.
//!
//! Policy under test: long YES marks at best yes bid x qty; long NO marks at
//! (100 - best yes ask) x |qty|. Stale book (strictly older than
//! max_book_age_ms) or wide spread (ask - bid strictly greater than
//! max_spread_cents, only when both touches exist) still uses the touch but
//! sets wide_flag. No needed touch (or no book at all) marks at Cents::ZERO
//! with wide_flag = true: zero is the conservative bound when no reliable
//! exit value exists. Boundaries: age == max age is NOT stale; spread == max
//! spread is NOT wide.

use fortuna_core::book::{OrderBook, PriceLevel};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Contracts, MarketId};
use fortuna_core::money::Cents;
use fortuna_state::{mark_lots, Mark, MarkPolicy};

fn ts(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(s).unwrap()
}

fn t0() -> UtcTimestamp {
    ts("2026-06-09T12:00:00.000Z")
}

fn book(as_of: UtcTimestamp, bids: &[(i64, i64)], asks: &[(i64, i64)]) -> OrderBook {
    let lvl = |&(p, q): &(i64, i64)| PriceLevel {
        price: Cents::new(p),
        qty: Contracts::new(q),
    };
    OrderBook {
        market: MarketId::new("M").unwrap(),
        as_of,
        yes_bids: bids.iter().map(lvl).collect(),
        yes_asks: asks.iter().map(lvl).collect(),
    }
}

fn policy() -> MarkPolicy {
    MarkPolicy {
        max_book_age_ms: 1_000,
        max_spread_cents: 15,
    }
}

fn assert_mark(m: Mark, value: i64, wide: bool) {
    assert_eq!(m.value, Cents::new(value), "mark value");
    assert_eq!(m.wide_flag, wide, "wide_flag");
}

#[test]
fn fresh_tight_book_long_marks_at_bid_times_qty() {
    let b = book(t0(), &[(45, 50)], &[(55, 50)]);
    let m = mark_lots(10, 0, Some(&b), t0(), &policy()).unwrap();
    assert_mark(m, 450, false);
}

#[test]
fn fresh_tight_book_short_marks_at_100_minus_ask_times_qty() {
    let b = book(t0(), &[(45, 50)], &[(55, 50)]);
    let m = mark_lots(0, 10, Some(&b), t0(), &policy()).unwrap();
    assert_mark(m, 450, false);
}

#[test]
fn stale_book_still_uses_touch_but_flags_wide() {
    let b = book(t0(), &[(45, 50)], &[(55, 50)]);
    let now = t0().checked_add_millis(1_001).unwrap();
    let m = mark_lots(10, 0, Some(&b), now, &policy()).unwrap();
    assert_mark(m, 450, true);
}

#[test]
fn age_exactly_at_max_is_not_stale() {
    // Stale is STRICTLY older than max_book_age_ms.
    let b = book(t0(), &[(45, 50)], &[(55, 50)]);
    let now = t0().checked_add_millis(1_000).unwrap();
    let m = mark_lots(10, 0, Some(&b), now, &policy()).unwrap();
    assert_mark(m, 450, false);
}

#[test]
fn wide_spread_still_uses_touch_but_flags_wide() {
    let b = book(t0(), &[(40, 50)], &[(60, 50)]); // spread 20 > 15
    let m = mark_lots(10, 0, Some(&b), t0(), &policy()).unwrap();
    assert_mark(m, 400, true);
    let m = mark_lots(0, 10, Some(&b), t0(), &policy()).unwrap();
    assert_mark(m, 400, true);
}

#[test]
fn spread_exactly_at_max_is_not_wide() {
    let b = book(t0(), &[(45, 50)], &[(60, 50)]); // spread 15 == 15
    let m = mark_lots(10, 0, Some(&b), t0(), &policy()).unwrap();
    assert_mark(m, 450, false);
}

#[test]
fn long_with_no_bids_marks_zero_and_flags() {
    // No reliable exit value on the needed side: conservative bound is zero.
    let b = book(t0(), &[], &[(55, 50)]);
    let m = mark_lots(10, 0, Some(&b), t0(), &policy()).unwrap();
    assert_mark(m, 0, true);
}

#[test]
fn short_with_no_asks_marks_zero_and_flags() {
    let b = book(t0(), &[(45, 50)], &[]);
    let m = mark_lots(0, 10, Some(&b), t0(), &policy()).unwrap();
    assert_mark(m, 0, true);
}

#[test]
fn missing_book_marks_zero_and_flags() {
    let m = mark_lots(10, 0, None, t0(), &policy()).unwrap();
    assert_mark(m, 0, true);
    let m = mark_lots(0, 10, None, t0(), &policy()).unwrap();
    assert_mark(m, 0, true);
}

#[test]
fn zero_position_marks_zero_without_flag() {
    // Nothing held: nothing to mark, no degraded-book noise.
    let m = mark_lots(0, 0, None, t0(), &policy()).unwrap();
    assert_mark(m, 0, false);
    let b = book(t0(), &[(45, 50)], &[(55, 50)]);
    let m = mark_lots(0, 0, Some(&b), t0(), &policy()).unwrap();
    assert_mark(m, 0, false);
}

#[test]
fn one_sided_fresh_book_with_needed_touch_is_not_wide() {
    // The spread test applies only when BOTH touches exist (documented).
    let b = book(t0(), &[(45, 50)], &[]);
    let m = mark_lots(10, 0, Some(&b), t0(), &policy()).unwrap();
    assert_mark(m, 450, false);
}

#[test]
fn book_from_the_future_is_not_stale() {
    // Negative age (clock skew / replay artifacts) never reads as stale.
    let b = book(
        t0().checked_add_millis(500).unwrap(),
        &[(45, 50)],
        &[(55, 50)],
    );
    let m = mark_lots(10, 0, Some(&b), t0(), &policy()).unwrap();
    assert_mark(m, 450, false);
}

#[test]
fn degenerate_no_side_value_clamps_to_zero_and_flags() {
    // Malformed ask above payout would imply a negative NO value; the mark
    // clamps at the conservative bound (zero) and flags wide.
    let b = book(t0(), &[], &[(101, 50)]);
    let m = mark_lots(0, 10, Some(&b), t0(), &policy()).unwrap();
    assert_mark(m, 0, true);
}

#[test]
fn stale_and_missing_touch_marks_zero_and_flags() {
    // Degradations compose: stale book with no needed touch is still zero+flag.
    let b = book(t0(), &[], &[(55, 50)]);
    let now = t0().checked_add_millis(5_000).unwrap();
    let m = mark_lots(10, 0, Some(&b), now, &policy()).unwrap();
    assert_mark(m, 0, true);
}

#[test]
fn pair_marks_at_roughly_pair_value() {
    // 10 YES + 10 NO against a 48/52 book: yes at 480 + no at 480 = 960
    // (the 100c pair value minus the spread), no wide flag.
    let b = book(t0(), &[(48, 50)], &[(52, 50)]);
    let m = mark_lots(10, 10, Some(&b), t0(), &policy()).unwrap();
    assert_eq!(
        m,
        Mark {
            value: Cents::new(960),
            wide_flag: false
        }
    );
}

#[test]
fn negative_lot_quantities_are_errors() {
    let b = book(t0(), &[(48, 50)], &[(52, 50)]);
    assert!(mark_lots(-1, 0, Some(&b), t0(), &policy()).is_err());
    assert!(mark_lots(0, -1, Some(&b), t0(), &policy()).is_err());
}
