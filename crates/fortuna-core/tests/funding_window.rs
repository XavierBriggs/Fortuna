//! T5.B7 foundation tests: the deterministic funding-forecast kernel
//! (`FundingWindow` + `finalize_funding_rate`). Written from research §4
//! and spec 5.15 BEFORE implementation.
//!
//! Contract under test (research §4, "Calculation"/"Cap"/"Zero threshold"):
//! - The venue computes funding as the time-weighted average of 1-minute
//!   candlestick premiums over the window's 480 candles, continuously
//!   estimated over [last_funding_time, now) and finalized at
//!   next_funding_time with a +/-2% clamp and a 0.01% zero threshold.
//! - `FundingWindow` reproduces that estimate deterministically from
//!   observed premiums: equal-weight mean (equal 1-minute candles =>
//!   time-weighted == arithmetic mean), with the premium per candle taken
//!   as INPUT (the exact premium-index formula is venue-unpublished,
//!   research §11 — never re-derived, same discipline as FundingAccrual).
//! - `finalize_funding_rate` is the venue's finalization: clamp to
//!   +/-2%, then zero if |rate| < 0.01%.
//! - `forecast_final` is the deterministic final-rate forecast under the
//!   stationary-mean assumption (remaining candles carry the running
//!   average) = finalize(running_estimate). Strategy-level extrapolation
//!   layers on top; the kernel provides the reconcilable baseline.

use fortuna_core::perp::{
    finalize_funding_rate, FundingWindow, PerpError, FUNDING_CANDLES_PER_WINDOW,
};
use rust_decimal::Decimal;
use std::str::FromStr;

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

// ---- finalize_funding_rate: the venue finalization vectors ----

#[test]
fn finalize_passes_through_normal_rates() {
    // Within (0.0001, 0.02): kept verbatim, both signs.
    assert_eq!(finalize_funding_rate(dec("0.0003")), dec("0.0003"));
    assert_eq!(finalize_funding_rate(dec("-0.0040")), dec("-0.0040"));
    // A real recorded rate (research §4: KXBCHPERP 2026-06-11T04:00Z).
    let real = dec("-0.0003971378687289");
    assert_eq!(finalize_funding_rate(real), real);
}

#[test]
fn finalize_clamps_to_two_percent_both_signs() {
    assert_eq!(finalize_funding_rate(dec("0.05")), dec("0.02"));
    assert_eq!(finalize_funding_rate(dec("-0.05")), dec("-0.02"));
    // Exactly at the clamp is kept; just beyond is clamped.
    assert_eq!(finalize_funding_rate(dec("0.02")), dec("0.02"));
    assert_eq!(finalize_funding_rate(dec("0.020001")), dec("0.02"));
    assert_eq!(finalize_funding_rate(dec("-0.020001")), dec("-0.02"));
}

#[test]
fn finalize_zeroes_below_the_threshold_only() {
    // "If the absolute value of the rate is below 0.01%": strictly below.
    assert_eq!(finalize_funding_rate(dec("0.00005")), Decimal::ZERO);
    assert_eq!(finalize_funding_rate(dec("-0.00009")), Decimal::ZERO);
    assert_eq!(finalize_funding_rate(Decimal::ZERO), Decimal::ZERO);
    // EXACTLY at the threshold is NOT below it: kept.
    assert_eq!(finalize_funding_rate(dec("0.0001")), dec("0.0001"));
    assert_eq!(finalize_funding_rate(dec("-0.0001")), dec("-0.0001"));
    // Just above the threshold: kept.
    assert_eq!(finalize_funding_rate(dec("0.00011")), dec("0.00011"));
}

// ---- FundingWindow accumulation ----

#[test]
fn empty_window_has_no_estimate() {
    let w = FundingWindow::new();
    assert_eq!(w.observed(), 0);
    assert_eq!(w.remaining(), FUNDING_CANDLES_PER_WINDOW);
    assert_eq!(w.running_estimate().unwrap(), None);
    assert_eq!(w.forecast_final().unwrap(), None);
}

#[test]
fn single_premium_estimate_equals_that_premium() {
    let mut w = FundingWindow::new();
    w.observe(dec("0.0003")).unwrap();
    assert_eq!(w.observed(), 1);
    assert_eq!(w.remaining(), FUNDING_CANDLES_PER_WINDOW - 1);
    assert_eq!(w.running_estimate().unwrap(), Some(dec("0.0003")));
    assert_eq!(w.forecast_final().unwrap(), Some(dec("0.0003")));
}

