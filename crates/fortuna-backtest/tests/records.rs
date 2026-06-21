//! Integration tests for S1: record contracts, JSONL round-trips, manifest,
//! and the orders==0 paper-only invariant.
//!
//! These are the TDD "failing" tests — written before implementation.

use fortuna_backtest::manifest::{EngagedMarket, UniverseManifest};
use fortuna_backtest::records::{
    BeliefPayload, HistoricalBelief, HistoricalOutcome, HistoricalSnapshot, HistoricalTrade,
    Provenance, RecordError,
};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::money::Cents;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ts(millis: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(millis).expect("test timestamp must be valid")
}

fn sample_provenance() -> Provenance {
    Provenance {
        producer_type: "forecast-model".to_string(),
        producer_id: "prod-001".to_string(),
        mind_id: Some("mind-abc".to_string()),
        mind_version: Some(7),
        strategy_id: "strat-x".to_string(),
        category: "temperature".to_string(),
        scope: "north-america".to_string(),
    }
}

// ---------------------------------------------------------------------------
// 1. belief_jsonl_round_trips — Binary AND Scalar payloads each survive
//    one JSONL line (serialize → deserialize → assert eq).
// ---------------------------------------------------------------------------

#[test]
fn belief_jsonl_round_trips() {
    let binary_belief = HistoricalBelief {
        provenance: sample_provenance(),
        payload: BeliefPayload::Binary { p: 0.73 },
        event_linkage: "event://forecast/station-001/bracket-ge40/2026-07-04".to_string(),
        available_at: ts(1_000_000),
        decided_at: ts(2_000_000),
    };

    let scalar_belief = HistoricalBelief {
        provenance: sample_provenance(),
        payload: BeliefPayload::Scalar {
            quantiles: vec![(0.1, 35.0), (0.5, 40.0), (0.9, 45.0)],
        },
        event_linkage: "event://forecast/station-001/temp-dist/2026-07-04".to_string(),
        available_at: ts(1_000_000),
        decided_at: ts(2_000_000),
    };

    // Each serializes to exactly one JSONL line (no embedded newlines).
    let binary_line = serde_json::to_string(&binary_belief).unwrap();
    let scalar_line = serde_json::to_string(&scalar_belief).unwrap();

    assert!(
        !binary_line.contains('\n'),
        "JSONL line must not contain newlines"
    );
    assert!(
        !scalar_line.contains('\n'),
        "JSONL line must not contain newlines"
    );

    // Pin the exact JSONL wire-format tokens so that a serde-attribute mutation
    // (renaming the tag key, changing rename_all, etc.) turns this test RED.
    // These tokens come from BeliefPayload: #[serde(tag = "kind", rename_all = "snake_case")]
    assert!(
        binary_line.contains("\"kind\":\"binary\""),
        "Binary variant must serialize with tag key 'kind' and value 'binary'; got: {binary_line}"
    );
    assert!(
        binary_line.contains("\"p\":"),
        "Binary variant must serialize the 'p' field; got: {binary_line}"
    );
    assert!(
        scalar_line.contains("\"kind\":\"scalar\""),
        "Scalar variant must serialize with tag key 'kind' and value 'scalar'; got: {scalar_line}"
    );
    assert!(
        scalar_line.contains("\"quantiles\":"),
        "Scalar variant must serialize the 'quantiles' field; got: {scalar_line}"
    );

    let binary_rt: HistoricalBelief = serde_json::from_str(&binary_line).unwrap();
    let scalar_rt: HistoricalBelief = serde_json::from_str(&scalar_line).unwrap();

    assert_eq!(binary_belief, binary_rt);
    assert_eq!(scalar_belief, scalar_rt);
}

// ---------------------------------------------------------------------------
// 2. outcome_snapshot_trade_round_trip
// ---------------------------------------------------------------------------

#[test]
fn outcome_snapshot_trade_round_trip() {
    let outcome = HistoricalOutcome {
        event_linkage: "event://forecast/station-001/bracket-ge40/2026-07-04".to_string(),
        outcome: 1.0,
        resolved_at: ts(5_000_000),
        resolution_source: "official-weather-service".to_string(),
    };

    let snapshot = HistoricalSnapshot {
        market: "MKT-00123".to_string(),
        price: Cents::new(55),
        at: ts(1_500_000),
    };

    let trade = HistoricalTrade::new(
        "event://link".to_string(),
        "yes".to_string(),
        Cents::new(55),
        10,
        ts(1_600_000),
        0,
    )
    .unwrap();

    // Round-trip each through one JSONL line.
    let outcome_rt: HistoricalOutcome =
        serde_json::from_str(&serde_json::to_string(&outcome).unwrap()).unwrap();
    let snapshot_rt: HistoricalSnapshot =
        serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
    let trade_rt: HistoricalTrade =
        serde_json::from_str(&serde_json::to_string(&trade).unwrap()).unwrap();

    assert_eq!(outcome, outcome_rt);
    assert_eq!(snapshot, snapshot_rt);
    assert_eq!(trade, trade_rt);
}

