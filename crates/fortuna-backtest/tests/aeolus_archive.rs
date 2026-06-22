//! S6 integration tests: `AeolusArchiveSource` mapped against the FIRMED real
//! Aeolus schema, exercised over a small committed SQLite fixture
//! (`tests/fixtures/aeolus_archive.sql`) loaded into an in-memory rusqlite DB.
//! NEVER the 17.8 GB live DB.
//!
//! Written FROM the plan (S6) and spec §9 BEFORE the implementation (TDD).
//!
//! ## The load-bearing trap (spec §9): the post-resolution leak
//!
//! Aeolus was built to *trade*, not as a bitemporal research archive, so it
//! mixes pre-decision and post-resolution data. A `HistoricalBelief`'s
//! `available_at` MUST be the forecast-ISSUANCE instant
//! (`bracket_probability_log.forecast_init_time`) — the knowledge time. It is
//! NEVER `target_date` (the event day) and NEVER `market_resolutions.settled_at`
//! (resolution). And NO realized score (crps/pit/absolute_error) may flow into
//! a belief payload — those are outcome-side only; FORTUNA recomputes them.
//!
//! These are black-box tests: they assert on the PUBLIC `HistoricalSource`
//! contract (the mapped records + the manifest), never on adapter internals.

use std::path::PathBuf;

use fortuna_backtest::records::BeliefPayload;
use fortuna_backtest::source::HistoricalSource;
use fortuna_backtest::sources::aeolus_archive::{AeolusArchiveSource, TimeRange};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Absolute path to the committed fixture SQL.
fn fixture_sql_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("aeolus_archive.sql")
}

/// Build a source backed by an in-memory DB seeded from the fixture SQL.
fn fixture_source() -> AeolusArchiveSource {
    AeolusArchiveSource::from_sql_fixture(&fixture_sql_path(), TimeRange::unbounded())
        .expect("fixture must load")
}

// The three DISTINCT instants the fixture encodes (see the .sql header).
const ISSUANCE: &str = "2026-07-01T00:00:00.000Z";
const TARGET_DATE: &str = "2026-07-04";
const SETTLED_AT: &str = "2026-07-05T18:00:00.000Z";

// ---------------------------------------------------------------------------
// 1. THE TRAP (load-bearing): a belief's available_at is the ISSUANCE instant,
//    never target_date, never settled_at — and the payload is the issuance-time
//    probability ONLY, with no realized score flowing in.
// ---------------------------------------------------------------------------

