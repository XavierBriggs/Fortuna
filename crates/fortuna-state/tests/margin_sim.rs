//! T5.B5 tests: paper-engine margin semantics (spec 5.15, plan §B5 —
//! written from spec/plan text BEFORE implementation).
//!
//! Contract under test:
//! - Mark-based PnL: signed position updates with VWAP entry (rounded
//!   against us by side), realized PnL floored toward -inf on every
//!   reduce/flip, fees debited from margin cash.
//! - Funding accrual on SimClock timestamps: the 04:00/12:00/20:00 UTC
//!   schedule (research §4, empirically confirmed) via
//!   `funding_times_between`; accruals apply to the balance and append to
//!   an append-only log; flat positions generate NO entry (mirrors the
//!   venue's funding_history).
//! - Liquidation simulation from the recorded risk curves: portfolio
//!   margin (account equity vs summed per-market maintenance requirement);
//!   equity below the requirement closes ALL positions at the conservative
//!   mark with a configured penalty AGAINST us; "a liquidation
//!   under-modeled = test failure, not surprise" — an unbounded curve or a
//!   missing mark for a held position is an ERROR, never a guess.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, Contracts, MarketId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{PerpMarks, PerpPrice};
use fortuna_state::{funding_times_between, MarginSim, MarginSimConfig, RiskCurve};
use proptest::prelude::*;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::str::FromStr;

fn ts(iso: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(iso).unwrap()
}

fn mkt(s: &str) -> MarketId {
    MarketId::new(s).unwrap()
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn config(penalty_bps: i64) -> MarginSimConfig {
    let mut curves = BTreeMap::new();
    curves.insert(
        mkt("KXBTCPERP"),
        RiskCurve {
            tiers: vec![(Cents::new(1_000_000), 500), (Cents::new(5_000_000), 800)],
        },
    );
    MarginSimConfig {
        mm_multiplier_pct: 100,
        liquidation_penalty_bps: penalty_bps,
        curves,
    }
}

fn sim(balance: i64) -> MarginSim {
    MarginSim::new(Cents::new(balance), config(0)).unwrap()
}

fn marks_at(price: i64) -> BTreeMap<MarketId, PerpMarks> {
    let mut m = BTreeMap::new();
    m.insert(
        mkt("KXBTCPERP"),
        PerpMarks {
            venue_settlement: PerpPrice::new(price),
            conservative: Some(PerpPrice::new(price)),
        },
    );
    m
}

// ---- position math (mark-based PnL) ----

#[test]
fn open_long_debits_fee_only() {
    let mut s = sim(100_000);
    let out = s
        .apply_fill(
            &mkt("KXBTCPERP"),
            Action::Buy,
            Contracts::new(100),
            PerpPrice::new(62_600),
            Cents::new(10),
        )
        .unwrap();
    assert_eq!(out.realized_pnl, Cents::ZERO);
    assert_eq!(s.balance(), Cents::new(99_990));
    let p = s.position(&mkt("KXBTCPERP")).unwrap();
    assert_eq!(p.qty, Contracts::new(100));
    assert_eq!(p.avg_entry, PerpPrice::new(62_600));
}

#[test]
fn add_to_long_vwaps_entry_rounded_against_us() {
    // Long entries round UP (a higher entry is worse for a long).
    // 100 @ 60,000 + 50 @ 60,001 -> (6,000,000 + 3,000,050)/150 =
    // 60,000.33... -> 60,001.
    let mut s = sim(1_000_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(100),
        PerpPrice::new(60_000),
        Cents::ZERO,
    )
    .unwrap();
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(50),
        PerpPrice::new(60_001),
        Cents::ZERO,
    )
    .unwrap();
    let p = s.position(&mkt("KXBTCPERP")).unwrap();
    assert_eq!(p.qty, Contracts::new(150));
    assert_eq!(p.avg_entry, PerpPrice::new(60_001));
}

