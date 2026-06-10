//! T0.1 tests: `Cents` newtype. Written from spec conventions before implementation.
//!
//! Spec/CLAUDE.md contract: money is integer cents in an `i64` newtype; arithmetic
//! is checked (no panic, no silent wrap, no unwrap in money paths); `Decimal` only
//! at conversion boundaries; rounding direction is always explicit and chosen by
//! the caller (fee math rounds against us).

use fortuna_core::money::{Cents, MoneyError};
use proptest::prelude::*;
use rust_decimal::Decimal;
use std::str::FromStr;

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

// ---- construction and raw access ----

#[test]
fn new_and_raw_round_trip() {
    assert_eq!(Cents::new(1234).raw(), 1234);
    assert_eq!(Cents::new(-5).raw(), -5);
    assert_eq!(Cents::ZERO.raw(), 0);
}

// ---- checked arithmetic ----

#[test]
fn checked_add_basic() {
    assert_eq!(
        Cents::new(100).checked_add(Cents::new(23)),
        Ok(Cents::new(123))
    );
    assert_eq!(
        Cents::new(-100).checked_add(Cents::new(40)),
        Ok(Cents::new(-60))
    );
}

#[test]
fn checked_add_overflow_is_error_not_panic() {
    assert!(matches!(
        Cents::new(i64::MAX).checked_add(Cents::new(1)),
        Err(MoneyError::Overflow { .. })
    ));
    assert!(matches!(
        Cents::new(i64::MIN).checked_add(Cents::new(-1)),
        Err(MoneyError::Overflow { .. })
    ));
}

#[test]
fn checked_sub_basic_and_underflow() {
    assert_eq!(
        Cents::new(100).checked_sub(Cents::new(150)),
        Ok(Cents::new(-50))
    );
    assert!(matches!(
        Cents::new(i64::MIN).checked_sub(Cents::new(1)),
        Err(MoneyError::Overflow { .. })
    ));
}

#[test]
fn checked_mul_by_quantity() {
    // Order cost = price * contracts: the canonical money multiplication.
    assert_eq!(Cents::new(37).checked_mul(100), Ok(Cents::new(3700)));
    assert_eq!(Cents::new(37).checked_mul(0), Ok(Cents::ZERO));
    // Negative quantities arise from sign conventions in PnL math; must work.
    assert_eq!(Cents::new(37).checked_mul(-2), Ok(Cents::new(-74)));
}

#[test]
fn checked_mul_overflow_is_error() {
    assert!(matches!(
        Cents::new(i64::MAX / 2).checked_mul(3),
        Err(MoneyError::Overflow { .. })
    ));
}

#[test]
fn checked_neg_and_abs_handle_i64_min() {
    assert_eq!(Cents::new(5).checked_neg(), Ok(Cents::new(-5)));
    assert!(matches!(
        Cents::new(i64::MIN).checked_neg(),
        Err(MoneyError::Overflow { .. })
    ));
    assert_eq!(Cents::new(-5).checked_abs(), Ok(Cents::new(5)));
    assert!(matches!(
        Cents::new(i64::MIN).checked_abs(),
        Err(MoneyError::Overflow { .. })
    ));
}

#[test]
fn checked_sum_empty_is_zero() {
    assert_eq!(Cents::checked_sum(std::iter::empty()), Ok(Cents::ZERO));
}

#[test]
fn checked_sum_accumulates_and_detects_mid_sequence_overflow() {
    let vals = [Cents::new(1), Cents::new(2), Cents::new(3)];
    assert_eq!(Cents::checked_sum(vals.iter().copied()), Ok(Cents::new(6)));

    let overflowing = [Cents::new(i64::MAX), Cents::new(1), Cents::new(-10)];
    assert!(matches!(
        Cents::checked_sum(overflowing.iter().copied()),
        Err(MoneyError::Overflow { .. })
    ));
}

// ---- Decimal boundary conversions (dollars <-> cents) ----

#[test]
fn from_dollars_exact_whole_cents() {
    assert_eq!(
        Cents::from_dollars_exact(dec("12.34")),
        Ok(Cents::new(1234))
    );
    assert_eq!(Cents::from_dollars_exact(dec("-0.05")), Ok(Cents::new(-5)));
    assert_eq!(Cents::from_dollars_exact(dec("3")), Ok(Cents::new(300)));
    // Trailing zeros beyond cents are still exact.
    assert_eq!(
        Cents::from_dollars_exact(dec("1.230000")),
        Ok(Cents::new(123))
    );
}

#[test]
fn from_dollars_exact_rejects_sub_cent_remainder() {
    assert!(matches!(
        Cents::from_dollars_exact(dec("0.005")),
        Err(MoneyError::SubCentRemainder { .. })
    ));
    assert!(matches!(
        Cents::from_dollars_exact(dec("-0.005")),
        Err(MoneyError::SubCentRemainder { .. })
    ));
}

