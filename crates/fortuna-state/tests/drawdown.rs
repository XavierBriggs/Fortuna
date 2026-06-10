//! T0.7 tests: drawdown monitor (I2 support). Written BEFORE implementation.
//!
//! Contract under test: day = 00:00 UTC. day_start_equity baselines on first
//! observation and on each UTC day roll. loss = day_start_equity -
//! current_equity (checked). Breach ONLY when limit > 0 AND loss >= limit
//! (a non-positive limit disables the monitor). Once breached, the verdict
//! stays Breach for the rest of the UTC day even if equity recovers
//! (sticky); the day roll clears the stickiness. The gates' halt flag (I2,
//! human re-arm) is the real lock; this stickiness is defense in depth.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::money::Cents;
use fortuna_state::{DrawdownMonitor, DrawdownVerdict};

fn ts(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(s).unwrap()
}

fn c(v: i64) -> Cents {
    Cents::new(v)
}

#[test]
fn same_day_loss_below_limit_is_ok() {
    let mut m = DrawdownMonitor::new(c(1_000));
    m.roll_day_if_needed(ts("2026-06-09T08:00:00.000Z"), c(100_000));
    let v = m.check(ts("2026-06-09T09:00:00.000Z"), c(99_001)).unwrap();
    assert_eq!(v, DrawdownVerdict::Ok);
}

#[test]
fn breach_at_exact_limit() {
    let mut m = DrawdownMonitor::new(c(1_000));
    m.roll_day_if_needed(ts("2026-06-09T08:00:00.000Z"), c(100_000));
    let v = m.check(ts("2026-06-09T09:00:00.000Z"), c(99_000)).unwrap();
    assert_eq!(
        v,
        DrawdownVerdict::Breach {
            loss: c(1_000),
            limit: c(1_000)
        }
    );
}

#[test]
fn breach_above_limit() {
    let mut m = DrawdownMonitor::new(c(1_000));
    m.roll_day_if_needed(ts("2026-06-09T08:00:00.000Z"), c(100_000));
    let v = m.check(ts("2026-06-09T09:00:00.000Z"), c(98_500)).unwrap();
    assert_eq!(
        v,
        DrawdownVerdict::Breach {
            loss: c(1_500),
            limit: c(1_000)
        }
    );
}

#[test]
fn breach_is_sticky_for_the_rest_of_the_day_even_after_recovery() {
    let mut m = DrawdownMonitor::new(c(1_000));
    m.roll_day_if_needed(ts("2026-06-09T08:00:00.000Z"), c(100_000));
    let v = m.check(ts("2026-06-09T09:00:00.000Z"), c(98_000)).unwrap();
    assert!(matches!(v, DrawdownVerdict::Breach { .. }));

    // Full recovery, even a net gain: verdict stays Breach (loss reported
    // is the CURRENT loss, which may be negative after recovery).
    let v = m.check(ts("2026-06-09T15:00:00.000Z"), c(100_500)).unwrap();
    assert_eq!(
        v,
        DrawdownVerdict::Breach {
            loss: c(-500),
            limit: c(1_000)
        }
    );
}

#[test]
fn day_roll_resets_baseline_and_stickiness() {
    let mut m = DrawdownMonitor::new(c(1_000));
    m.roll_day_if_needed(ts("2026-06-09T08:00:00.000Z"), c(100_000));
    let v = m.check(ts("2026-06-09T09:00:00.000Z"), c(98_000)).unwrap();
    assert!(matches!(v, DrawdownVerdict::Breach { .. }));

    // Next UTC day: baseline re-anchors at current equity, stickiness clears.
    let v = m.check(ts("2026-06-10T00:00:00.000Z"), c(98_200)).unwrap();
    assert_eq!(v, DrawdownVerdict::Ok);

    // And losses are now measured from the NEW baseline (98_200).
    let v = m.check(ts("2026-06-10T01:00:00.000Z"), c(97_100)).unwrap();
    assert_eq!(
        v,
        DrawdownVerdict::Breach {
            loss: c(1_100),
            limit: c(1_000)
        }
    );
}