#[test]
fn add_to_short_vwaps_entry_floored() {
    // Short entries round DOWN (a lower entry is worse for a short).
    let mut s = sim(1_000_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Sell,
        Contracts::new(100),
        PerpPrice::new(60_001),
        Cents::ZERO,
    )
    .unwrap();
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Sell,
        Contracts::new(50),
        PerpPrice::new(60_000),
        Cents::ZERO,
    )
    .unwrap();
    let p = s.position(&mkt("KXBTCPERP")).unwrap();
    assert_eq!(p.qty, Contracts::new(-150));
    // (6,000,100 + 3,000,000)/150 = 60,000.66... -> floor 60,000.
    assert_eq!(p.avg_entry, PerpPrice::new(60_000));
}

#[test]
fn reduce_long_realizes_floored_pnl() {
    let mut s = sim(100_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(100),
        PerpPrice::new(60_000),
        Cents::ZERO,
    )
    .unwrap();
    // Sell 40 @ 65,000: realized = 5,000 tt x 40 = $20.00 = 2,000c.
    let out = s
        .apply_fill(
            &mkt("KXBTCPERP"),
            Action::Sell,
            Contracts::new(40),
            PerpPrice::new(65_000),
            Cents::new(5),
        )
        .unwrap();
    assert_eq!(out.realized_pnl, Cents::new(2_000));
    assert_eq!(s.balance(), Cents::new(101_995));
    let p = s.position(&mkt("KXBTCPERP")).unwrap();
    assert_eq!(p.qty, Contracts::new(60));
    assert_eq!(p.avg_entry, PerpPrice::new(60_000)); // entry unchanged on reduce
}

#[test]
fn reduce_at_loss_realizes_negative() {
    let mut s = sim(100_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(100),
        PerpPrice::new(60_000),
        Cents::ZERO,
    )
    .unwrap();
    let out = s
        .apply_fill(
            &mkt("KXBTCPERP"),
            Action::Sell,
            Contracts::new(100),
            PerpPrice::new(55_000),
            Cents::ZERO,
        )
        .unwrap();
    assert_eq!(out.realized_pnl, Cents::new(-5_000));
    assert_eq!(s.balance(), Cents::new(95_000));
    assert!(s.position(&mkt("KXBTCPERP")).is_none()); // flat removes
}

#[test]
fn sub_cent_realized_floors_against_us() {
    // 3 contracts x 1 tick = 0.03c: a gain floors to 0, a loss to -1c.
    let mut s = sim(100_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(3),
        PerpPrice::new(60_000),
        Cents::ZERO,
    )
    .unwrap();
    let gain = s
        .apply_fill(
            &mkt("KXBTCPERP"),
            Action::Sell,
            Contracts::new(3),
            PerpPrice::new(60_001),
            Cents::ZERO,
        )
        .unwrap();
    assert_eq!(gain.realized_pnl, Cents::ZERO);

    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(3),
        PerpPrice::new(60_001),
        Cents::ZERO,
    )
    .unwrap();
    let loss = s
        .apply_fill(
            &mkt("KXBTCPERP"),
            Action::Sell,
            Contracts::new(3),
            PerpPrice::new(60_000),
            Cents::ZERO,
        )
        .unwrap();
    assert_eq!(loss.realized_pnl, Cents::new(-1));
}

#[test]
fn flip_realizes_closed_part_and_opens_remainder_at_fill_price() {
    let mut s = sim(100_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(100),
        PerpPrice::new(60_000),
        Cents::ZERO,
    )
    .unwrap();
    // Sell 150 @ 62,000: closes 100 (realized 2,000 tt x 100 = 2,000c),
    // opens short 50 @ 62,000.
    let out = s
        .apply_fill(
            &mkt("KXBTCPERP"),
            Action::Sell,
            Contracts::new(150),
            PerpPrice::new(62_000),
            Cents::ZERO,
        )
        .unwrap();
    assert_eq!(out.realized_pnl, Cents::new(2_000));
    let p = s.position(&mkt("KXBTCPERP")).unwrap();
    assert_eq!(p.qty, Contracts::new(-50));
    assert_eq!(p.avg_entry, PerpPrice::new(62_000));
}

