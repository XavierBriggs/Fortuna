//! Injected time. `SystemTime::now()`/`Utc::now()` outside this module is a defect.
//!
//! `UtcTimestamp` is UTC ISO8601 with FIXED millisecond precision: finer
//! precision is truncated at construction so the in-memory value always equals
//! its serialized form (byte-identical replay; ULIDs are millisecond-granular
//! anyway). `SimClock` is deterministic, shareable, and monotone
//! non-decreasing; `RealClock` is the single permitted wall-time read.

use chrono::{DateTime, NaiveDate, TimeZone, Timelike, Utc};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::sync::{Mutex, PoisonError};
use thiserror::Error;

/// Errors from timestamp construction and sim-clock control.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ClockError {
    /// Millisecond value outside chrono's representable datetime range.
    #[error("timestamp {millis}ms is outside the representable datetime range")]
    OutOfRange { millis: i64 },
    /// Input string is not parseable ISO8601/RFC3339.
    #[error("cannot parse {input:?} as an ISO8601 timestamp: {reason}")]
    Parse { input: String, reason: String },
    /// Determinism guard: sim time never moves backwards.
    #[error("sim clock cannot move backwards: current {current}, requested {requested}")]
    BackwardsTime { current: String, requested: String },
    /// Timestamp arithmetic overflowed.
    #[error("timestamp arithmetic overflow")]
    Overflow,
}

/// UTC timestamp, millisecond precision, fixed-format ISO8601 serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcTimestamp(DateTime<Utc>);

const ISO8601_MS: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

/// Zero out sub-millisecond precision. Truncation (floor on the time-of-day
/// nanos) keeps the value exactly representable as epoch milliseconds.
fn truncate_to_ms(dt: DateTime<Utc>) -> DateTime<Utc> {
    let trunc = (dt.nanosecond() / 1_000_000) * 1_000_000;
    // with_nanosecond only fails for out-of-range inputs; trunc is derived
    // from a valid value, so fall back to the original (never fires).
    dt.with_nanosecond(trunc).unwrap_or(dt)
}

impl UtcTimestamp {
    pub fn from_epoch_millis(millis: i64) -> Result<Self, ClockError> {
        Utc.timestamp_millis_opt(millis)
            .single()
            .map(UtcTimestamp)
            .ok_or(ClockError::OutOfRange { millis })
    }

    pub fn epoch_millis(self) -> i64 {
        self.0.timestamp_millis()
    }

    /// Fixed-format `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    pub fn to_iso8601(self) -> String {
        self.0.format(ISO8601_MS).to_string()
    }

    /// Parse RFC3339/ISO8601; offsets are normalized to UTC; sub-millisecond
    /// precision is truncated.
    pub fn parse_iso8601(input: &str) -> Result<Self, ClockError> {
        let dt = DateTime::parse_from_rfc3339(input).map_err(|e| ClockError::Parse {
            input: input.to_string(),
            reason: e.to_string(),
        })?;
        Ok(UtcTimestamp(truncate_to_ms(dt.with_timezone(&Utc))))
    }

    /// Lenient parse for MODEL-EMITTED horizon strings: a full RFC3339 datetime,
    /// OR a bare calendar date `YYYY-MM-DD` (which an LLM routinely emits for an
    /// event horizon) normalized to `00:00:00.000Z` UTC. It also accepts the
    /// observed short model phrase `resolved YYYY-MM-DD` by extracting its one
    /// date token. This is OPT-IN — the strict `parse_iso8601` and the default
    /// `Deserialize` stay strict so venue timestamps, audit times, etc. never
    /// silently widen; only cognition horizon fields that face the model use
    /// this. Anything else is an error.
    pub fn parse_iso8601_or_date(input: &str) -> Result<Self, ClockError> {
        if let Ok(ts) = Self::parse_iso8601(input) {
            return Ok(ts);
        }
        let t = input.trim();
        if let Some(date) = normalized_model_horizon_date(t) {
            return Self::parse_iso8601(&format!("{date}T00:00:00.000Z"));
        }
        Err(ClockError::Parse {
            input: input.to_string(),
            reason: "neither an RFC3339 datetime nor a YYYY-MM-DD model horizon".to_string(),
        })
    }

    pub fn checked_add_millis(self, millis: i64) -> Result<Self, ClockError> {
        let sum = self
            .epoch_millis()
            .checked_add(millis)
            .ok_or(ClockError::Overflow)?;
        Self::from_epoch_millis(sum)
    }
}

fn normalized_model_horizon_date(input: &str) -> Option<String> {
    if is_valid_date_token(input) {
        return Some(input.to_string());
    }
    if input.len() > 32 {
        return None;
    }
    let lower = input.to_ascii_lowercase();
    if !matches!(
        lower.split_ascii_whitespace().next(),
        Some("resolved" | "resolve" | "by" | "on" | "until")
    ) {
        return None;
    }
    let mut dates = input
        .split_ascii_whitespace()
        .filter(|token| is_valid_date_token(token));
    let only = dates.next()?;
    if dates.next().is_some() {
        return None;
    }
    Some(only.to_string())
}

fn is_valid_date_token(input: &str) -> bool {
    let b = input.as_bytes();
    if !(input.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[8..10].iter().all(u8::is_ascii_digit))
    {
        return false;
    }
    NaiveDate::parse_from_str(input, "%Y-%m-%d").is_ok()
}

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_iso8601())
    }
}

impl Serialize for UtcTimestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_iso8601())
    }
}

impl<'de> Deserialize<'de> for UtcTimestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        UtcTimestamp::parse_iso8601(&s).map_err(de::Error::custom)
    }
}

/// The injected time source. Components never read wall time directly.
pub trait Clock: Send + Sync {
    fn now(&self) -> UtcTimestamp;
}

/// Wall-clock time. The ONLY permitted wall-time read in the workspace.
pub struct RealClock;

impl Clock for RealClock {
    fn now(&self) -> UtcTimestamp {
        UtcTimestamp(truncate_to_ms(Utc::now()))
    }
}

/// Deterministic, manually-advanced clock for tests, DST, and replay.
/// Monotone non-decreasing by construction.
pub struct SimClock {
    current: Mutex<UtcTimestamp>,
}

impl SimClock {
    pub fn new(start: UtcTimestamp) -> Self {
        SimClock {
            current: Mutex::new(start),
        }
    }

    fn read(&self) -> UtcTimestamp {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Advance forward by `millis`. Zero is legal.
    pub fn advance_millis(&self, millis: u64) -> Result<(), ClockError> {
        let step = i64::try_from(millis).map_err(|_| ClockError::Overflow)?;
        let mut guard = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        *guard = guard.checked_add_millis(step)?;
        Ok(())
    }

    /// Jump to an absolute instant. Backwards movement is a determinism
    /// violation and is rejected; the clock is unchanged on error.
    pub fn set(&self, to: UtcTimestamp) -> Result<(), ClockError> {
        let mut guard = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        if to < *guard {
            return Err(ClockError::BackwardsTime {
                current: guard.to_iso8601(),
                requested: to.to_iso8601(),
            });
        }
        *guard = to;
        Ok(())
    }
}

impl Clock for SimClock {
    fn now(&self) -> UtcTimestamp {
        self.read()
    }
}
