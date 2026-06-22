//! G-PIT (point-in-time / no-look-ahead) tests for the as-of join (S2).
//!
//! Written FROM the plan text (S2) and spec §5 G-PIT BEFORE the implementation
//! (TDD). The rule under test is the load-bearing one:
//!
//!   A record enters a decision's context iff `available_at < decided_at`
//!   **STRICT**.
//!
//! Strict `<` is the conservative tie-break against coarse/same-bucket
//! timestamps (a daily forecast whose `available_at` and `decided_at` land in
//! the same day must not admit same-bucket future info). These tests are
//! black-box: they assert on the PUBLIC `asof_join` contract (the returned
//! `DecisionContext` / disposition + the look-ahead count), never on internals.
//!
//! BVA coverage: the equality boundary (`available_at == decided_at`) is the
//! single most important case — a `<` → `<=` mutation in `asof.rs` MUST red
//! `gpit_strict_excludes_equal_timestamp`.

use fortuna_backtest::asof::{asof_join, AsOfDisposition};
use fortuna_backtest::records::{
    BeliefPayload, HistoricalBelief, HistoricalOutcome, HistoricalSnapshot, Provenance,
};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::money::Cents;

const LINKAGE: &str = "event://forecast/station-DFW/bracket-ge40/2026-07-04";

fn ts(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(ms).expect("valid timestamp")
}

fn provenance() -> Provenance {
    Provenance {
        producer_type: "forecast".to_string(),
        producer_id: "p1".to_string(),
        mind_id: None,
        mind_version: None,
        strategy_id: "s1".to_string(),
        category: "weather".to_string(),
        scope: "weather:DFW".to_string(),
    }
}

/// A belief whose `available_at`/`decided_at` are caller-controlled so each test
/// can pin the boundary it is exercising.
fn belief(available_ms: i64, decided_ms: i64, p: f64) -> HistoricalBelief {
    HistoricalBelief {
        provenance: provenance(),
        payload: BeliefPayload::Binary { p },
        event_linkage: LINKAGE.to_string(),
        available_at: ts(available_ms),
        decided_at: ts(decided_ms),
    }
}

fn outcome(resolved_ms: i64, o: f64) -> HistoricalOutcome {
    HistoricalOutcome {
        event_linkage: LINKAGE.to_string(),
        outcome: o,
        resolved_at: ts(resolved_ms),
        resolution_source: "settlement".to_string(),
    }
}

fn snapshot(at_ms: i64, price: i64) -> HistoricalSnapshot {
    HistoricalSnapshot {
        // The snapshot's `market` carries the canonical join key (the harness
        // normalises a venue ticker to the `event_linkage` namespace before
        // emitting snapshots — see the source.rs namespace contract); the as-of
        // join matches on it.
        market: LINKAGE.to_string(),
        price: Cents::new(price),
        at: ts(at_ms),
    }
}

#[test]
fn gpit_strict_excludes_equal_timestamp() {
    // BVA — the load-bearing equality boundary. available_at == decided_at is a
    // LEAK under strict `<` and must be EXCLUDED + counted. A `<` → `<=`
    // mutation in asof.rs flips this to Joined and reds the test.
    let decided = 1_000_000;
    let b = belief(decided, decided, 0.7); // available_at == decided_at exactly
    let disposition = asof_join(&b, &[], &[]);
    assert_eq!(
        disposition,
        AsOfDisposition::LookAheadRejected,
        "available_at == decided_at must be REJECTED under strict `<` (a `<=` mutation reds this)"
    );
}

#[test]
fn gpit_admits_strictly_prior_belief() {
    // The positive control: one millisecond before decided_at is admitted.
    // This pins that the rejection above is the boundary, not an over-broad
    // "reject everything" bug.
    let decided = 1_000_000;
    let b = belief(decided - 1, decided, 0.7);
    let disposition = asof_join(&b, &[], &[]);
    match disposition {
        AsOfDisposition::Joined(ctx) => {
            let ctx = *ctx;
            assert_eq!(ctx.belief.payload, BeliefPayload::Binary { p: 0.7 });
            assert!(ctx.snapshot.is_none());
            assert!(ctx.outcome.is_none());
        }
        AsOfDisposition::LookAheadRejected => {
            panic!("a strictly-prior belief (available_at < decided_at) must be Joined")
        }
    }
}