#[test]
fn zero_or_negative_qty_fill_is_error() {
    let mut s = sim(100_000);
    assert!(s
        .apply_fill(
            &mkt("KXBTCPERP"),
            Action::Buy,
            Contracts::new(0),
            PerpPrice::new(60_000),
            Cents::ZERO,
        )
        .is_err());
    assert!(s
        .apply_fill(
            &mkt("KXBTCPERP"),
            Action::Buy,
            Contracts::new(-5),
            PerpPrice::new(60_000),
            Cents::ZERO,
        )
        .is_err());
}

// ---- funding accrual on SimClock timestamps ----

#[test]
fn funding_schedule_is_04_12_20_utc() {
    // Research §4 (empirically confirmed): funding_time values are exactly
    // 04:00, 12:00, 20:00 UTC, 8h apart.
    let times = funding_times_between(
        ts("2026-06-12T05:00:00.000Z"),
        ts("2026-06-13T05:00:00.000Z"),
    )
    .unwrap();
    assert_eq!(
        times,
        vec![
            ts("2026-06-12T12:00:00.000Z"),
            ts("2026-06-12T20:00:00.000Z"),
            ts("2026-06-13T04:00:00.000Z"),
        ]
    );
}

#[test]
fn funding_schedule_boundaries_are_exclusive_after_inclusive_until() {
    // A tick exactly at `after` is NOT due again; exactly at `until` IS.
    let times = funding_times_between(
        ts("2026-06-12T04:00:00.000Z"),
        ts("2026-06-12T20:00:00.000Z"),
    )
    .unwrap();
    assert_eq!(
        times,
        vec![
            ts("2026-06-12T12:00:00.000Z"),
            ts("2026-06-12T20:00:00.000Z"),
        ]
    );
    let none = funding_times_between(
        ts("2026-06-12T04:00:00.000Z"),
        ts("2026-06-12T11:59:59.999Z"),
    )
    .unwrap();
    assert!(none.is_empty());
}

#[test]
fn funding_applies_to_balance_and_appends_log() {
    let mut s = sim(100_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(100),
        PerpPrice::new(62_600),
        Cents::ZERO,
    )
    .unwrap();
    // Long + positive rate pays: -(0.0001 x 6.26 x 100) = -$0.0626 ->
    // floor -> -7c (same vector as the T5.B2 core test).
    let acc = s
        .apply_funding(
            &mkt("KXBTCPERP"),
            dec("0.0001"),
            PerpPrice::new(62_600),
            ts("2026-06-12T12:00:00.000Z"),
        )
        .unwrap()
        .expect("held position must accrue");
    assert_eq!(acc.amount, Cents::new(-7));
    assert_eq!(s.balance(), Cents::new(99_993));
    assert_eq!(s.funding_log().len(), 1);
    assert_eq!(s.funding_log()[0].position_qty, Contracts::new(100));
}

#[test]
fn flat_position_accrues_nothing_and_logs_nothing() {
    let mut s = sim(100_000);
    let none = s
        .apply_funding(
            &mkt("KXBTCPERP"),
            dec("0.0001"),
            PerpPrice::new(62_600),
            ts("2026-06-12T12:00:00.000Z"),
        )
        .unwrap();
    assert!(none.is_none());
    assert_eq!(s.balance(), Cents::new(100_000));
    assert!(s.funding_log().is_empty());
}

// ---- account view + liquidation simulation ----

