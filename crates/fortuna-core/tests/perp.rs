//! T5.B2 tests: perps domain core types. Written from spec 5.15 and
//! docs/design/kinetics-perps-module-plan.md §2 BEFORE implementation.
//!
//! Contract under test:
//! - `PerpPrice` is a venue-scoped integer newtype in TEN-THOUSANDTHS of a
//!   dollar (the venue tick is $0.0001), checked arithmetic, `Decimal` only
//!   at venue payload boundaries. Type-level separation from `Cents`.
//! - Conversion to `Cents` happens exclusively at notional/PnL boundaries
//!   with the rounding direction explicit and chosen against us.
//! - `FundingAccrual` is the append-only periodic cash-flow record: positive
//!   funding rate means longs pay (research §4); our signed amount uses
//!   positive = received, negative = paid (matches the venue's
//!   funding_history convention) and rounds toward -inf (against us).
//! - `PerpPosition` is SIGNED (positive = long, negative = short); perp
//!   positions carry no settlement lifecycle. Unrealized PnL floors toward
//!   -inf; notional-at-mark ceils (exposure never understated).
//! - `MarginAccountView` applies conservative marking: if the venue
//!   settlement mark and our conservative mark disagree, the worse-for-us
//!   number governs halt math (spec 5.15).

use fortuna_core::market::{Contracts, MarketId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{
    FundingAccrual, FundingObservation, InstrumentKind, MarginAccountView, PerpError, PerpMarks,
    PerpPosition, PerpPrice, PerpValue,
};
use proptest::prelude::*;
use rust_decimal::Decimal;
use std::str::FromStr;

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn mkt(s: &str) -> MarketId {
    MarketId::new(s).unwrap()
}

fn ts(millis: i64) -> fortuna_core::clock::UtcTimestamp {
    fortuna_core::clock::UtcTimestamp::from_epoch_millis(millis).unwrap()
}

// ---- InstrumentKind ----

#[test]
fn instrument_kind_serde_is_snake_case() {
    assert_eq!(
        serde_json::to_string(&InstrumentKind::BinaryEvent).unwrap(),
        "\"binary_event\""
    );
    assert_eq!(
        serde_json::to_string(&InstrumentKind::Perp).unwrap(),
        "\"perp\""
    );
    let back: InstrumentKind = serde_json::from_str("\"perp\"").unwrap();
    assert_eq!(back, InstrumentKind::Perp);
}

// ---- PerpPrice construction and raw access ----

#[test]
fn perp_price_new_and_raw_round_trip() {
    assert_eq!(PerpPrice::new(62600).raw(), 62600);
    assert_eq!(PerpPrice::new(-1).raw(), -1);
    assert_eq!(PerpPrice::ZERO.raw(), 0);
}

#[test]
fn perp_price_serde_is_transparent_integer() {
    let json = serde_json::to_string(&PerpPrice::new(62600)).unwrap();
    assert_eq!(json, "62600");
    let back: PerpPrice = serde_json::from_str("62600").unwrap();
    assert_eq!(back, PerpPrice::new(62600));
    // A price from a JSON float must never silently truncate.
    assert!(serde_json::from_str::<PerpPrice>("6.26").is_err());
    assert!(serde_json::from_str::<PerpPrice>("\"62600\"").is_err());
}

#[test]
fn perp_price_display_is_four_decimal_dollars() {
    // BTCPERP-style quote: $6.2600 per contract (0.0001 BTC).
    assert_eq!(PerpPrice::new(62600).to_string(), "$6.2600");
    assert_eq!(PerpPrice::new(1).to_string(), "$0.0001");
    assert_eq!(PerpPrice::ZERO.to_string(), "$0.0000");
    assert_eq!(PerpPrice::new(-1).to_string(), "-$0.0001");
    // i64::MIN must render without panicking (abs() would overflow).
    assert_eq!(
        PerpPrice::new(i64::MIN).to_string(),
        "-$922337203685477.5808"
    );
}

// ---- PerpPrice checked arithmetic ----

#[test]
fn perp_price_checked_add_sub_basic_and_overflow() {
    assert_eq!(
        PerpPrice::new(62600).checked_add(PerpPrice::new(400)),
        Ok(PerpPrice::new(63000))
    );
    assert_eq!(
        PerpPrice::new(62600).checked_sub(PerpPrice::new(63000)),
        Ok(PerpPrice::new(-400))
    );
    assert!(matches!(
        PerpPrice::new(i64::MAX).checked_add(PerpPrice::new(1)),
        Err(PerpError::Overflow { .. })
    ));
    assert!(matches!(
        PerpPrice::new(i64::MIN).checked_sub(PerpPrice::new(1)),
        Err(PerpError::Overflow { .. })
    ));
}

#[test]
fn perp_price_times_quantity_is_perp_value() {
    // $6.2600 x 10 contracts = $62.60 of notional, still in ten-thousandths.
    assert_eq!(
        PerpPrice::new(62600).checked_mul(10),
        Ok(PerpValue::new(626000))
    );
    assert_eq!(PerpPrice::new(62600).checked_mul(0), Ok(PerpValue::ZERO));
    // Negative quantities arise from signed-position PnL math; must work.
    assert_eq!(
        PerpPrice::new(62600).checked_mul(-2),
        Ok(PerpValue::new(-125200))
    );
    assert!(matches!(
        PerpPrice::new(i64::MAX / 2).checked_mul(3),
        Err(PerpError::Overflow { .. })
    ));
}

// ---- PerpPrice Decimal boundary (venue payloads are fixed-point strings) ----

#[test]
fn perp_price_from_dollars_exact_whole_ticks() {
    // The venue quotes prices as fixed-point dollar strings at $0.0001 tick.
    assert_eq!(
        PerpPrice::from_dollars_exact(dec("6.2600")),
        Ok(PerpPrice::new(62600))
    );
    assert_eq!(
        PerpPrice::from_dollars_exact(dec("3")),
        Ok(PerpPrice::new(30000))
    );
    // Trailing zeros beyond the tick are still exact.
    assert_eq!(
        PerpPrice::from_dollars_exact(dec("6.260000")),
        Ok(PerpPrice::new(62600))
    );
}

#[test]
fn perp_price_from_dollars_exact_rejects_sub_tick_remainder() {
    assert!(matches!(
        PerpPrice::from_dollars_exact(dec("0.00005")),
        Err(PerpError::SubTickRemainder { .. })
    ));
    assert!(matches!(
        PerpPrice::from_dollars_exact(dec("-0.00005")),
        Err(PerpError::SubTickRemainder { .. })
    ));
}

#[test]
fn perp_price_from_dollars_floor_and_ceil_round_toward_named_direction() {
    assert_eq!(
        PerpPrice::from_dollars_floor(dec("0.00005")),
        Ok(PerpPrice::ZERO)
    );
    assert_eq!(
        PerpPrice::from_dollars_ceil(dec("0.00005")),
        Ok(PerpPrice::new(1))
    );
    assert_eq!(
        PerpPrice::from_dollars_floor(dec("-0.00005")),
        Ok(PerpPrice::new(-1))
    );
    assert_eq!(
        PerpPrice::from_dollars_ceil(dec("-0.00005")),
        Ok(PerpPrice::ZERO)
    );
    // Exact values are unchanged by either.
    assert_eq!(
        PerpPrice::from_dollars_floor(dec("6.2600")),
        Ok(PerpPrice::new(62600))
    );
    assert_eq!(
        PerpPrice::from_dollars_ceil(dec("6.2600")),
        Ok(PerpPrice::new(62600))
    );
}

#[test]
fn perp_price_from_dollars_out_of_range_is_error() {
    for f in [
        PerpPrice::from_dollars_exact,
        PerpPrice::from_dollars_floor,
        PerpPrice::from_dollars_ceil,
    ] {
        assert!(matches!(f(Decimal::MAX), Err(PerpError::OutOfRange { .. })));
        assert!(matches!(f(Decimal::MIN), Err(PerpError::OutOfRange { .. })));
    }
}

#[test]
fn perp_price_to_dollars_round_trips() {
    assert_eq!(PerpPrice::new(62600).to_dollars(), dec("6.2600"));
    assert_eq!(
        PerpPrice::from_dollars_exact(PerpPrice::new(i64::MAX).to_dollars()),
        Ok(PerpPrice::new(i64::MAX))
    );
    assert_eq!(
        PerpPrice::from_dollars_exact(PerpPrice::new(i64::MIN).to_dollars()),
        Ok(PerpPrice::new(i64::MIN))
    );
}

// ---- PerpValue -> Cents: THE conversion boundary (rounding against us) ----

#[test]
fn perp_value_to_cents_divisible_is_exact_both_ways() {
    // 626000 tt = $62.60 = 6260 cents, exactly.
    assert_eq!(PerpValue::new(626000).to_cents_floor(), Cents::new(6260));
    assert_eq!(PerpValue::new(626000).to_cents_ceil(), Cents::new(6260));
    assert_eq!(PerpValue::ZERO.to_cents_floor(), Cents::ZERO);
    assert_eq!(PerpValue::ZERO.to_cents_ceil(), Cents::ZERO);
}

#[test]
fn perp_value_to_cents_rounds_toward_named_direction() {
    // 62650 tt = $6.265 = 626.5 cents.
    assert_eq!(PerpValue::new(62650).to_cents_floor(), Cents::new(626));
    assert_eq!(PerpValue::new(62650).to_cents_ceil(), Cents::new(627));
    // -50 tt = -0.5 cents: floor -> -1 (a loss never shrinks), ceil -> 0.
    assert_eq!(PerpValue::new(-50).to_cents_floor(), Cents::new(-1));
    assert_eq!(PerpValue::new(-50).to_cents_ceil(), Cents::ZERO);
    // Extremes must not panic.
    let _ = PerpValue::new(i64::MIN).to_cents_floor();
    let _ = PerpValue::new(i64::MIN).to_cents_ceil();
    let _ = PerpValue::new(i64::MAX).to_cents_floor();
    let _ = PerpValue::new(i64::MAX).to_cents_ceil();
}

#[test]
fn perp_value_checked_ops() {
    assert_eq!(
        PerpValue::new(100).checked_add(PerpValue::new(23)),
        Ok(PerpValue::new(123))
    );
    assert_eq!(
        PerpValue::new(100).checked_sub(PerpValue::new(150)),
        Ok(PerpValue::new(-50))
    );
    assert!(matches!(
        PerpValue::new(i64::MAX).checked_add(PerpValue::new(1)),
        Err(PerpError::Overflow { .. })
    ));
}

// ---- FundingAccrual (append-only cash-flow record; research §4 semantics) ----

#[test]
fn funding_long_pays_when_rate_positive() {
    // rate +0.01% (the venue's zero threshold), mark $6.2600, long 100:
    // flow = -(0.0001 * 6.26 * 100) = -$0.0626 -> floor -> -7 cents.
    let acc = FundingAccrual::accrue(
        mkt("KXBTCPERP"),
        ts(1_760_000_000_000),
        dec("0.0001"),
        PerpPrice::new(62600),
        Contracts::new(100),
    )
    .unwrap();
    assert_eq!(acc.amount, Cents::new(-7));
}

#[test]
fn funding_short_receives_when_rate_positive() {
    // Same inputs, short side: flow = +$0.0626 -> floor -> +6 cents.
    // Conservative asymmetry: what we receive rounds down, what we pay
    // rounds up in magnitude.
    let acc = FundingAccrual::accrue(
        mkt("KXBTCPERP"),
        ts(1_760_000_000_000),
        dec("0.0001"),
        PerpPrice::new(62600),
        Contracts::new(-100),
    )
    .unwrap();
    assert_eq!(acc.amount, Cents::new(6));
}

#[test]
fn funding_negative_rate_pays_shorts() {
    // Observed live regime: negative funding (perp below index) -> shorts
    // pay longs. Long receives; short pays.
    let rate = dec("-0.0003971378687289"); // real KXBCHPERP record 2026-06-11
    let mark = PerpPrice::new(26605); // $2.6605
    let long = FundingAccrual::accrue(
        mkt("KXBCHPERP"),
        ts(1_760_000_000_000),
        rate,
        mark,
        Contracts::new(1000),
    )
    .unwrap();
    let short = FundingAccrual::accrue(
        mkt("KXBCHPERP"),
        ts(1_760_000_000_000),
        rate,
        mark,
        Contracts::new(-1000),
    )
    .unwrap();
    // flow_long = -(rate * 2.6605 * 1000) = +$1.05656... -> floor -> 105.
    assert_eq!(long.amount, Cents::new(105));
    // flow_short = -$1.05656... -> floor -> -106.
    assert_eq!(short.amount, Cents::new(-106));
}

#[test]
fn funding_zero_rate_or_flat_position_is_zero_amount() {
    let zero_rate = FundingAccrual::accrue(
        mkt("KXBTCPERP"),
        ts(0),
        Decimal::ZERO,
        PerpPrice::new(62600),
        Contracts::new(100),
    )
    .unwrap();
    assert_eq!(zero_rate.amount, Cents::ZERO);
    let flat = FundingAccrual::accrue(
        mkt("KXBTCPERP"),
        ts(0),
        dec("0.0001"),
        PerpPrice::new(62600),
        Contracts::ZERO,
    )
    .unwrap();
    assert_eq!(flat.amount, Cents::ZERO);
}

#[test]
fn funding_record_preserves_reconciliation_fields() {
    // The record must carry everything needed to reconcile against the
    // venue's funding_history endpoint (rate, mark, qty, time).
    let acc = FundingAccrual::accrue(
        mkt("KXBTCPERP"),
        ts(1_760_000_000_000),
        dec("0.0001"),
        PerpPrice::new(62600),
        Contracts::new(100),
    )
    .unwrap();
    assert_eq!(acc.market, mkt("KXBTCPERP"));
    assert_eq!(acc.funding_time, ts(1_760_000_000_000));
    assert_eq!(acc.rate, dec("0.0001"));
    assert_eq!(acc.settlement_mark, PerpPrice::new(62600));
    assert_eq!(acc.position_qty, Contracts::new(100));
}

// ---- PerpPosition (signed; no settlement lifecycle) ----

fn pos(qty: i64, entry: i64) -> PerpPosition {
    PerpPosition {
        market: mkt("KXBTCPERP"),
        qty: Contracts::new(qty),
        avg_entry: PerpPrice::new(entry),
    }
}

#[test]
fn position_sign_helpers() {
    assert!(pos(10, 62600).is_long());
    assert!(pos(-10, 62600).is_short());
    assert!(pos(0, 62600).is_flat());
    assert!(!pos(10, 62600).is_short());
    assert!(!pos(-10, 62600).is_flat());
}

#[test]
fn long_unrealized_pnl_signs() {
    // entry $6.0000, mark $6.5000, +10: (65000-60000) x 10 tt = $5.00.
    assert_eq!(
        pos(10, 60000).unrealized_pnl(PerpPrice::new(65000)),
        Ok(Cents::new(500))
    );
    // Mark below entry: long loses.
    assert_eq!(
        pos(10, 60000).unrealized_pnl(PerpPrice::new(55000)),
        Ok(Cents::new(-500))
    );
}

#[test]
fn short_unrealized_pnl_signs() {
    // Short profits when the mark falls, loses when it rises.
    assert_eq!(
        pos(-10, 60000).unrealized_pnl(PerpPrice::new(55000)),
        Ok(Cents::new(500))
    );
    assert_eq!(
        pos(-10, 60000).unrealized_pnl(PerpPrice::new(65000)),
        Ok(Cents::new(-500))
    );
}

#[test]
fn unrealized_pnl_floors_toward_negative_infinity() {
    // One tick in our favor x 3 contracts = 3 tt = 0.03 cents -> floor 0:
    // sub-cent gains are never counted.
    assert_eq!(
        pos(3, 60000).unrealized_pnl(PerpPrice::new(60001)),
        Ok(Cents::ZERO)
    );
    // One tick against x 3 = -0.03 cents -> floor -1: sub-cent losses round
    // to a full cent against us.
    assert_eq!(
        pos(3, 60001).unrealized_pnl(PerpPrice::new(60000)),
        Ok(Cents::new(-1))
    );
}

#[test]
fn flat_position_marks_zero() {
    assert_eq!(
        pos(0, 60000).unrealized_pnl(PerpPrice::new(99999)),
        Ok(Cents::ZERO)
    );
    assert_eq!(
        pos(0, 60000).notional_at(PerpPrice::new(99999)),
        Ok(Cents::ZERO)
    );
}

#[test]
fn notional_at_uses_absolute_quantity_and_ceils() {
    // Exposure is direction-blind and never understated.
    assert_eq!(
        pos(10, 60000).notional_at(PerpPrice::new(62600)),
        Ok(Cents::new(6260))
    );
    assert_eq!(
        pos(-10, 60000).notional_at(PerpPrice::new(62600)),
        Ok(Cents::new(6260))
    );
    // 1 x 62650 tt = 626.5 cents -> ceil 627.
    assert_eq!(
        pos(1, 60000).notional_at(PerpPrice::new(62650)),
        Ok(Cents::new(627))
    );
}

// ---- MarginAccountView (conservative marking; spec 5.15 halt-math rule) ----

fn marks(venue: i64, ours: Option<i64>) -> PerpMarks {
    PerpMarks {
        venue_settlement: PerpPrice::new(venue),
        conservative: ours.map(PerpPrice::new),
    }
}

#[test]
fn view_uses_worse_for_us_mark_long() {
    // Long position: the LOWER mark is worse for us.
    let p = pos(10, 60000);
    let worse_ours = MarginAccountView::compute(
        Cents::new(10_000),
        &[(p.clone(), marks(65000, Some(64000)))],
        Cents::ZERO,
    )
    .unwrap();
    // uPnL at ours (64000): $4.00; at venue (65000): $5.00 -> ours governs.
    assert_eq!(worse_ours.unrealized, Cents::new(400));
    let worse_venue = MarginAccountView::compute(
        Cents::new(10_000),
        &[(p, marks(64000, Some(65000)))],
        Cents::ZERO,
    )
    .unwrap();
    assert_eq!(worse_venue.unrealized, Cents::new(400));
}

#[test]
fn view_uses_worse_for_us_mark_short() {
    // Short position: the HIGHER mark is worse for us. The rule is
    // worse-uPnL, not lower-price.
    let p = pos(-10, 60000);
    let view = MarginAccountView::compute(
        Cents::new(10_000),
        &[(p, marks(55000, Some(56000)))],
        Cents::ZERO,
    )
    .unwrap();
    // uPnL at venue (55000): +$5.00; at ours (56000): +$4.00 -> ours governs.
    assert_eq!(view.unrealized, Cents::new(400));
}

#[test]
fn view_missing_conservative_mark_flags_unmarked() {
    let view = MarginAccountView::compute(
        Cents::new(10_000),
        &[(pos(10, 60000), marks(65000, None))],
        Cents::ZERO,
    )
    .unwrap();
    assert!(view.unmarked_flag);
    // Venue mark still values the position (the only number available).
    assert_eq!(view.unrealized, Cents::new(500));

    let marked = MarginAccountView::compute(
        Cents::new(10_000),
        &[(pos(10, 60000), marks(65000, Some(65000)))],
        Cents::ZERO,
    )
    .unwrap();
    assert!(!marked.unmarked_flag);
}

#[test]
fn view_equity_is_balance_plus_unrealized_plus_pending_funding() {
    let view = MarginAccountView::compute(
        Cents::new(10_000),
        &[(pos(10, 60000), marks(65000, Some(65000)))],
        Cents::new(-30),
    )
    .unwrap();
    assert_eq!(view.balance, Cents::new(10_000));
    assert_eq!(view.unrealized, Cents::new(500));
    assert_eq!(view.pending_funding, Cents::new(-30));
    assert_eq!(view.equity, Cents::new(10_470));
}

#[test]
fn view_sums_multiple_positions() {
    let view = MarginAccountView::compute(
        Cents::new(10_000),
        &[
            (pos(10, 60000), marks(65000, Some(65000))), // +500
            (pos(-5, 70000), marks(72000, Some(73000))), // worse mark 73000: -150
        ],
        Cents::ZERO,
    )
    .unwrap();
    assert_eq!(view.unrealized, Cents::new(350));
    assert_eq!(view.equity, Cents::new(10_350));
}

#[test]
fn view_empty_positions_is_balance_only() {
    let view = MarginAccountView::compute(Cents::new(10_000), &[], Cents::ZERO).unwrap();
    assert_eq!(view.unrealized, Cents::ZERO);
    assert_eq!(view.equity, Cents::new(10_000));
    assert!(!view.unmarked_flag);
}

// ---- properties: conversions round against us, never panic ----

proptest! {
    #[test]
    fn prop_perp_checked_ops_never_panic(a in any::<i64>(), b in any::<i64>()) {
        let _ = PerpPrice::new(a).checked_add(PerpPrice::new(b));
        let _ = PerpPrice::new(a).checked_sub(PerpPrice::new(b));
        let _ = PerpPrice::new(a).checked_mul(b);
        let _ = PerpValue::new(a).checked_add(PerpValue::new(b));
        let _ = PerpValue::new(a).checked_sub(PerpValue::new(b));
        let _ = PerpValue::new(a).to_cents_floor();
        let _ = PerpValue::new(a).to_cents_ceil();
    }

    #[test]
    fn prop_perp_price_to_from_dollars_round_trip(a in any::<i64>()) {
        prop_assert_eq!(
            PerpPrice::from_dollars_exact(PerpPrice::new(a).to_dollars()),
            Ok(PerpPrice::new(a))
        );
    }

    #[test]
    fn prop_to_cents_floor_le_ceil_within_one_and_exact_iff_divisible(a in any::<i64>()) {
        let v = PerpValue::new(a);
        let floor = v.to_cents_floor();
        let ceil = v.to_cents_ceil();
        prop_assert!(floor <= ceil);
        prop_assert!(ceil.raw() - floor.raw() <= 1);
        if a % 100 == 0 {
            prop_assert_eq!(floor, ceil);
        } else {
            prop_assert_eq!(ceil.raw() - floor.raw(), 1);
        }
        // i128 reference: floor(a/100) toward -inf.
        let reference = (i128::from(a)).div_euclid(100);
        prop_assert_eq!(i128::from(floor.raw()), reference);
    }

    #[test]
    fn prop_notional_matches_i128_reference(
        price in -1_000_000_000_000i64..1_000_000_000_000,
        qty in -1_000_000i64..1_000_000,
    ) {
        let reference = i128::from(price) * i128::from(qty);
        match PerpPrice::new(price).checked_mul(qty) {
            Ok(v) => prop_assert_eq!(i128::from(v.raw()), reference),
            Err(_) => prop_assert!(
                reference > i128::from(i64::MAX) || reference < i128::from(i64::MIN)
            ),
        }
    }

    #[test]
    fn prop_funding_amount_is_floored_exact_flow(
        rate_ppm in -20_000i64..=20_000, // +/- 2% cap, in parts-per-million
        mark in 1i64..1_000_000_000,
        qty in -1_000_000i64..1_000_000,
    ) {
        let rate = Decimal::new(rate_ppm, 6);
        let acc = FundingAccrual::accrue(
            mkt("KXBTCPERP"),
            ts(0),
            rate,
            PerpPrice::new(mark),
            Contracts::new(qty),
        ).unwrap();
        // Exact signed flow in dollars: -(rate * mark$ * qty).
        let exact = -(rate * Decimal::new(mark, 4) * Decimal::from(qty));
        // amount = floor(exact) in cents: amount <= exact < amount + 1c.
        let amount_dollars = acc.amount.to_dollars();
        prop_assert!(amount_dollars <= exact);
        prop_assert!(exact - amount_dollars < dec("0.01"));
    }

    #[test]
    fn prop_unrealized_pnl_is_floored_exact_value(
        entry in 1i64..1_000_000_000,
        mark in 1i64..1_000_000_000,
        qty in -1_000_000i64..1_000_000,
    ) {
        let p = PerpPosition {
            market: mkt("KXBTCPERP"),
            qty: Contracts::new(qty),
            avg_entry: PerpPrice::new(entry),
        };
        let upnl = p.unrealized_pnl(PerpPrice::new(mark)).unwrap();
        // Exact value in ten-thousandths, via i128 (no overflow possible).
        let exact_tt = (i128::from(mark) - i128::from(entry)) * i128::from(qty);
        let floor_cents = exact_tt.div_euclid(100);
        prop_assert_eq!(i128::from(upnl.raw()), floor_cents);
    }

    #[test]
    fn prop_notional_at_never_understates(
        entry in 1i64..1_000_000_000,
        mark in 1i64..1_000_000_000,
        qty in -1_000_000i64..1_000_000,
    ) {
        let p = PerpPosition {
            market: mkt("KXBTCPERP"),
            qty: Contracts::new(qty),
            avg_entry: PerpPrice::new(entry),
        };
        let notional = p.notional_at(PerpPrice::new(mark)).unwrap();
        // ceil(|qty| * mark / 100) in cents, via i128 reference.
        let exact_tt = i128::from(qty).abs() * i128::from(mark);
        let ceil_cents = exact_tt.div_euclid(100)
            + i128::from(exact_tt.rem_euclid(100) != 0);
        prop_assert_eq!(i128::from(notional.raw()), ceil_cents);
        // Exposure in cents always covers the exact ten-thousandths value.
        prop_assert!(i128::from(notional.raw()) * 100 >= exact_tt);
    }
}

// ---- FundingObservation (perp-strategies design §2.1, §4) ----

fn funding_obs() -> FundingObservation {
    FundingObservation {
        // A small NEGATIVE decimal fraction with trailing precision — the
        // venue's recorded estimate (running TWAP), not a price.
        estimate: dec("-0.00012500"),
        next_funding_time: ts(1_718_294_400_000),
        reference_price: PerpPrice::new(626_000_000),
        obs_at: ts(1_718_290_800_000),
    }
}

#[test]
fn funding_observation_serde_round_trips() {
    let obs = funding_obs();
    let json = serde_json::to_string(&obs).unwrap();
    let back: FundingObservation = serde_json::from_str(&json).unwrap();
    assert_eq!(obs, back);
}

#[test]
fn funding_observation_serde_is_byte_stable() {
    // CRITICAL (design §2.5 / bus replay byte-compare): `Decimal` serializes
    // as a STRING; confirm to_string -> from_str -> to_string is byte-identical
    // for the funding estimate so a PerpTick replays byte-for-byte. The
    // fallback (i64 fixed-point rate) would only be needed if this fails.
    let obs = funding_obs();
    let once = serde_json::to_string(&obs).unwrap();
    let back: FundingObservation = serde_json::from_str(&once).unwrap();
    let twice = serde_json::to_string(&back).unwrap();
    assert_eq!(once, twice, "FundingObservation JSON is not byte-stable");
    // The Decimal preserves its scale/precision verbatim across the round trip
    // (the reconciliation-grade discipline FundingAccrual.rate also carries).
    assert_eq!(back.estimate, dec("-0.00012500"));
    assert_eq!(back.estimate.to_string(), "-0.00012500");
}

#[test]
fn funding_observation_decimal_scale_survives_round_trip() {
    // A spread of estimate magnitudes/scales (zero, tiny, capped, high
    // precision): each must round-trip byte-stable through serde_json. This is
    // the load-bearing property the whole PerpTick replay rests on.
    for s in [
        "0",
        "0.0001",
        "-0.0001",
        "0.02",
        "-0.02",
        "0.000123456789",
        "-0.000000000001",
        "0.00012500",
        "-0.00012500",
    ] {
        let obs = FundingObservation {
            estimate: dec(s),
            next_funding_time: ts(1_718_294_400_000),
            reference_price: PerpPrice::new(626_000_000),
            obs_at: ts(1_718_290_800_000),
        };
        let once = serde_json::to_string(&obs).unwrap();
        let back: FundingObservation = serde_json::from_str(&once).unwrap();
        let twice = serde_json::to_string(&back).unwrap();
        assert_eq!(once, twice, "estimate {s} not byte-stable");
        assert_eq!(back.estimate, dec(s), "estimate {s} value drifted");
    }
}