#[test]
fn from_dollars_floor_and_ceil_round_toward_named_direction() {
    // floor -> toward negative infinity; ceil -> toward positive infinity.
    // The caller picks the direction that is against us; these primitives
    // must be exact about which way they go, including for negatives.
    assert_eq!(Cents::from_dollars_floor(dec("0.005")), Ok(Cents::ZERO));
    assert_eq!(Cents::from_dollars_ceil(dec("0.005")), Ok(Cents::new(1)));
    assert_eq!(Cents::from_dollars_floor(dec("-0.005")), Ok(Cents::new(-1)));
    assert_eq!(Cents::from_dollars_ceil(dec("-0.005")), Ok(Cents::ZERO));
    // Exact values are unchanged by either.
    assert_eq!(Cents::from_dollars_floor(dec("2.50")), Ok(Cents::new(250)));
    assert_eq!(Cents::from_dollars_ceil(dec("2.50")), Ok(Cents::new(250)));
}

#[test]
fn from_dollars_out_of_i64_range_is_error() {
    for f in [
        Cents::from_dollars_exact,
        Cents::from_dollars_floor,
        Cents::from_dollars_ceil,
    ] {
        assert!(matches!(
            f(Decimal::MAX),
            Err(MoneyError::OutOfRange { .. })
        ));
        assert!(matches!(
            f(Decimal::MIN),
            Err(MoneyError::OutOfRange { .. })
        ));
    }
}

#[test]
fn from_dollars_in_decimal_range_but_beyond_i64_cents_is_error() {
    // 1e17 dollars = 1e19 cents: survives the Decimal multiply but exceeds
    // i64 (~9.2e18). The narrowing step must catch it, not wrap.
    let big = dec("100000000000000000.00");
    for f in [
        Cents::from_dollars_exact,
        Cents::from_dollars_floor,
        Cents::from_dollars_ceil,
    ] {
        assert!(matches!(f(big), Err(MoneyError::OutOfRange { .. })));
        assert!(matches!(f(-big), Err(MoneyError::OutOfRange { .. })));
    }
}

#[test]
fn to_dollars_round_trips_whole_cents() {
    assert_eq!(Cents::new(1234).to_dollars(), dec("12.34"));
    assert_eq!(Cents::new(-5).to_dollars(), dec("-0.05"));
    // Extremes must not panic.
    assert_eq!(
        Cents::from_dollars_exact(Cents::new(i64::MIN).to_dollars()),
        Ok(Cents::new(i64::MIN))
    );
    assert_eq!(
        Cents::from_dollars_exact(Cents::new(i64::MAX).to_dollars()),
        Ok(Cents::new(i64::MAX))
    );
}

// ---- display and serde ----

#[test]
fn display_is_unambiguous_dollars() {
    assert_eq!(Cents::new(1234).to_string(), "$12.34");
    assert_eq!(Cents::new(-5).to_string(), "-$0.05");
    assert_eq!(Cents::ZERO.to_string(), "$0.00");
    // i64::MIN must render without panicking (abs() would overflow).
    assert_eq!(Cents::new(i64::MIN).to_string(), "-$92233720368547758.08");
}

#[test]
fn serde_is_transparent_integer() {
    let json = serde_json::to_string(&Cents::new(1234)).unwrap();
    assert_eq!(json, "1234");
    let back: Cents = serde_json::from_str("-5").unwrap();
    assert_eq!(back, Cents::new(-5));
}

#[test]
fn serde_rejects_floats_and_strings() {
    // Money from a JSON float must never silently truncate.
    assert!(serde_json::from_str::<Cents>("12.5").is_err());
    assert!(serde_json::from_str::<Cents>("\"1234\"").is_err());
}

// ---- properties ----

proptest! {
    #[test]
    fn prop_checked_ops_never_panic(a in any::<i64>(), b in any::<i64>()) {
        // Result is Ok or Err; the point is no panic for any input pair.
        let _ = Cents::new(a).checked_add(Cents::new(b));
        let _ = Cents::new(a).checked_sub(Cents::new(b));
        let _ = Cents::new(a).checked_mul(b);
        let _ = Cents::new(a).checked_neg();
        let _ = Cents::new(a).checked_abs();
    }

    #[test]
    fn prop_to_dollars_from_dollars_round_trip(a in any::<i64>()) {
        prop_assert_eq!(Cents::from_dollars_exact(Cents::new(a).to_dollars()), Ok(Cents::new(a)));
    }

    #[test]
    fn prop_floor_le_ceil_and_within_one_cent(
        units in -1_000_000_000i64..1_000_000_000,
        scale in 0u32..15,
    ) {
        let d = Decimal::new(units, scale); // arbitrary decimal dollars
        let floor = Cents::from_dollars_floor(d).unwrap();
        let ceil = Cents::from_dollars_ceil(d).unwrap();
        prop_assert!(floor <= ceil);
        prop_assert!(ceil.raw() - floor.raw() <= 1);
        // Exact succeeds iff floor == ceil, and then agrees with both.
        match Cents::from_dollars_exact(d) {
            Ok(c) => { prop_assert_eq!(c, floor); prop_assert_eq!(c, ceil); }
            Err(_) => prop_assert!(floor < ceil),
        }
    }

    #[test]
    fn prop_add_matches_i128_reference(a in any::<i64>(), b in any::<i64>()) {
        let reference = a as i128 + b as i128;
        match Cents::new(a).checked_add(Cents::new(b)) {
            Ok(c) => prop_assert_eq!(c.raw() as i128, reference),
            Err(_) => prop_assert!(reference > i64::MAX as i128 || reference < i64::MIN as i128),
        }
    }
}