#[test]
fn account_view_composes_positions_at_conservative_marks() {
    let mut s = sim(10_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(100),
        PerpPrice::new(62_600),
        Cents::ZERO,
    )
    .unwrap();
    let view = s.account_view(&marks_at(60_000), Cents::ZERO).unwrap();
    // uPnL = -2,600 tt x 100 = -26,000 tt... = -2,600c.
    assert_eq!(view.unrealized, Cents::new(-2_600));
    assert_eq!(view.equity, Cents::new(7_400));
}

#[test]
fn healthy_account_does_not_liquidate() {
    let mut s = sim(10_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(100),
        PerpPrice::new(62_600),
        Cents::ZERO,
    )
    .unwrap();
    // At entry mark: equity 10,000; notional 62,600 -> MM 3,130. Safe.
    let event = s.check_liquidation(&marks_at(62_600), Cents::ZERO).unwrap();
    assert!(event.is_none());
    assert!(s.position(&mkt("KXBTCPERP")).is_some());
}

#[test]
fn equity_below_maintenance_liquidates_everything_at_penalty() {
    // Balance 10,000c, long 100 @ 62,600. Mark drops to 53,000:
    // uPnL = -9,600c -> equity 400c; notional 53,000c -> MM 2,650c.
    // 400 < 2,650 -> LIQUIDATION. Penalty 100 bps against us: close at
    // 53,000 - 530 = 52,470 -> realized = -10,130c -> balance -130c
    // (negative balances are modeled, not clamped).
    let mut s = MarginSim::new(Cents::new(10_000), config(100)).unwrap();
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(100),
        PerpPrice::new(62_600),
        Cents::ZERO,
    )
    .unwrap();
    let event = s
        .check_liquidation(&marks_at(53_000), Cents::ZERO)
        .unwrap()
        .expect("must liquidate below maintenance");
    assert_eq!(event.closed.len(), 1);
    assert_eq!(event.closed[0].0, mkt("KXBTCPERP"));
    assert_eq!(event.closed[0].1.qty, Contracts::new(100));
    assert_eq!(event.balance_after, Cents::new(-130));
    assert_eq!(s.balance(), Cents::new(-130));
    assert!(s.position(&mkt("KXBTCPERP")).is_none());
}

#[test]
fn short_liquidation_penalty_is_against_us_upward() {
    // Short 100 @ 62,600, balance 4,000c. Mark rises to 66,000:
    // uPnL = -3,400c -> equity 600c; notional 66,000c -> MM 3,300c.
    // 600 < 3,300 -> liquidate. Penalty 100 bps: close at 66,000 + 660 =
    // 66,660 -> realized = (62,600 - 66,660) x 100 = -4,060c.
    let mut s = MarginSim::new(Cents::new(4_000), config(100)).unwrap();
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Sell,
        Contracts::new(100),
        PerpPrice::new(62_600),
        Cents::ZERO,
    )
    .unwrap();
    let event = s
        .check_liquidation(&marks_at(66_000), Cents::ZERO)
        .unwrap()
        .expect("must liquidate");
    assert_eq!(event.balance_after, Cents::new(-60)); // 4,000 - 4,060
}

#[test]
fn unbounded_curve_at_check_is_error_not_guess() {
    // "A liquidation under-modeled = test failure, not surprise": a held
    // notional beyond the last curve tier cannot be margin-checked.
    let mut s = sim(100_000_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(9_000),
        PerpPrice::new(62_600),
        Cents::ZERO,
    )
    .unwrap();
    // notional = 9,000 x 626c = 5,634,000c > last tier 5,000,000.
    let result = s.check_liquidation(&marks_at(62_600), Cents::ZERO);
    assert!(result.is_err());
}

#[test]
fn missing_mark_for_held_position_is_error() {
    let mut s = sim(100_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(100),
        PerpPrice::new(62_600),
        Cents::ZERO,
    )
    .unwrap();
    let empty: BTreeMap<MarketId, PerpMarks> = BTreeMap::new();
    assert!(s.account_view(&empty, Cents::ZERO).is_err());
    assert!(s.check_liquidation(&empty, Cents::ZERO).is_err());
}

