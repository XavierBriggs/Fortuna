//! Deterministic ULID generation and typed ids.
//!
//! IDs are ULIDs (conventions). Generation is deterministic under a seed:
//! same seed + same clock readings produce a byte-identical id sequence on
//! any platform. The PRNG is an in-house SplitMix64 pinned by published test
//! vectors, NOT a `rand` generator, because `rand`'s small RNGs make no
//! cross-version/cross-platform stability promise and id determinism is
//! load-bearing for replay.
//!
//! Monotonicity: ids strictly increase, even within one millisecond
//! (random-part increment per the ULID spec) and even if the clock is read
//! backwards (clamp to the high-water-mark millisecond, never duplicate).

use crate::clock::UtcTimestamp;
use thiserror::Error;
pub use ulid::Ulid;

/// Errors from id generation and parsing.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum IdError {
    /// ULID timestamps are 48-bit milliseconds since the Unix epoch.
    #[error("timestamp {millis}ms is not representable in a ULID (must be in [0, 2^48))")]
    TimestampOutOfRange { millis: i64 },
    /// 2^80 ids were generated in one millisecond (practically unreachable;
    /// erroring is the conservative alternative to silently wrapping).
    #[error("monotonic ULID random space exhausted within one millisecond")]
    RandomExhausted,
    /// Input is not a canonical ULID string.
    #[error("cannot parse {input:?} as a ULID")]
    Parse { input: String },
}

/// SplitMix64 (Vigna). Portable, byte-stable forever, pinned by test vectors.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub const fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

const ULID_RANDOM_MAX: u128 = (1 << 80) - 1;
const ULID_TS_MAX_MS: i64 = (1 << 48) - 1;

/// Seeded, deterministic, monotonic ULID generator. Time is injected per
/// call (from the `Clock`); the generator never reads wall time itself.
#[derive(Debug, Clone)]
pub struct IdGen {
    rng: SplitMix64,
    last: Option<(u64, u128)>, // (millis, random) of the last issued id
}

impl IdGen {
    pub const fn new(seed: u64) -> Self {
        IdGen {
            rng: SplitMix64::new(seed),
            last: None,
        }
    }

    pub fn next(&mut self, now: UtcTimestamp) -> Result<Ulid, IdError> {
        let millis = now.epoch_millis();
        if !(0..=ULID_TS_MAX_MS).contains(&millis) {
            return Err(IdError::TimestampOutOfRange { millis });
        }
        let mut ms = millis as u64;
        let random = match self.last {
            // Same or backwards millisecond: clamp forward and bump the
            // random part so ordering and uniqueness hold unconditionally.
            Some((last_ms, last_random)) if ms <= last_ms => {
                ms = last_ms;
                if last_random >= ULID_RANDOM_MAX {
                    return Err(IdError::RandomExhausted);
                }
                last_random + 1
            }
            _ => self.fresh_random(),
        };
        self.last = Some((ms, random));
        Ok(Ulid::from_parts(ms, random))
    }

    /// 80 fresh bits: high 16 from one PRNG draw, low 64 from the next.
    /// Draw order is part of the determinism contract; never reorder.
    fn fresh_random(&mut self) -> u128 {
        let hi = u128::from(self.rng.next_u64() & 0xFFFF);
        let lo = u128::from(self.rng.next_u64());
        (hi << 64) | lo
    }
}

/// Define a ULID-backed typed id: `Display`/`FromStr` as the canonical ULID
/// string, serde as a string, `Ord` matching ULID order. Consuming crates
/// need `serde` as a dependency (derive expansion references it).
#[macro_export]
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            ::serde::Serialize, ::serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name($crate::ids::Ulid);

        impl $name {
            pub const fn new(id: $crate::ids::Ulid) -> Self {
                Self(id)
            }

            pub const fn ulid(self) -> $crate::ids::Ulid {
                self.0
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(&self.0, f)
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::ids::IdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse::<$crate::ids::Ulid>()
                    .map(Self)
                    .map_err(|_| $crate::ids::IdError::Parse {
                        input: s.to_string(),
                    })
            }
        }
    };
}

define_id!(
    /// An order intent (spec 5.4). Client order ids derive from this.
    IntentId
);
define_id!(
    /// A multi-leg intent group (spec 5.4).
    IntentGroupId
);
define_id!(
    /// A belief row (spec 5.5).
    BeliefId
);
define_id!(
    /// A canonical event (spec 5.12).
    EventId
);
define_id!(
    /// A model proposal (spec 5.9).
    ProposalId
);
define_id!(
    /// An audit record (spec I5).
    AuditId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_exhaustion_within_one_millisecond_is_an_error() {
        // Force the internal high-water mark to the random ceiling; the next
        // id in the same millisecond must error, never wrap or duplicate.
        let mut g = IdGen::new(0);
        g.last = Some((1_000, ULID_RANDOM_MAX));
        let now = UtcTimestamp::from_epoch_millis(1_000).unwrap();
        assert_eq!(g.next(now), Err(IdError::RandomExhausted));
        // A later millisecond recovers with fresh randomness.
        let later = UtcTimestamp::from_epoch_millis(1_001).unwrap();
        assert!(g.next(later).is_ok());
    }
}
