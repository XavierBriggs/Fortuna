//! T0.1 tests: `UtcTimestamp` and the `Clock` trait. Written from spec 5.1 +
//! conventions before implementation.
//!
//! Contract: all time comes from an injected `Clock`; `SystemTime::now()`/
//! `Utc::now()` outside the real impl is a defect. Timestamps are UTC ISO8601
//! with FIXED millisecond precision so that replay serialization is
//! byte-identical (ULIDs are millisecond-granular; finer precision is
//! truncated at construction so the in-memory value always equals its wire
//! form). The sim clock is deterministic, shareable, and never moves backwards.

use fortuna_core::clock::{Clock, ClockError, RealClock, SimClock, UtcTimestamp};
use std::sync::Arc;

fn ts(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(ms).unwrap()
}

// ---- UtcTimestamp ----

#[test]
fn epoch_zero_formats_as_fixed_precision_iso8601() {
    assert_eq!(ts(0).to_iso8601(), "1970-01-01T00:00:00.000Z");
}

#[test]
fn known_timestamp_formats_correctly() {
    // 1_700_000_000.123 s = 2023-11-14T22:13:20.123Z (verified externally).
    assert_eq!(
        ts(1_700_000_000_123).to_iso8601(),
        "2023-11-14T22:13:20.123Z"
    );
}

#[test]
fn epoch_millis_round_trip() {
    assert_eq!(ts(1_700_000_000_123).epoch_millis(), 1_700_000_000_123);
    assert_eq!(ts(-1).epoch_millis(), -1); // pre-1970 is representable
}

#[test]
fn parse_round_trips_own_format() {
    let t = ts(1_700_000_000_123);
    assert_eq!(UtcTimestamp::parse_iso8601(&t.to_iso8601()), Ok(t));
}

#[test]
fn parse_normalizes_offsets_to_utc() {
    let utc = UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap();
    let offset = UtcTimestamp::parse_iso8601("2026-06-09T14:00:00.000+02:00").unwrap();
    assert_eq!(utc, offset);
    assert_eq!(offset.to_iso8601(), "2026-06-09T12:00:00.000Z");
}

#[test]
fn parse_truncates_sub_millisecond_precision() {
    let t = UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.123999Z").unwrap();
    assert_eq!(t.to_iso8601(), "2026-06-09T12:00:00.123Z");
}

#[test]
fn parse_rejects_garbage() {
    assert!(UtcTimestamp::parse_iso8601("not a timestamp").is_err());
    assert!(UtcTimestamp::parse_iso8601("2026-13-45T99:99:99Z").is_err());
    assert!(UtcTimestamp::parse_iso8601("").is_err());
}

#[test]
fn from_epoch_millis_out_of_chrono_range_is_error() {
    assert!(matches!(
        UtcTimestamp::from_epoch_millis(i64::MAX),
        Err(ClockError::OutOfRange { .. })
    ));
    assert!(matches!(
        UtcTimestamp::from_epoch_millis(i64::MIN),
        Err(ClockError::OutOfRange { .. })
    ));
}

#[test]
fn ordering_follows_time() {
    assert!(ts(1) < ts(2));
    assert!(ts(-1) < ts(0));
}

#[test]
fn checked_add_millis_moves_forward_and_detects_overflow() {
    assert_eq!(ts(1000).checked_add_millis(234), Ok(ts(1234)));
    assert_eq!(ts(1000).checked_add_millis(-1000), Ok(ts(0)));
    assert!(ts(0).checked_add_millis(i64::MAX).is_err());
}

#[test]
fn serde_round_trips_as_iso8601_string() {
    let t = ts(1_700_000_000_123);
    let json = serde_json::to_string(&t).unwrap();
    assert_eq!(json, "\"2023-11-14T22:13:20.123Z\"");
    let back: UtcTimestamp = serde_json::from_str(&json).unwrap();
    assert_eq!(back, t);
}

// ---- SimClock ----

#[test]
fn sim_clock_starts_at_given_time_and_is_stable() {
    let c = SimClock::new(ts(5000));
    assert_eq!(c.now(), ts(5000));
    assert_eq!(c.now(), ts(5000)); // reading does not advance
}

#[test]
fn sim_clock_advances_forward() {
    let c = SimClock::new(ts(5000));
    c.advance_millis(250).unwrap();
    assert_eq!(c.now(), ts(5250));
    c.advance_millis(0).unwrap(); // zero advance is legal
    assert_eq!(c.now(), ts(5250));
}

#[test]
fn sim_clock_set_forward_ok_backwards_rejected() {
    let c = SimClock::new(ts(5000));
    c.set(ts(6000)).unwrap();
    assert_eq!(c.now(), ts(6000));
    // Determinism guard: sim time is monotone non-decreasing.
    assert!(matches!(
        c.set(ts(5999)),
        Err(ClockError::BackwardsTime { .. })
    ));
    assert_eq!(c.now(), ts(6000)); // unchanged after the rejected set
    c.set(ts(6000)).unwrap(); // setting to the same instant is legal
}

#[test]
fn sim_clock_advance_overflow_is_error_and_leaves_time_unchanged() {
    let c = SimClock::new(ts(0));
    assert!(c.advance_millis(i64::MAX as u64 + 5).is_err());
    assert_eq!(c.now(), ts(0));
    // Overflow inside the timestamp range check, not just the u64->i64 cast.
    assert!(c.advance_millis(i64::MAX as u64 - 5).is_err());
    assert_eq!(c.now(), ts(0));
}

#[test]
fn sim_clock_is_shareable_behind_dyn_clock() {
    let sim = Arc::new(SimClock::new(ts(100)));
    let as_clock: Arc<dyn Clock> = sim.clone();
    assert_eq!(as_clock.now(), ts(100));
    sim.advance_millis(50).unwrap(); // advance via the concrete handle...
    assert_eq!(as_clock.now(), ts(150)); // ...is visible through the trait handle
}

// ---- RealClock ----

#[test]
fn real_clock_returns_plausible_utc_now() {
    let now = RealClock.now();
    // Sanity band: after 2026-01-01 and before 2100-01-01.
    let lower = UtcTimestamp::parse_iso8601("2026-01-01T00:00:00.000Z").unwrap();
    let upper = UtcTimestamp::parse_iso8601("2100-01-01T00:00:00.000Z").unwrap();
    assert!(now > lower && now < upper);
}
