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

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use fortuna_backtest::asof::{asof_join, AsOfDisposition};
use fortuna_backtest::manifest::{enforce_gdead, ScoredRow};
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

    // And every engaged market (4) is present, voided + pending included.
    assert_eq!(
        manifest.engaged.len(),
        4,
        "all four engaged markets (yes, no, void, pending) must appear in the manifest"
    );

    // The PENDING market (no market_resolutions row) is engaged-but-unresolved:
    // resolved=false AND voided=false. This is the shape G-DEAD must EXEMPT.
    let pending: Vec<_> = manifest
        .engaged
        .iter()
        .filter(|m| !m.resolved && !m.voided)
        .collect();
    assert_eq!(
        pending.len(),
        1,
        "exactly one pending (resolved=false, voided=false) market expected"
    );
    assert!(
        pending[0].event_linkage.contains("MKT-PENDING"),
        "the pending market must be identifiable: {}",
        pending[0].event_linkage
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

// ---------------------------------------------------------------------------
// 6. RECORDS-THROUGH-ASOF-JOIN (the production-observed namespace-drift trap):
//    a scored, CLV-benchmarked sample must genuinely form for a resolved
//    fixture market. The records mapped by `AeolusArchiveSource` are fed into
//    the REAL `crate::asof::asof_join`; for MKT-YES BOTH the outcome AND the
//    CLV-entry snapshot must attach.
//
//    This BITES on the snapshot.market key fix: if snapshots carry the bare
//    ticker instead of the canonical composite `event_linkage`, the as-of join
//    silently drops the snapshot (`snapshot.is_some()` fails) even though the
//    outcome still joins — exactly the namespace-drift failure source.rs warns
//    about. The leak-free `available_at < decided_at` belief (issuance well
//    before the event day) passes G-PIT, and the prior snapshot
//    (2026-07-02, before the 2026-07-04 event day) is eligible.
// ---------------------------------------------------------------------------

#[test]
fn aeolus_records_join_through_asof() {
    let src = fixture_source();

    let beliefs: Vec<_> = src
        .beliefs()
        .map(|r| r.expect("belief row must map"))
        .collect();
    let outcomes: Vec<_> = src
        .outcomes()
        .map(|r| r.expect("outcome row must map"))
        .collect();
    let snapshots: Vec<_> = src
        .snapshots()
        .map(|r| r.expect("snapshot row must map"))
        .collect();

    // The resolved YES market that also has a prior snapshot in the fixture.
    let yes_belief = beliefs
        .iter()
        .find(|b| b.event_linkage.contains("MKT-YES"))
        .expect("the YES-resolved market must yield a belief");

    let disposition = asof_join(yes_belief, &snapshots, &outcomes);

    let ctx = match disposition {
        AsOfDisposition::Joined(ctx) => ctx,
        AsOfDisposition::LookAheadRejected => {
            panic!("the YES belief is leak-free (issuance < event day) and must join")
        }
    };

    // The outcome label must attach (resolved sample → scorable).
    let outcome = ctx
        .outcome
        .as_ref()
        .expect("the resolved YES outcome must attach to the joined context");
    assert_eq!(outcome.outcome, 1.0, "MKT-YES resolves YES → outcome 1.0");

    // The CLV-entry snapshot must attach — this is the Fix-1 bite. With the
    // bare-ticker snapshot key it would be None (silent namespace drift).
    let snapshot = ctx
        .snapshot
        .as_ref()
        .expect("the prior CLV-entry snapshot must attach (snapshot.market join key)");
    assert_eq!(
        snapshot.price,
        fortuna_core::money::Cents::new(70),
        "the joined snapshot is the YES market's 70c mid",
    );
}

// ---------------------------------------------------------------------------
// 7. CROSS-PAGE PAGING CORRECTNESS: the bounded-memory PagedRowStream uses
//    LIMIT/OFFSET, which only yields a well-defined sequence if the ORDER BY is a
//    TOTAL deterministic order. A non-total order (beliefs ordered by a
//    forecast_init_time shared across every row) leaves the row sequence at the
//    mercy of the query plan — and OFFSET paging over an under-determined order
//    risks duplicates/gaps across page boundaries.
//
//    We insert MORE than one page worth of belief rows (PAGE_SIZE is 256, so 300
//    rows cross two page boundaries) — DELIBERATELY all sharing ONE
//    forecast_init_time (the FIRST ORDER BY column is therefore constant) AND
//    DELIBERATELY in an INSERT order that DISAGREES with the total-order
//    tie-break (markers inserted in DESCENDING market_ticker so insertion/rowid
//    order is the reverse of the sorted order). We assert TWO things over the
//    full streamed sequence:
//      (a) the recovered marker SET equals the inserted set exactly (no gap), and
//          the streamed count equals the inserted count (no duplicate); and
//      (b) the streamed SEQUENCE is in the canonical sorted (total) order —
//          ASCENDING market_ticker — NOT the reversed insertion order.
//    (b) is the load-bearing bite: under the TOTAL ORDER BY the tie-break sorts
//    the markers ascending regardless of insertion order; revert the ORDER BY to
//    the non-total `forecast_init_time` only and SQLite falls back to insertion
//    (rowid) order — the reversed sequence — which reds (b). This exercises the
//    REAL production PAGE_SIZE and the REAL paging code (not a test-only page).
// ---------------------------------------------------------------------------

#[test]
fn aeolus_beliefs_are_yes_side_only() {
    // bracket_probability_log stores a yes AND a no row per market (the no-side
    // prob is just 1 - yes); both rows carry the SAME event_linkage (which has no
    // side). Emitting both yields two beliefs that COLLIDE on the join key with
    // complementary p. The belief query filters to side='yes' — the canonical
    // bracket belief — matching Alexandria's yes-side publish contract so the two
    // readers agree on real both-sided data. Before the filter this fixture
    // produced two colliding beliefs; now it produces one.
    let sql = "CREATE TABLE bracket_probability_log (\
        station_id TEXT NOT NULL, target_date TEXT NOT NULL, \
        forecast_init_time TEXT NOT NULL, market_ticker TEXT NOT NULL, \
        side TEXT NOT NULL, bracket_lo INTEGER, bracket_hi INTEGER, \
        predicted_prob REAL NOT NULL, \
        PRIMARY KEY (station_id, target_date, forecast_init_time, market_ticker, side));\n\
        INSERT INTO bracket_probability_log VALUES \
        ('KNYC','2026-07-04','2026-07-01T00:00:00Z','MKT-A','yes',40,44,0.73);\n\
        INSERT INTO bracket_probability_log VALUES \
        ('KNYC','2026-07-04','2026-07-01T00:00:00Z','MKT-A','no',40,44,0.27);\n";
    let path = std::env::temp_dir().join(format!("aeolus_yesside_{}.sql", std::process::id()));
    std::fs::write(&path, sql).expect("write temp fixture");
    let src = AeolusArchiveSource::from_sql_fixture(&path, TimeRange::unbounded())
        .expect("yes-side fixture must load");

    let beliefs: Vec<_> = src
        .beliefs()
        .map(|r| r.expect("belief row must map"))
        .collect();
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        beliefs.len(),
        1,
        "exactly one belief per market — the yes-side row, not both sides"
    );
    match beliefs[0].payload {
        BeliefPayload::Binary { p } => assert_eq!(
            p, 0.73,
            "the surviving belief carries the YES probability, not the no-side 0.27"
        ),
        ref other => panic!("expected a Binary bracket belief, got {other:?}"),
    }
}