#[test]
fn config_validation_rejects_bad_shapes() {
    // Multiplier below 100 weakens the venue's own requirement.
    let mut cfg = config(0);
    cfg.mm_multiplier_pct = 99;
    assert!(MarginSim::new(Cents::new(1_000), cfg).is_err());

    // Negative penalty would model liquidation as a FAVOR.
    let mut cfg = config(0);
    cfg.liquidation_penalty_bps = -1;
    assert!(MarginSim::new(Cents::new(1_000), cfg).is_err());

    // Empty curve for a configured market.
    let mut cfg = config(0);
    cfg.curves
        .insert(mkt("KXETHPERP"), RiskCurve { tiers: vec![] });
    assert!(MarginSim::new(Cents::new(1_000), cfg).is_err());

    // Fill on a market with NO curve fails closed (cannot be margin-checked).
    let mut s = sim(100_000);
    assert!(s
        .apply_fill(
            &mkt("KXETHPERP"),
            Action::Buy,
            Contracts::new(1),
            PerpPrice::new(1_000),
            Cents::ZERO,
        )
        .is_err());
}

// ---- properties ----

proptest! {
    /// Open then fully close at arbitrary prices: realized PnL matches the
    /// i128 floor reference and the balance is exactly initial + realized
    /// - fees. No panics anywhere in range.
    #[test]
    fn prop_open_close_accounting_matches_reference(
        qty in 1i64..2_000,
        entry in 1i64..200_000,
        exit in 1i64..200_000,
        fee_open in 0i64..1_000,
        fee_close in 0i64..1_000,
        long in any::<bool>(),
    ) {
        let mut s = MarginSim::new(Cents::new(10_000_000), {
            let mut curves = BTreeMap::new();
            curves.insert(
                mkt("KXBTCPERP"),
                RiskCurve { tiers: vec![(Cents::new(i64::MAX / 4), 500)] },
            );
            MarginSimConfig {
                mm_multiplier_pct: 100,
                liquidation_penalty_bps: 0,
                curves,
            }
        }).unwrap();
        let (open, close) = if long {
            (Action::Buy, Action::Sell)
        } else {
            (Action::Sell, Action::Buy)
        };
        s.apply_fill(&mkt("KXBTCPERP"), open, Contracts::new(qty),
            PerpPrice::new(entry), Cents::new(fee_open)).unwrap();
        let out = s.apply_fill(&mkt("KXBTCPERP"), close, Contracts::new(qty),
            PerpPrice::new(exit), Cents::new(fee_close)).unwrap();
        let sign: i128 = if long { 1 } else { -1 };
        let exact_tt = sign * (i128::from(exit) - i128::from(entry)) * i128::from(qty);
        let reference = exact_tt.div_euclid(100);
        prop_assert_eq!(i128::from(out.realized_pnl.raw()), reference);
        let expected_balance =
            10_000_000 + reference - i128::from(fee_open) - i128::from(fee_close);
        prop_assert_eq!(i128::from(s.balance().raw()), expected_balance);
        prop_assert!(s.position(&mkt("KXBTCPERP")).is_none());
    }
}

// ---- gate-fix F3 (track-c-perp-gates-gate-2026-06-12): funding cap ----