// ---------------------------------------------------------------------------
// 3. trade_orders_is_zero — constructor rejects orders != 0; accepts orders == 0.
// ---------------------------------------------------------------------------

#[test]
fn trade_orders_is_zero() {
    // orders == 0 must succeed (paper-only: no real order ever placed).
    let ok = HistoricalTrade::new(
        "event://link".to_string(),
        "yes".to_string(),
        Cents::new(60),
        5,
        ts(1_000_000),
        0,
    );
    assert!(ok.is_ok(), "orders == 0 must succeed");

    // orders != 0 must be rejected.
    let err = HistoricalTrade::new(
        "event://link".to_string(),
        "yes".to_string(),
        Cents::new(60),
        5,
        ts(1_000_000),
        1,
    );
    assert!(
        matches!(err, Err(RecordError::RealOrderForbidden { orders: 1 })),
        "orders != 0 must produce RealOrderForbidden; got {err:?}"
    );

    let err2 = HistoricalTrade::new(
        "event://link".to_string(),
        "yes".to_string(),
        Cents::new(60),
        5,
        ts(1_000_000),
        999,
    );
    assert!(
        matches!(err2, Err(RecordError::RealOrderForbidden { orders: 999 })),
        "orders 999 must produce RealOrderForbidden; got {err2:?}"
    );
}

// ---------------------------------------------------------------------------
// 4a. manifest_round_trips
// ---------------------------------------------------------------------------

#[test]
fn manifest_round_trips() {
    let manifest = UniverseManifest {
        engaged: vec![
            EngagedMarket {
                event_linkage: "event://link-a".to_string(),
                resolved: true,
                voided: false,
            },
            EngagedMarket {
                event_linkage: "event://link-b".to_string(),
                resolved: false,
                voided: true,
            },
        ],
    };

    let line = serde_json::to_string(&manifest).unwrap();
    assert!(!line.contains('\n'));

    // Pin the exact field names in the wire format so that a rename_all or field
    // rename mutation on EngagedMarket turns this test RED.
    assert!(
        line.contains("\"voided\":"),
        "EngagedMarket must serialize the 'voided' field; got: {line}"
    );
    assert!(
        line.contains("\"resolved\":"),
        "EngagedMarket must serialize the 'resolved' field; got: {line}"
    );

    let rt: UniverseManifest = serde_json::from_str(&line).unwrap();
    assert_eq!(manifest, rt);
}

// ---------------------------------------------------------------------------
// 4b. manifest_marks_voided_and_resolved
// ---------------------------------------------------------------------------

#[test]
fn manifest_marks_voided_and_resolved() {
    let manifest = UniverseManifest {
        engaged: vec![
            EngagedMarket {
                event_linkage: "event://live".to_string(),
                resolved: true,
                voided: false,
            },
            EngagedMarket {
                event_linkage: "event://void".to_string(),
                resolved: false,
                voided: true,
            },
            EngagedMarket {
                event_linkage: "event://open".to_string(),
                resolved: false,
                voided: false,
            },
        ],
    };

    let voided: Vec<_> = manifest.engaged.iter().filter(|m| m.voided).collect();
    let resolved: Vec<_> = manifest.engaged.iter().filter(|m| m.resolved).collect();

    assert_eq!(voided.len(), 1);
    assert_eq!(voided[0].event_linkage, "event://void");

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].event_linkage, "event://live");
}

// ---------------------------------------------------------------------------
// 5. proptest: round-trip stability over arbitrary records
// ---------------------------------------------------------------------------

use proptest::prelude::*;