#[test]
fn aeolus_paging_total_order_no_dupes_no_gaps() {
    // 300 > PAGE_SIZE (256) → crosses a page boundary (and then some).
    const N: usize = 300;
    const SHARED_INIT: &str = "2026-07-01T00:00:00Z";

    // Minimal schema + N belief rows, all sharing forecast_init_time so the
    // total-order tie-break columns are load-bearing. Unique market_ticker per
    // row is the recovery marker.
    let mut sql = String::new();
    sql.push_str(
        "CREATE TABLE bracket_probability_log (\
            station_id TEXT NOT NULL, target_date TEXT NOT NULL, \
            forecast_init_time TEXT NOT NULL, market_ticker TEXT NOT NULL, \
            side TEXT NOT NULL, bracket_lo INTEGER, bracket_hi INTEGER, \
            predicted_prob REAL NOT NULL, \
            PRIMARY KEY (station_id, target_date, forecast_init_time, market_ticker, side));\n",
    );
    // INSERT in DESCENDING marker order so rowid/insertion order is the REVERSE
    // of the canonical ascending total order — this is what lets the test detect
    // a non-total ORDER BY (which would fall back to insertion order).
    for i in (0..N).rev() {
        writeln!(
            sql,
            "INSERT INTO bracket_probability_log \
             (station_id, target_date, forecast_init_time, market_ticker, side, \
              bracket_lo, bracket_hi, predicted_prob) VALUES \
             ('KNYC', '2026-07-04', '{SHARED_INIT}', 'PAGE-{i:04}', 'yes', 40, 44, 0.5);"
        )
        .expect("write to String never fails");
    }

    // Write to a unique temp file and load via the public fixture loader.
    let path = std::env::temp_dir().join(format!("aeolus_paging_{}_{}.sql", std::process::id(), N));
    std::fs::write(&path, &sql).expect("write temp fixture");
    let src = AeolusArchiveSource::from_sql_fixture(&path, TimeRange::unbounded())
        .expect("paging fixture must load");

    // Stream EVERY belief and collect the ticker markers IN STREAM ORDER.
    let mut seen: Vec<String> = Vec::new();
    for r in src.beliefs() {
        let b = r.expect("belief row must map");
        // strategy_id carries the market_ticker (the PAGE-#### marker).
        seen.push(b.provenance.strategy_id.clone());
    }

    let _ = std::fs::remove_file(&path);

    // (a) NO gaps: every inserted marker was recovered.
    let recovered: HashSet<&str> = seen.iter().map(String::as_str).collect();
    let expected: HashSet<String> = (0..N).map(|i| format!("PAGE-{i:04}")).collect();
    let expected_refs: HashSet<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(
        recovered, expected_refs,
        "every inserted belief marker must be recovered exactly once (no gap) across page boundaries"
    );

    // (a) NO duplicates: total streamed count equals inserted count.
    assert_eq!(
        seen.len(),
        N,
        "streamed count must equal inserted count — no row duplicated or dropped across pages \
         (got {} for {N} inserted)",
        seen.len()
    );

    // (b) THE BITE: the streamed SEQUENCE is the canonical ascending total order,
    // NOT the reversed insertion order. With a non-total ORDER BY SQLite falls
    // back to rowid/insertion order (descending markers) and this reds.
    let sorted: Vec<String> = (0..N).map(|i| format!("PAGE-{i:04}")).collect();
    assert_eq!(
        seen, sorted,
        "the streamed sequence must follow the TOTAL deterministic order (ascending marker), \
         not the under-determined insertion order — a non-total ORDER BY reds this"
    );
}