#[test]
fn running_estimate_is_the_equal_weight_mean() {
    let mut w = FundingWindow::new();
    for p in ["0.0001", "0.0003", "0.0008"] {
        w.observe(dec(p)).unwrap();
    }
    // mean = (1 + 3 + 8)e-4 / 3 = 4e-4.
    assert_eq!(w.running_estimate().unwrap(), Some(dec("0.0004")));
    assert_eq!(w.forecast_final().unwrap(), Some(dec("0.0004")));
}

#[test]
fn negative_premiums_average_signed() {
    let mut w = FundingWindow::new();
    for p in ["-0.0010", "-0.0006"] {
        w.observe(dec(p)).unwrap();
    }
    assert_eq!(w.running_estimate().unwrap(), Some(dec("-0.0008")));
    assert_eq!(w.forecast_final().unwrap(), Some(dec("-0.0008")));
}

#[test]
fn forecast_applies_the_zero_threshold_to_a_small_mean() {
    // Mean below 0.01% in absolute value: the running ESTIMATE is the raw
    // mean, but the FORECAST (finalized) zeroes it (no payment).
    let mut w = FundingWindow::new();
    w.observe(dec("0.00004")).unwrap();
    w.observe(dec("0.00006")).unwrap();
    assert_eq!(w.running_estimate().unwrap(), Some(dec("0.00005")));
    assert_eq!(w.forecast_final().unwrap(), Some(Decimal::ZERO));
}

#[test]
fn forecast_clamps_a_runaway_mean() {
    // A window whose premiums average beyond the +/-2% cap forecasts the
    // clamped rate (the venue clamps at finalization), while the running
    // estimate stays the raw mean.
    let mut w = FundingWindow::new();
    w.observe(dec("0.03")).unwrap();
    w.observe(dec("0.05")).unwrap();
    assert_eq!(w.running_estimate().unwrap(), Some(dec("0.04")));
    assert_eq!(w.forecast_final().unwrap(), Some(dec("0.02")));
}

#[test]
fn window_rejects_more_than_480_candles() {
    let mut w = FundingWindow::new();
    for _ in 0..FUNDING_CANDLES_PER_WINDOW {
        w.observe(dec("0.0002")).unwrap();
    }
    assert_eq!(w.observed(), FUNDING_CANDLES_PER_WINDOW);
    assert_eq!(w.remaining(), 0);
    // The 481st candle belongs to the next window: the caller must roll.
    assert!(matches!(
        w.observe(dec("0.0002")),
        Err(PerpError::FundingWindowOverfull)
    ));
    // A full window still yields its estimate.
    assert_eq!(w.running_estimate().unwrap(), Some(dec("0.0002")));
    assert_eq!(w.forecast_final().unwrap(), Some(dec("0.0002")));
}

#[test]
fn full_window_mean_matches_a_known_average() {
    // 240 candles at +0.0002 and 240 at -0.0002 average to exactly 0,
    // then forecast zeroes it.
    let mut w = FundingWindow::new();
    for i in 0..FUNDING_CANDLES_PER_WINDOW {
        let p = if i < FUNDING_CANDLES_PER_WINDOW / 2 {
            dec("0.0002")
        } else {
            dec("-0.0002")
        };
        w.observe(p).unwrap();
    }
    assert_eq!(w.running_estimate().unwrap(), Some(Decimal::ZERO));
    assert_eq!(w.forecast_final().unwrap(), Some(Decimal::ZERO));
}

#[test]
fn estimate_moves_as_candles_accumulate_deterministically() {
    // The estimate continues to move as data accumulates (research §4);
    // re-running the same observation sequence is byte-identical.
    let run = || {
        let mut w = FundingWindow::new();
        let mut trace = Vec::new();
        for p in ["0.0010", "0.0000", "-0.0004", "0.0006"] {
            w.observe(dec(p)).unwrap();
            trace.push(w.running_estimate().unwrap().unwrap());
        }
        trace
    };
    let a = run();
    let b = run();
    assert_eq!(a, b);
    // (10 + 0 - 4 + 6)e-4 / 4 = 3e-4.
    assert_eq!(*a.last().unwrap(), dec("0.0003"));
}

#[test]
fn window_is_default_constructible_empty() {
    let w = FundingWindow::default();
    assert_eq!(w.observed(), 0);
    assert_eq!(w.running_estimate().unwrap(), None);
}