#[test]
fn equity_gain_never_breaches() {
    let mut m = DrawdownMonitor::new(c(1));
    m.roll_day_if_needed(ts("2026-06-09T08:00:00.000Z"), c(100_000));
    let v = m.check(ts("2026-06-09T09:00:00.000Z"), c(150_000)).unwrap();
    assert_eq!(v, DrawdownVerdict::Ok);
}

#[test]
fn day_boundary_2359_is_same_day_0000_is_next_day() {
    // 23:59:59.999Z is still the same UTC day: baseline holds, loss breaches.
    let mut m = DrawdownMonitor::new(c(1_000));
    m.roll_day_if_needed(ts("2026-06-09T00:00:00.000Z"), c(100_000));
    let v = m.check(ts("2026-06-09T23:59:59.999Z"), c(98_000)).unwrap();
    assert!(matches!(v, DrawdownVerdict::Breach { .. }));

    // 00:00:00.000Z next day rolls: same equity reads as zero loss.
    let mut m = DrawdownMonitor::new(c(1_000));
    m.roll_day_if_needed(ts("2026-06-09T00:00:00.000Z"), c(100_000));
    let v = m.check(ts("2026-06-10T00:00:00.000Z"), c(98_000)).unwrap();
    assert_eq!(v, DrawdownVerdict::Ok);
}

#[test]
fn non_positive_limit_disables_the_monitor() {
    // Documented: limit must be > 0 to ever breach.
    let mut m = DrawdownMonitor::new(c(0));
    m.roll_day_if_needed(ts("2026-06-09T08:00:00.000Z"), c(100_000));
    let v = m.check(ts("2026-06-09T09:00:00.000Z"), c(1)).unwrap();
    assert_eq!(v, DrawdownVerdict::Ok);

    let mut m = DrawdownMonitor::new(c(-100));
    m.roll_day_if_needed(ts("2026-06-09T08:00:00.000Z"), c(100_000));
    let v = m.check(ts("2026-06-09T09:00:00.000Z"), c(1)).unwrap();
    assert_eq!(v, DrawdownVerdict::Ok);
}

#[test]
fn first_observation_via_check_baselines_at_current_equity() {
    // check() rolls internally; a fresh monitor's first check is loss zero.
    let mut m = DrawdownMonitor::new(c(1_000));
    let v = m.check(ts("2026-06-09T12:00:00.000Z"), c(42)).unwrap();
    assert_eq!(v, DrawdownVerdict::Ok);
}

#[test]
fn same_day_repeat_roll_does_not_rebaseline() {
    let mut m = DrawdownMonitor::new(c(1_000));
    m.roll_day_if_needed(ts("2026-06-09T08:00:00.000Z"), c(100_000));
    // A second roll call later the same day must NOT move the baseline
    // (otherwise intraday losses would be silently forgiven).
    m.roll_day_if_needed(ts("2026-06-09T09:00:00.000Z"), c(99_500));
    let v = m.check(ts("2026-06-09T10:00:00.000Z"), c(99_000)).unwrap();
    assert_eq!(
        v,
        DrawdownVerdict::Breach {
            loss: c(1_000),
            limit: c(1_000)
        }
    );
}

#[test]
fn intraday_checks_do_not_move_the_baseline() {
    let mut m = DrawdownMonitor::new(c(1_000));
    m.roll_day_if_needed(ts("2026-06-09T08:00:00.000Z"), c(100_000));
    // Drip losses in steps each below the limit; the cumulative loss from
    // the day baseline is what counts.
    assert_eq!(
        m.check(ts("2026-06-09T09:00:00.000Z"), c(99_600)).unwrap(),
        DrawdownVerdict::Ok
    );
    assert_eq!(
        m.check(ts("2026-06-09T10:00:00.000Z"), c(99_200)).unwrap(),
        DrawdownVerdict::Ok
    );
    let v = m.check(ts("2026-06-09T11:00:00.000Z"), c(99_000)).unwrap();
    assert_eq!(
        v,
        DrawdownVerdict::Breach {
            loss: c(1_000),
            limit: c(1_000)
        }
    );
}