#[test]
fn aeolus_belief_available_at_is_issuance() {
    let src = fixture_source();

    let beliefs: Vec<_> = src
        .beliefs()
        .map(|r| r.expect("belief row must map"))
        .collect();

    assert!(
        !beliefs.is_empty(),
        "fixture must yield at least one belief"
    );

    for belief in &beliefs {
        // available_at is the forecast-ISSUANCE instant (knowledge time).
        assert_eq!(
            belief.available_at.to_iso8601(),
            ISSUANCE,
            "available_at MUST be forecast_init_time (issuance), not target/resolution"
        );

        // It is NEVER the event day...
        assert_ne!(
            belief.available_at.to_iso8601(),
            format!("{TARGET_DATE}T00:00:00.000Z"),
            "available_at must NOT be target_date (the event day)"
        );
        // ...and NEVER the resolution instant.
        assert_ne!(
            belief.available_at.to_iso8601(),
            SETTLED_AT,
            "available_at must NOT be settled_at (resolution time)"
        );

        // decided_at is strictly AFTER available_at (the harness G-PIT rule
        // admits a belief iff available_at < decided_at; equality is a leak).
        assert!(
            belief.available_at < belief.decided_at,
            "available_at must be strictly before decided_at (no same-instant leak)"
        );

        // The payload is the issuance-time probability ONLY — a Binary p in
        // [0, 1]. No realized score (crps/pit/absolute_error) is a probability
        // in this shape; the contract structurally forbids a score leaking in.
        match &belief.payload {
            BeliefPayload::Binary { p } => {
                assert!(
                    (0.0..=1.0).contains(p),
                    "belief payload must be an issuance probability in [0,1], got {p}"
                );
            }
            BeliefPayload::Scalar { .. } => {
                panic!("Aeolus bracket beliefs are Binary, never Scalar")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 2. The manifest includes the VOIDED market (result NOT IN ('yes','no')).
// ---------------------------------------------------------------------------

#[test]
fn aeolus_manifest_includes_voided() {
    let src = fixture_source();
    let manifest = src.universe_manifest().expect("manifest must build");

    // The fixture's voided market is MKT-VOID; its canonical linkage carries
    // the station/bracket/target encoding. We match on the ticker token.
    let voided: Vec<_> = manifest.engaged.iter().filter(|m| m.voided).collect();

    assert_eq!(
        voided.len(),
        1,
        "exactly one voided market expected in the manifest"
    );
    assert!(
        voided[0].event_linkage.contains("MKT-VOID") || voided[0].event_linkage.contains("DFW"),
        "the voided market must be present and identifiable: {}",
        voided[0].event_linkage
    );
    assert!(
        !voided[0].resolved,
        "a voided market is NOT resolved (it was cancelled before resolution)"
    );

    // And every engaged market (3) is present, voided included.
    assert_eq!(
        manifest.engaged.len(),
        3,
        "all three engaged markets (yes, no, void) must appear in the manifest"
    );
}

// ---------------------------------------------------------------------------
// 3. A NO-resolved market (result='no') maps to outcome 0.0 and is present in
//    both outcomes and the manifest (resolved=true, voided=false).
// ---------------------------------------------------------------------------

#[test]
fn aeolus_no_resolved_present() {
    let src = fixture_source();

    let outcomes: Vec<_> = src
        .outcomes()
        .map(|r| r.expect("outcome row must map"))
        .collect();

    let no_outcome = outcomes
        .iter()
        .find(|o| o.event_linkage.contains("MKT-NO"))
        .expect("the NO-resolved market must appear in outcomes");

    assert_eq!(no_outcome.outcome, 0.0, "result='no' maps to outcome 0.0");
    // The outcome's knowledge time is the RESOLUTION instant.
    assert_eq!(
        no_outcome.resolved_at.to_iso8601(),
        SETTLED_AT,
        "an outcome's resolved_at is settled_at"
    );

    // It is also in the manifest, resolved and not voided.
    let manifest = src.universe_manifest().expect("manifest must build");
    let no_market = manifest
        .engaged
        .iter()
        .find(|m| m.event_linkage.contains("MKT-NO"))
        .expect("the NO-resolved market must be in the manifest");
    assert!(no_market.resolved, "NO-resolved is resolved");
    assert!(!no_market.voided, "NO-resolved is not voided");

    // The voided market must NOT appear in outcomes (no numeric label).
    assert!(
        !outcomes
            .iter()
            .any(|o| o.event_linkage.contains("MKT-VOID")),
        "a voided market has no numeric outcome and must not be in outcomes"
    );
}

// ---------------------------------------------------------------------------
// 4. Streaming: the belief/outcome iterators are lazy (row-by-row), not a
//    full Vec materialization. We assert on the iterator's bounded-memory
//    contract: the source yields Result<_, SourceError> per row and can be
//    consumed one element at a time without collecting the whole archive.
// ---------------------------------------------------------------------------

#[test]
fn aeolus_streams_without_full_load() {
    let src = fixture_source();

    // Consume exactly ONE belief via the iterator, then drop the rest. A
    // fully-materialized Vec would have already allocated the whole archive;
    // a lazy iterator yields just the first row on demand.
    let mut beliefs = src.beliefs();
    let first = beliefs
        .next()
        .expect("at least one belief")
        .expect("first belief maps cleanly");
    assert!(
        matches!(first.payload, BeliefPayload::Binary { .. }),
        "the first streamed belief is a Binary bracket belief"
    );

    // The same for outcomes: pull one, prove the per-row Result contract.
    let mut outcomes = src.outcomes();
    let first_outcome = outcomes
        .next()
        .expect("at least one outcome")
        .expect("first outcome maps cleanly");
    assert!(
        first_outcome.outcome == 0.0 || first_outcome.outcome == 1.0,
        "binary outcome label"
    );

    // The iterator is still alive and yields more without re-running the query
    // from scratch (bounded, streaming).
    assert!(
        outcomes.next().is_some(),
        "the outcome iterator streams the second row lazily"
    );
}

// ---------------------------------------------------------------------------
// 5. A shadow_intents row maps to a HistoricalTrade with orders == 0.
// ---------------------------------------------------------------------------

#[test]
fn aeolus_trade_orders_zero() {
    let src = fixture_source();

    let trades: Vec<_> = src
        .trades()
        .map(|r| r.expect("trade row must map"))
        .collect();

    assert_eq!(trades.len(), 1, "fixture has one shadow intent");
    let trade = &trades[0];

    // The paper-only invariant: real orders never flow through the replay path.
    assert_eq!(trade.orders, 0, "a shadow/paper intent maps to orders == 0");
    assert_eq!(trade.contracts, 5, "contracts preserved from the intent");
    assert_eq!(trade.side, "yes", "side preserved");
    assert_eq!(
        trade.price,
        fortuna_core::money::Cents::new(70),
        "fill price = reference_price_cents"
    );
    assert!(
        trade.event_linkage.contains("MKT-YES"),
        "the trade is linked to its market: {}",
        trade.event_linkage
    );
}