// ---------------------------------------------------------------------------
// 8. REAL-DATA G-DEAD REGRESSION (the permanent guard for the live-smoke bug):
//    the FIRMED Aeolus manifest mixes RESOLVED (yes/no), VOIDED, and PENDING
//    (engaged belief, NO market_resolutions row) markets — exactly the shape
//    the real archive slice carries (67 pending markets) that false-failed
//    G-DEAD.  We build the scored set the SAME way the harness does — one
//    ScoredRow per RESOLVED market (from the outcomes pool) and one per VOIDED
//    market (from the manifest), and NO row for PENDING markets (they have no
//    outcome and cannot be scored) — then run the REAL `enforce_gdead` against
//    the REAL manifest. It MUST succeed: the pending market is exempt while the
//    resolved + voided markets are covered.
//
//    Then we prove the guard STILL BITES: drop the resolved YES market from the
//    scored set → `enforce_gdead` must report a violation naming it (a resolved
//    market dropped is survivorship, never exempted by the pending rule).
//
//    This test reds if the pending exemption is removed (the pending market
//    would be required and absent → false violation) OR if the exemption is
//    widened to resolved markets (the dropped-resolved sub-case would stop
//    biting). It is the load-bearing regression so the fixture can never again
//    hide the pending case.
// ---------------------------------------------------------------------------

