//! T0.3 tests: config-driven fee engine. Written from spec 5.2 before
//! implementation.
//!
//! "Fee schedules are data, not code": one engine interprets versioned
//! schedules (quadratic p(1-p) | flat bps | tiered; maker/taker variants;
//! category multipliers; effective_date). Rounding is ALWAYS up (against us).
//! Worked vectors below are hand-computed.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::Contracts;
use fortuna_core::money::Cents;
use fortuna_venues::fees::{FeeModel, FeeSchedule, FillRole, ScheduleFeeModel};
use fortuna_venues::VenueError;

fn at(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(s).unwrap()
}

fn schedule(toml_src: &str) -> FeeSchedule {
    toml::from_str(toml_src).unwrap()
}

fn quadratic_model() -> ScheduleFeeModel {
    ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
            maker_coeff = "0.0175"
        "#,
    )])
    .unwrap()
}

const T: &str = "2026-06-09T00:00:00.000Z";

// ---- quadratic ----

#[test]
fn quadratic_taker_exact_cents() {
    // 0.07 * 100 * 0.5 * 0.5 = $1.75 exactly.
    let m = quadratic_model();
    let fee = m
        .fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at(T),
        )
        .unwrap();
    assert_eq!(fee, Cents::new(175));
}

#[test]
fn quadratic_rounds_up_to_the_next_cent_against_us() {
    let m = quadratic_model();
    // 0.07 * 1 * 0.25 = $0.0175 -> 1.75 cents -> 2 cents.
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(1),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(2)
    );
    // 0.07 * 7 * 0.33 * 0.67 = $0.108339 -> 10.8339 cents -> 11 cents.
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(33),
            Contracts::new(7),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(11)
    );
    // 0.07 * 10 * 0.01 * 0.99 = $0.00693 -> 0.693 cents -> 1 cent.
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(1),
            Contracts::new(10),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(1)
    );
}

#[test]
fn quadratic_maker_uses_maker_coefficient() {
    let m = quadratic_model();
    // 0.0175 * 100 * 0.25 = $0.4375 -> 43.75 cents -> 44 cents.
    assert_eq!(
        m.fee(
            FillRole::Maker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(44)
    );
}

#[test]
fn missing_maker_variant_means_zero_maker_fee() {
    let m = ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
        "#,
    )])
    .unwrap();
    assert_eq!(
        m.fee(
            FillRole::Maker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at(T)
        )
        .unwrap(),
        Cents::ZERO
    );
}

#[test]
fn extreme_prices_have_zero_quadratic_fee() {
    let m = quadratic_model();
    for price in [0, 100] {
        assert_eq!(
            m.fee(
                FillRole::Taker,
                Cents::new(price),
                Contracts::new(50),
                None,
                at(T)
            )
            .unwrap(),
            Cents::ZERO
        );
    }
}

// ---- flat bps ----

#[test]
fn flat_bps_on_notional() {
    let m = ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "flat_bps"
            effective_date = "2026-01-01"
            taker_bps = 10
        "#,
    )])
    .unwrap();
    // Notional 100 * 50c = $50; 10 bps = $0.05 = 5 cents exact.
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(5)
    );
    // Notional 33 cents; 10 bps = 0.033 cents -> rounds up to 1 cent.
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(33),
            Contracts::new(1),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(1)
    );
    // Maker variant missing -> zero.
    assert_eq!(
        m.fee(
            FillRole::Maker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at(T)
        )
        .unwrap(),
        Cents::ZERO
    );
}

// ---- tiered ----

#[test]
fn tiered_selects_rate_by_notional_bracket() {
    let m = ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "tiered"
            effective_date = "2026-01-01"
            [[taker_tiers]]
            up_to_notional_cents = 1000
            bps = 50
            [[taker_tiers]]
            bps = 10
        "#,
    )])
    .unwrap();
    // $5 notional -> first tier 50 bps: 500 * 0.005 = 2.5 cents -> 3.
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(10),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(3)
    );
    // Exactly $10 notional -> still first tier: 1000 * 0.005 = 5 cents.
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(20),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(5)
    );
    // $50 notional -> second tier 10 bps: 5000 * 0.001 = 5 cents.
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(5)
    );
}

#[test]
fn tiered_requires_an_unbounded_final_tier() {
    let result = ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "tiered"
            effective_date = "2026-01-01"
            [[taker_tiers]]
            up_to_notional_cents = 1000
            bps = 50
        "#,
    )]);
    assert!(matches!(result, Err(VenueError::FeeConfig { .. })));
}

// ---- category multipliers ----

#[test]
fn category_multiplier_applies_before_rounding() {
    let m = ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
            [category_multipliers]
            sp500 = "0.5"
        "#,
    )])
    .unwrap();
    // 175 cents * 0.5 = 87.5 -> 88 cents.
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(100),
            Some("sp500"),
            at(T)
        )
        .unwrap(),
        Cents::new(88)
    );
    // Unknown category -> multiplier 1.
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(100),
            Some("weather"),
            at(T)
        )
        .unwrap(),
        Cents::new(175)
    );
}

// ---- effective-date versioning ----

#[test]
fn versioning_picks_the_latest_schedule_at_or_before_the_trade_time() {
    let m = ScheduleFeeModel::new(vec![
        schedule(
            r#"
                formula = "quadratic"
                effective_date = "2026-01-01"
                taker_coeff = "0.07"
            "#,
        ),
        schedule(
            r#"
                formula = "quadratic"
                effective_date = "2026-06-01"
                taker_coeff = "0.05"
            "#,
        ),
    ])
    .unwrap();
    let f = |t: &str| {
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at(t),
        )
        .unwrap()
    };
    assert_eq!(f("2026-05-01T00:00:00.000Z"), Cents::new(175)); // old schedule
    assert_eq!(f("2026-06-01T00:00:00.000Z"), Cents::new(125)); // boundary: new
    assert_eq!(f("2026-06-09T00:00:00.000Z"), Cents::new(125));
    // Before any schedule is effective: error, never a silent zero.
    assert!(matches!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at("2025-12-31T23:59:59.999Z")
        ),
        Err(VenueError::NoEffectiveSchedule { .. })
    ));
}

