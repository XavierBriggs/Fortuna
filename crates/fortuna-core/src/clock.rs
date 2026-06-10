//! Injected time. `SystemTime::now()`/`Utc::now()` outside this module is a defect.
//!
//! `UtcTimestamp` is UTC ISO8601 with FIXED millisecond precision: finer
//! precision is truncated at construction so the in-memory value always equals
//! its serialized form (byte-identical replay; ULIDs are millisecond-granular
//! anyway). `SimClock` is deterministic, shareable, and monotone
//! non-decreasing; `RealClock` is the single permitted wall-time read.

use chrono::{DateTime, TimeZone, Timelike, Utc};
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

    pub fn checked_add_millis(self, millis: i64) -> Result<Self, ClockError> {
        let sum = self
            .epoch_millis()
            .checked_add(millis)
            .ok_or(ClockError::Overflow)?;
        Self::from_epoch_millis(sum)
    }
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
