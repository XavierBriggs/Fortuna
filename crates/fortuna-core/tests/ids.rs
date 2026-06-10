//! T0.1 tests: deterministic ULID generation and typed ids. Written from spec
//! conventions ("IDs are ULIDs") + the determinism doctrine before implementation.
//!
//! Contract: id generation must be deterministic under a seed (same seed +
//! same clock readings => byte-identical id sequence on any platform), which
//! is why the PRNG is an in-house SplitMix64 with published test vectors
//! rather than a `rand` generator with no cross-version stability promise.
//! ULIDs are monotonic: strictly increasing even within one millisecond and
//! even if the clock is read backwards (clamp-forward, never duplicate).

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IdGen, SplitMix64};

fn ts(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(ms).unwrap()
}

// ---- SplitMix64 (the determinism anchor) ----

#[test]
fn splitmix64_matches_published_vectors_seed_0() {
    let mut rng = SplitMix64::new(0);
    assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
    assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
    assert_eq!(rng.next_u64(), 0x06C4_5D18_8009_454F);
}

#[test]
fn splitmix64_matches_published_vectors_seed_42() {
    let mut rng = SplitMix64::new(42);
    assert_eq!(rng.next_u64(), 0xBDD7_3226_2FEB_6E95);
    assert_eq!(rng.next_u64(), 0x28EF_E333_B266_F103);
    assert_eq!(rng.next_u64(), 0x4752_6757_130F_9F52);
}

// ---- IdGen determinism ----

#[test]
fn same_seed_same_timestamps_yield_identical_id_sequences() {
    let mut a = IdGen::new(7);
    let mut b = IdGen::new(7);
    for ms in [1_000i64, 1_000, 1_001, 2_000, 2_000, 2_000] {
        let ia = a.next(ts(ms)).unwrap();
        let ib = b.next(ts(ms)).unwrap();
        assert_eq!(ia.to_string(), ib.to_string());
    }
}

#[test]
fn different_seeds_yield_different_ids() {
    let mut a = IdGen::new(1);
    let mut b = IdGen::new(2);
    assert_ne!(
        a.next(ts(1000)).unwrap().to_string(),
        b.next(ts(1000)).unwrap().to_string()
    );
}

#[test]
fn ulid_embeds_the_clock_timestamp() {
    let mut g = IdGen::new(7);
    let id = g.next(ts(1_700_000_000_123)).unwrap();
    assert_eq!(id.timestamp_ms(), 1_700_000_000_123);
}

#[test]
fn ids_strictly_increase_within_one_millisecond() {
    let mut g = IdGen::new(7);
    let mut prev = g.next(ts(1000)).unwrap();
    for _ in 0..1000 {
        let next = g.next(ts(1000)).unwrap();
        assert!(next > prev, "monotonicity violated: {next} <= {prev}");
        assert_eq!(next.timestamp_ms(), 1000);
        prev = next;
    }
}

#[test]
fn ids_strictly_increase_across_milliseconds() {
    let mut g = IdGen::new(7);
    let a = g.next(ts(1000)).unwrap();
    let b = g.next(ts(2000)).unwrap();
    assert!(b > a);
}

#[test]
fn backwards_clock_clamps_forward_never_duplicates_or_reorders() {
    // A real clock can jump backwards (NTP). Ids must remain strictly
    // increasing; the generator clamps to the high-water-mark millisecond.
    let mut g = IdGen::new(7);
    let a = g.next(ts(5000)).unwrap();
    let b = g.next(ts(4000)).unwrap(); // clock went backwards
    assert!(b > a);
    assert_eq!(b.timestamp_ms(), 5000); // clamped, not trusted
}

#[test]
fn pre_1970_timestamp_is_rejected() {
    let mut g = IdGen::new(7);
    assert!(g.next(ts(-1)).is_err());
}

#[test]
fn timestamp_beyond_48_bits_is_rejected() {
    // ULID timestamps are 48-bit milliseconds; 2^48 = 281474976710656.
    let mut g = IdGen::new(7);
    assert!(g.next(ts(281_474_976_710_656)).is_err());
    assert!(g.next(ts(281_474_976_710_655)).is_ok()); // last representable ms
}

// ---- properties ----

use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_ids_strictly_increase_for_any_timestamp_sequence(
        seed in any::<u64>(),
        // Arbitrary order, including repeats and backwards jumps.
        millis in proptest::collection::vec(0i64..281_474_976_710_655, 1..200),
    ) {
        let mut g = IdGen::new(seed);
        let mut prev: Option<fortuna_core::ids::Ulid> = None;
        for ms in millis {
            let id = g.next(ts(ms)).unwrap();
            if let Some(p) = prev {
                prop_assert!(id > p, "monotonicity violated: {id} <= {p}");
            }
            prev = Some(id);
        }
    }

    #[test]
    fn prop_same_seed_same_sequence(
        seed in any::<u64>(),
        millis in proptest::collection::vec(0i64..281_474_976_710_655, 1..50),
    ) {
        let mut a = IdGen::new(seed);
        let mut b = IdGen::new(seed);
        for ms in millis {
            prop_assert_eq!(a.next(ts(ms)).unwrap(), b.next(ts(ms)).unwrap());
        }
    }
}

// ---- typed ids ----

#[test]
fn typed_ids_display_as_ulid_and_round_trip() {
    use fortuna_core::ids::IntentId;
    let mut g = IdGen::new(7);
    let id = IntentId::new(g.next(ts(1000)).unwrap());
    let s = id.to_string();
    assert_eq!(s.len(), 26); // canonical ULID text length
    let parsed: IntentId = s.parse().unwrap();
    assert_eq!(parsed, id);
}

#[test]
fn typed_ids_serde_as_ulid_strings() {
    use fortuna_core::ids::BeliefId;
    let mut g = IdGen::new(9);
    let id = BeliefId::new(g.next(ts(1000)).unwrap());
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, format!("\"{id}\""));
    let back: BeliefId = serde_json::from_str(&json).unwrap();
    assert_eq!(back, id);
}

#[test]
fn typed_id_ordering_matches_generation_order() {
    use fortuna_core::ids::EventId;
    let mut g = IdGen::new(7);
    let a = EventId::new(g.next(ts(1000)).unwrap());
    let b = EventId::new(g.next(ts(1000)).unwrap());
    assert!(a < b);
}

#[test]
fn typed_id_parse_rejects_garbage() {
    use fortuna_core::ids::IntentId;
    assert!("not-a-ulid".parse::<IntentId>().is_err());
    assert!("".parse::<IntentId>().is_err());
}