#[test]
fn funding_rate_beyond_venue_cap_is_error_not_clamp() {
    // Research §4: the venue clamps funding to +/-2% per window BEFORE
    // reporting. A larger |rate| in our inputs is corrupt data — error,
    // never a silent clamp (clamping would alter venue-reported data).
    let mut s = sim(100_000);
    s.apply_fill(
        &mkt("KXBTCPERP"),
        Action::Buy,
        Contracts::new(100),
        PerpPrice::new(62_600),
        Cents::ZERO,
    )
    .unwrap();
    // Exactly at the cap: accepted, both signs.
    assert!(s
        .apply_funding(
            &mkt("KXBTCPERP"),
            dec("0.02"),
            PerpPrice::new(62_600),
            ts("2026-06-12T12:00:00.000Z")
        )
        .is_ok());
    assert!(s
        .apply_funding(
            &mkt("KXBTCPERP"),
            dec("-0.02"),
            PerpPrice::new(62_600),
            ts("2026-06-12T20:00:00.000Z")
        )
        .is_ok());
    // Beyond the cap: error, balance untouched.
    let before = s.balance();
    assert!(s
        .apply_funding(
            &mkt("KXBTCPERP"),
            dec("0.020001"),
            PerpPrice::new(62_600),
            ts("2026-06-13T04:00:00.000Z")
        )
        .is_err());
    assert!(s
        .apply_funding(
            &mkt("KXBTCPERP"),
            dec("-0.21"),
            PerpPrice::new(62_600),
            ts("2026-06-13T04:00:00.000Z")
        )
        .is_err());
    assert_eq!(s.balance(), before);
}

// ---- gate-fix F1 follow-on (BINDING for B4): recorded-curve converter ----

#[test]
fn risk_curve_from_recorded_leverage_estimates_shape() {
    // The RECORDED fixture is the ground truth: markets__single.json
    // leverage_estimates {"1000": 5.899, "10000": 5.899, "100000": 5.899,
    // "1000000": 5.8143} -> tiers in CENTS with mm_bps = ceil(10000/L)
    // (rounding UP = more margin = conservative).
    let raw = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/kinetics-perps/markets__single.json"),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let estimates: BTreeMap<String, f64> =
        serde_json::from_value(v["market"]["leverage_estimates"].clone()).unwrap();
    let curve = RiskCurve::from_leverage_estimates(&estimates).unwrap();
    assert_eq!(
        curve.tiers,
        vec![
            (Cents::new(100_000), 1_696),     // $1k tier, ceil(10000/5.899)
            (Cents::new(1_000_000), 1_696),   // $10k
            (Cents::new(10_000_000), 1_696),  // $100k
            (Cents::new(100_000_000), 1_720), // $1M, ceil(10000/5.8143)
        ]
    );
    // The converted curve drives the sim directly.
    let mut curves = BTreeMap::new();
    curves.insert(mkt("KXBTCPERP"), curve);
    assert!(MarginSim::new(
        Cents::new(1_000),
        MarginSimConfig {
            mm_multiplier_pct: 130,
            liquidation_penalty_bps: 100,
            curves,
        }
    )
    .is_ok());
}

#[test]
fn risk_curve_converter_sorts_numerically_and_fails_closed() {
    // Lexicographic key order is NOT numeric ("9" > "10000"): the
    // converter must sort by parsed value.
    let mut estimates = BTreeMap::new();
    estimates.insert("9".to_string(), 2.0);
    estimates.insert("10000".to_string(), 1.5);
    let curve = RiskCurve::from_leverage_estimates(&estimates).unwrap();
    assert_eq!(
        curve.tiers,
        vec![(Cents::new(900), 5_000), (Cents::new(1_000_000), 6_667)]
    );

    // Leverage below 1 implies mm > 100% of notional: outside the curve
    // domain, fail closed.
    let mut bad = BTreeMap::new();
    bad.insert("1000".to_string(), 0.5);
    assert!(RiskCurve::from_leverage_estimates(&bad).is_err());

    // Non-numeric tier key: fail closed.
    let mut bad = BTreeMap::new();
    bad.insert("not-a-number".to_string(), 5.0);
    assert!(RiskCurve::from_leverage_estimates(&bad).is_err());

    // Empty input: no curve, no bound.
    assert!(RiskCurve::from_leverage_estimates(&BTreeMap::new()).is_err());

    // Non-finite leverage: fail closed.
    let mut bad = BTreeMap::new();
    bad.insert("1000".to_string(), f64::NAN);
    assert!(RiskCurve::from_leverage_estimates(&bad).is_err());
}