proptest! {
    /// Round-trip a Binary belief through one JSONL line.
    ///
    /// float_roundtrip feature makes serde_json deserialization bit-exact for finite f64
    /// (ryu already gives exact shortest serialization).
    #[test]
    fn belief_binary_round_trips_prop(
        p in 0.0_f64..=1.0_f64,
        available_millis in 1_000_000_i64..2_000_000_i64,
        decided_millis in 2_000_000_i64..3_000_000_i64,
    ) {
        let belief = HistoricalBelief {
            provenance: sample_provenance(),
            payload: BeliefPayload::Binary { p },
            event_linkage: "event://prop-test".to_string(),
            available_at: ts(available_millis),
            decided_at: ts(decided_millis),
        };
        let serialized = serde_json::to_string(&belief).unwrap();
        let deserialized: HistoricalBelief = serde_json::from_str(&serialized).unwrap();
        // Structural equality on all non-f64 fields.
        prop_assert_eq!(&belief.provenance, &deserialized.provenance);
        prop_assert_eq!(&belief.event_linkage, &deserialized.event_linkage);
        prop_assert_eq!(belief.available_at, deserialized.available_at);
        prop_assert_eq!(belief.decided_at, deserialized.decided_at);
        // float_roundtrip feature makes serde_json deserialization bit-exact for finite f64
        // (ryu already gives exact shortest serialization).
        match (&belief.payload, &deserialized.payload) {
            (BeliefPayload::Binary { p: orig }, BeliefPayload::Binary { p: rt }) => {
                prop_assert_eq!(orig, rt, "p round-trip must be exact for finite f64");
            }
            _ => prop_assert!(false, "payload kind mismatch"),
        }
    }

    #[test]
    fn outcome_round_trips_prop(
        outcome_val in 0.0_f64..=1.0_f64,
        resolved_millis in 1_000_000_i64..5_000_000_i64,
    ) {
        let outcome = HistoricalOutcome {
            event_linkage: "event://prop-test".to_string(),
            outcome: outcome_val,
            resolved_at: ts(resolved_millis),
            resolution_source: "official-source".to_string(),
        };
        let serialized = serde_json::to_string(&outcome).unwrap();
        let deserialized: HistoricalOutcome = serde_json::from_str(&serialized).unwrap();
        prop_assert_eq!(&outcome.event_linkage, &deserialized.event_linkage);
        prop_assert_eq!(outcome.resolved_at, deserialized.resolved_at);
        prop_assert_eq!(&outcome.resolution_source, &deserialized.resolution_source);
        // float_roundtrip feature makes serde_json deserialization bit-exact for finite f64
        // (ryu already gives exact shortest serialization).
        prop_assert_eq!(
            outcome.outcome,
            deserialized.outcome,
            "outcome round-trip must be exact for finite f64"
        );
    }

    #[test]
    fn snapshot_round_trips_prop(
        price_raw in i64::MIN..=i64::MAX,
        at_millis in 1_000_000_i64..5_000_000_i64,
    ) {
        let snapshot = HistoricalSnapshot {
            market: "MKT-PROP".to_string(),
            price: Cents::new(price_raw),
            at: ts(at_millis),
        };
        let serialized = serde_json::to_string(&snapshot).unwrap();
        let deserialized: HistoricalSnapshot = serde_json::from_str(&serialized).unwrap();
        prop_assert_eq!(snapshot, deserialized);
    }

    #[test]
    fn manifest_round_trips_prop(
        n_markets in 0_usize..20_usize,
    ) {
        let engaged: Vec<EngagedMarket> = (0..n_markets)
            .map(|i| EngagedMarket {
                event_linkage: format!("event://prop/{i}"),
                resolved: i % 2 == 0,
                voided: i % 3 == 0,
            })
            .collect();
        let manifest = UniverseManifest { engaged };
        let serialized = serde_json::to_string(&manifest).unwrap();
        let deserialized: UniverseManifest = serde_json::from_str(&serialized).unwrap();
        prop_assert_eq!(manifest, deserialized);
    }

    /// Round-trip a Scalar belief with varying-length quantiles vectors
    /// (includes empty, single-element, and larger vecs).
    ///
    /// float_roundtrip feature makes serde_json deserialization bit-exact for finite f64
    /// (ryu already gives exact shortest serialization).
    #[test]
    fn belief_scalar_round_trips_prop(
        quantiles in proptest::collection::vec(
            (0.0_f64..=1.0_f64, 0.0_f64..=100.0_f64),
            0..=10_usize,
        ),
        available_millis in 1_000_000_i64..2_000_000_i64,
        decided_millis in 2_000_000_i64..3_000_000_i64,
    ) {
        let belief = HistoricalBelief {
            provenance: sample_provenance(),
            payload: BeliefPayload::Scalar { quantiles: quantiles.clone() },
            event_linkage: "event://prop-scalar".to_string(),
            available_at: ts(available_millis),
            decided_at: ts(decided_millis),
        };
        let serialized = serde_json::to_string(&belief).unwrap();
        let deserialized: HistoricalBelief = serde_json::from_str(&serialized).unwrap();
        // float_roundtrip feature makes serde_json deserialization bit-exact for finite f64
        // (ryu already gives exact shortest serialization).
        prop_assert_eq!(belief, deserialized, "Scalar belief round-trip must be exact for finite f64");
    }

    /// Round-trip a HistoricalTrade with orders == 0 (paper-only invariant).
    ///
    /// serde_json/Ryu shortest-round-trip → exact for finite f64 (no f64 here,
    /// but the pattern is uniform: strict equality on all fields).
    #[test]
    fn historical_trade_round_trips_prop(
        price_raw in i64::MIN..=i64::MAX,
        contracts in 0_u32..=u32::MAX,
        at_millis in 1_000_000_i64..5_000_000_i64,
    ) {
        // orders == 0 is the paper-only invariant; the constructor enforces it.
        let trade = HistoricalTrade::new(
            "event://prop-trade".to_string(),
            "yes".to_string(),
            Cents::new(price_raw),
            contracts,
            ts(at_millis),
            0,
        ).unwrap();
        let serialized = serde_json::to_string(&trade).unwrap();
        let deserialized: HistoricalTrade = serde_json::from_str(&serialized).unwrap();
        prop_assert_eq!(trade, deserialized, "HistoricalTrade round-trip must be exact");
    }
}