#[test]
fn gpit_rejects_future_data() {
    // available_at strictly AFTER decided_at is the textbook look-ahead leak →
    // excluded + counted.
    let decided = 1_000_000;
    let b = belief(decided + 1, decided, 0.7);
    let disposition = asof_join(&b, &[], &[]);
    assert_eq!(
        disposition,
        AsOfDisposition::LookAheadRejected,
        "available_at > decided_at is future-contaminated and must be rejected"
    );
}

#[test]
fn asof_picks_latest_prior_snapshot() {
    // CLV-entry snapshot = the LATEST snapshot strictly before decided_at.
    // - one at decided_at-100 (eligible, older)
    // - one at decided_at-1   (eligible, latest → must be chosen)
    // - one at decided_at     (INELIGIBLE: not strictly before → must be skipped
    //   even though it is the newest, which is exactly the leak strict `<` stops)
    // - one at decided_at+50  (future → skipped)
    let decided = 1_000_000;
    let b = belief(decided - 10, decided, 0.6);
    let snaps = vec![
        snapshot(decided - 100, 31),
        snapshot(decided - 1, 42),
        snapshot(decided, 99),      // tie at decided_at — must NOT be picked
        snapshot(decided + 50, 77), // future — must NOT be picked
    ];
    let disposition = asof_join(&b, &snaps, &[]);
    match disposition {
        AsOfDisposition::Joined(ctx) => {
            let ctx = *ctx;
            let chosen = ctx.snapshot.expect("a prior snapshot exists");
            assert_eq!(
                chosen.price,
                Cents::new(42),
                "the CLV-entry snapshot is the latest with at < decided_at (the 42c one), \
                 NOT the tie-at-decided_at 99c snapshot"
            );
        }
        AsOfDisposition::LookAheadRejected => panic!("belief is strictly prior; must be Joined"),
    }
}

#[test]
fn asof_attaches_outcome_label_regardless_of_resolution_time() {
    // An outcome is a POST-RESOLUTION label (available_at == resolved_at) — it
    // may LABEL but never DECIDE. It is attached to the joined context on
    // event_linkage so the harness can score, and its resolution time being
    // after decided_at is EXPECTED (you resolve after you decide), not a leak.
    let decided = 1_000_000;
    let b = belief(decided - 5, decided, 0.55);
    let outcomes = vec![outcome(decided + 10_000, 1.0)];
    let disposition = asof_join(&b, &[], &outcomes);
    match disposition {
        AsOfDisposition::Joined(ctx) => {
            let ctx = *ctx;
            let o = ctx.outcome.expect("the matching outcome is attached");
            assert_eq!(o.outcome, 1.0);
        }
        AsOfDisposition::LookAheadRejected => panic!("strictly-prior belief must be Joined"),
    }
}

#[test]
fn asof_ignores_outcome_for_a_different_event() {
    // The join is on event_linkage: an outcome for a DIFFERENT event must not
    // contaminate this decision (the silent-mismatch failure mode the namespace
    // contract guards against).
    let decided = 1_000_000;
    let b = belief(decided - 5, decided, 0.55);
    let other = HistoricalOutcome {
        event_linkage: "event://forecast/station-LGA/bracket-ge40/2026-07-04".to_string(),
        outcome: 0.0,
        resolved_at: ts(decided + 10_000),
        resolution_source: "settlement".to_string(),
    };
    let disposition = asof_join(&b, &[], &[other]);
    match disposition {
        AsOfDisposition::Joined(ctx) => {
            let ctx = *ctx;
            assert!(
                ctx.outcome.is_none(),
                "an outcome for a different event_linkage must not be attached"
            );
        }
        AsOfDisposition::LookAheadRejected => panic!("strictly-prior belief must be Joined"),
    }
}