#[test]
fn aeolus_gdead_exempts_pending_but_requires_resolved() {
    let src = fixture_source();

    let manifest = src.universe_manifest().expect("manifest must build");
    let outcomes: Vec<_> = src
        .outcomes()
        .map(|r| r.expect("outcome row must map"))
        .collect();

    // Sanity on the real shape: yes/no resolved + one voided + one pending.
    let resolved_n = manifest.engaged.iter().filter(|m| m.resolved).count();
    let voided_n = manifest.engaged.iter().filter(|m| m.voided).count();
    let pending_n = manifest
        .engaged
        .iter()
        .filter(|m| !m.resolved && !m.voided)
        .count();
    assert_eq!(resolved_n, 2, "MKT-YES + MKT-NO are the resolved markets");
    assert_eq!(voided_n, 1, "MKT-VOID is the one voided market");
    assert_eq!(pending_n, 1, "MKT-PENDING is the one pending market");

    // Build the scored set the SAME way the harness does:
    //   - resolved markets: one ScoredRow each, from the outcomes pool;
    //   - voided markets: one ScoredRow each, from the manifest (voided=true);
    //   - PENDING markets: NO ScoredRow (no outcome → cannot be scored).
    let mut scored: Vec<ScoredRow> = outcomes
        .iter()
        .map(|o| ScoredRow {
            event_linkage: o.event_linkage.clone(),
            outcome: o.outcome,
            voided: false,
        })
        .collect();
    for m in manifest.engaged.iter().filter(|m| m.voided) {
        scored.push(ScoredRow {
            event_linkage: m.event_linkage.clone(),
            outcome: 0.0,
            voided: true,
        });
    }

    // With the pending market deliberately ABSENT from scored, G-DEAD must
    // SUCCEED — the pending market is exempt, the terminal markets are covered.
    assert!(
        enforce_gdead(&scored, &manifest).is_ok(),
        "a manifest with a pending (unresolved) engaged market must pass G-DEAD \
         when every RESOLVED/VOIDED market is scored — the pending market is exempt"
    );

    // THE GUARD STILL BITES: drop the resolved YES market from the scored set.
    // A resolved market dropped is survivorship and must STILL be a violation.
    let yes_linkage = outcomes
        .iter()
        .find(|o| o.outcome == 1.0)
        .map(|o| o.event_linkage.clone())
        .expect("the YES-resolved market must be in outcomes");
    let scored_minus_yes: Vec<ScoredRow> = scored
        .iter()
        .filter(|r| r.event_linkage != yes_linkage)
        .cloned()
        .collect();
    let result = enforce_gdead(&scored_minus_yes, &manifest);
    assert!(
        result.is_err(),
        "dropping a RESOLVED market must STILL fail G-DEAD (survivorship), even \
         though a pending market is present in the same manifest"
    );
    match result.unwrap_err() {
        fortuna_backtest::manifest::GDeadViolation::DroppedMarkets(linkages) => {
            assert!(
                linkages.contains(&yes_linkage),
                "the dropped RESOLVED market must be named in the violation"
            );
            // The pending market must NOT be reported — it is exempt.
            let pending_linkage = manifest
                .engaged
                .iter()
                .find(|m| !m.resolved && !m.voided)
                .map(|m| m.event_linkage.clone())
                .expect("the pending market must be in the manifest");
            assert!(
                !linkages.contains(&pending_linkage),
                "the exempt PENDING market must NOT appear in the violation"
            );
        }
    }
}