#[test]
fn full_timestamp_effective_date_is_accepted() {
    let m = ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01T12:30:00.000Z"
            taker_coeff = "0.07"
        "#,
    )])
    .unwrap();
    assert!(m
        .fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(1),
            None,
            at(T)
        )
        .is_ok());
}

// ---- maker rebates + rounding modes (Polymarket US, researched 2026-06-09) ----
// docs/research/venue/polymarket-fees-2026-06-09: taker theta 0.05, maker
// theta -0.0125 (rebate paid at trade time), banker's rounding to the cent.

#[test]
fn polymarket_us_schedule_taker_and_maker_rebate_with_bankers_rounding() {
    let m = ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "quadratic"
            effective_date = "2026-04-03"
            taker_coeff = "0.05"
            maker_coeff = "-0.0125"
            rounding = "half_even"
        "#,
    )])
    .unwrap();
    // Taker 100 @ 50c: 0.05 * 100 * 0.25 = $1.25 exactly (the documented
    // "$1.25 per 100-lot at 50c" maximum).
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(125)
    );
    // Maker 100 @ 50c: -0.0125 * 100 * 0.25 = -$0.3125 -> -31.25c ->
    // nearest cent -31 (a rebate: negative fee).
    assert_eq!(
        m.fee(
            FillRole::Maker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(-31)
    );
    // Banker's midpoints: 37.5c -> 38 (even); 12.5c -> 12 (even).
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(30),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(38)
    );
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(10),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(12)
    );
}

#[test]
fn default_up_rounding_truncates_rebates_toward_zero() {
    // Without an explicit rounding mode, "up" (against us) applies in both
    // directions: fees round up, rebate magnitudes round down.
    let m = ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.05"
            maker_coeff = "-0.0127"
        "#,
    )])
    .unwrap();
    // Maker 100 @ 50c: -0.0127 * 100 * 0.25 = -$0.3175 -> ceil -> -31c
    // (we model receiving LESS than the exact -31.75c).
    assert_eq!(
        m.fee(
            FillRole::Maker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at(T)
        )
        .unwrap(),
        Cents::new(-31)
    );
}

#[test]
fn taker_rebates_are_rejected_at_construction() {
    // No venue prices negative taker fees per fill; a negative taker
    // coefficient is a config mistake, not a rebate.
    let result = ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "-0.05"
        "#,
    )]);
    assert!(matches!(result, Err(VenueError::FeeConfig { .. })));
}

// ---- input validation ----

#[test]
fn prices_outside_0_to_100_cents_are_invalid() {
    let m = quadratic_model();
    for bad in [-1, 101, 10_000] {
        assert!(matches!(
            m.fee(
                FillRole::Taker,
                Cents::new(bad),
                Contracts::new(1),
                None,
                at(T)
            ),
            Err(VenueError::Invalid { .. })
        ));
    }
}

#[test]
fn negative_quantity_is_invalid_zero_quantity_is_free() {
    let m = quadratic_model();
    assert!(matches!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(-1),
            None,
            at(T)
        ),
        Err(VenueError::Invalid { .. })
    ));
    assert_eq!(
        m.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(0),
            None,
            at(T)
        )
        .unwrap(),
        Cents::ZERO
    );
}

#[test]
fn malformed_coefficients_fail_at_construction_not_at_fee_time() {
    let result = ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "not-a-number"
        "#,
    )]);
    assert!(matches!(result, Err(VenueError::FeeConfig { .. })));
    // Negative coefficients are config errors too (fees are never rebates here).
    let result = ScheduleFeeModel::new(vec![schedule(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "-0.07"
        "#,
    )]);
    assert!(matches!(result, Err(VenueError::FeeConfig { .. })));
}

// ---- properties ----

use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_fee_is_never_negative_and_monotone_in_quantity(
        price in 0i64..=100,
        qty in 0i64..10_000,
        extra in 0i64..1_000,
    ) {
        let m = quadratic_model();
        let f1 = m.fee(FillRole::Taker, Cents::new(price), Contracts::new(qty), None, at(T)).unwrap();
        let f2 = m.fee(FillRole::Taker, Cents::new(price), Contracts::new(qty + extra), None, at(T)).unwrap();
        prop_assert!(f1 >= Cents::ZERO);
        prop_assert!(f2 >= f1);
    }

    #[test]
    fn prop_rounding_never_undercharges(
        price in 1i64..=99,
        qty in 1i64..10_000,
    ) {
        // ceil(x) >= x: the integer fee always covers the exact decimal fee.
        use rust_decimal::Decimal;
        use std::str::FromStr;
        let m = quadratic_model();
        let fee = m.fee(FillRole::Taker, Cents::new(price), Contracts::new(qty), None, at(T)).unwrap();
        let p = Decimal::new(price, 2);
        let exact_dollars = Decimal::from_str("0.07").unwrap()
            * Decimal::from(qty) * p * (Decimal::ONE - p);
        prop_assert!(fee.to_dollars() >= exact_dollars);
        // And never overcharges by a full cent.
        prop_assert!(fee.to_dollars() - exact_dollars < Decimal::new(1, 2));
    }
}
