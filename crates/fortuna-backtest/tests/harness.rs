//! Replay-harness tests (S2): idempotent + deterministic + source-stamped +
//! G-PARITY.
//!
//! Written FROM the plan text (S2) and spec §5 G-PARITY / §6 BEFORE the
//! implementation (TDD). These tests drive the REAL ledger write path: each
//! gets an isolated migrated Postgres via `#[sqlx::test]` (the
//! `fortuna-ledger` migrations), so the harness writes through the same
//! `BeliefsRepo`/`ScorecardsRepo` that the live daemon uses.
//!
//! The literal `"historical-import"` is allowed in THIS file (the grep gate
//! only scans `crates/fortuna-backtest/src/`); the harness itself references the
//! ledger-side `SOURCE_HISTORICAL_IMPORT` const so its `src/` stays clean.
//!
//! G-PARITY (the load-bearing claim): a known `(belief, outcome, snapshot)` set
//! scored via (a) the live `fortuna_cognition::scorecard_agg::assemble_from_samples`
//! on the ground-truth `(p, outcome)` set and (b) `ReplayHarness::replay` against
//! an in-memory `HistoricalSource` (which actually streams → as-of-joins → scores)
//! must produce byte-identical `Scorecard`s modulo the `window`/source field. A
//! divergence in replay's pipeline (wrong snapshot picked, a dropped/extra
//! sample, a leaked source field) reds it.

use fortuna_backtest::harness::{ReplayHarness, TimeRange};
use fortuna_backtest::manifest::UniverseManifest;
use fortuna_backtest::records::{
    BeliefPayload, HistoricalBelief, HistoricalOutcome, HistoricalSnapshot, HistoricalTrade,
    Provenance,
};
use fortuna_backtest::source::{HistoricalSource, SourceError};
use fortuna_cognition::scorecard_agg::assemble_from_samples;
use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::money::Cents;
use fortuna_ledger::{BeliefsRepo, ScorecardsRepo, SOURCE_HISTORICAL_IMPORT};
use sqlx::PgPool;

const SCOPE: &str = "weather:DFW";
const PRODUCER: &str = "p1";
const CATEGORY: &str = "weather";
const MIN_N: u32 = 3;

fn ts(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(ms).expect("valid timestamp")
}

/// One synthetic event for the in-memory source: a belief, its prior CLV-entry
/// snapshot, and its post-resolution outcome, all sharing one `event_linkage`.
struct Triple {
    linkage: String,
    p: f64,
    available_ms: i64,
    decided_ms: i64,
    snapshot_price: i64,
    snapshot_ms: i64,
    outcome: f64,
    resolved_ms: i64,
}

/// A deterministic in-memory `HistoricalSource` built from a list of `Triple`s.
/// This is the test double that the G-PARITY / idempotency / determinism tests
/// stream through `ReplayHarness::replay`. It performs NO scoring of its own —
/// the harness must do the as-of join + scoring — so any divergence in the
/// replay pipeline shows up against the ground-truth assembler.
struct MemSource {
    triples: Vec<Triple>,
}

impl MemSource {
    fn provenance(&self) -> Provenance {
        Provenance {
            producer_type: "forecast".to_string(),
            producer_id: PRODUCER.to_string(),
            mind_id: None,
            mind_version: None,
            strategy_id: "s1".to_string(),
            category: CATEGORY.to_string(),
            scope: SCOPE.to_string(),
        }
    }
}

impl HistoricalSource for MemSource {
    fn beliefs(&self) -> Box<dyn Iterator<Item = Result<HistoricalBelief, SourceError>> + '_> {
        let prov = self.provenance();
        Box::new(self.triples.iter().map(move |t| {
            Ok(HistoricalBelief {
                provenance: prov.clone(),
                payload: BeliefPayload::Binary { p: t.p },
                event_linkage: t.linkage.clone(),
                available_at: ts(t.available_ms),
                decided_at: ts(t.decided_ms),
            })
        }))
    }

    fn outcomes(&self) -> Box<dyn Iterator<Item = Result<HistoricalOutcome, SourceError>> + '_> {
        Box::new(self.triples.iter().map(|t| {
            Ok(HistoricalOutcome {
                event_linkage: t.linkage.clone(),
                outcome: t.outcome,
                resolved_at: ts(t.resolved_ms),
                resolution_source: "settlement".to_string(),
            })
        }))
    }

    fn snapshots(&self) -> Box<dyn Iterator<Item = Result<HistoricalSnapshot, SourceError>> + '_> {
        Box::new(self.triples.iter().map(|t| {
            Ok(HistoricalSnapshot {
                market: t.linkage.clone(),
                price: Cents::new(t.snapshot_price),
                at: ts(t.snapshot_ms),
            })
        }))
    }

    fn trades(&self) -> Box<dyn Iterator<Item = Result<HistoricalTrade, SourceError>> + '_> {
        Box::new(std::iter::empty())
    }

    fn universe_manifest(&self) -> Result<UniverseManifest, SourceError> {
        Ok(UniverseManifest {
            engaged: Vec::new(),
        })
    }
}

/// Four resolved binary samples with a known `(p, outcome)` ground truth. The
/// snapshot prices and timestamps are picked so the as-of join is unambiguous
/// (one prior snapshot per event). decided_ms strictly after available_ms so
/// every belief passes G-PIT.
fn four_triples() -> Vec<Triple> {
    let base = 10_000_000_i64;
    let specs = [
        (0.10, false_o(), 31),
        (0.20, false_o(), 33),
        (0.80, true_o(), 60),
        (0.90, true_o(), 70),
    ];
    specs
        .iter()
        .enumerate()
        .map(|(i, (p, outcome, price))| {
            let d = base + (i as i64) * 1_000;
            Triple {
                linkage: format!("event://forecast/station-DFW/bracket-{i}/2026-07-04"),
                p: *p,
                available_ms: d - 100,
                decided_ms: d,
                snapshot_price: *price,
                snapshot_ms: d - 10, // strictly before decided → eligible
                outcome: *outcome,
                resolved_ms: d + 1_000_000,
            }
        })
        .collect()
}

fn true_o() -> f64 {
    1.0
}
fn false_o() -> f64 {
    0.0
}

fn full_range() -> TimeRange {
    TimeRange {
        from: ts(0),
        to: ts(i64::from(u32::MAX) * 1_000),
    }
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn replay_is_idempotent(pool: PgPool) {
    let source = MemSource {
        triples: four_triples(),
    };
    let clock = SimClock::new(ts(20_000_000));
    let harness = ReplayHarness::new(pool.clone(), clock, MIN_N);

    let first = harness
        .replay(&source, full_range())
        .await
        .expect("first replay");
    assert!(first.written > 0, "first replay writes the four beliefs");
    assert_eq!(
        first.skipped_idempotent, 0,
        "first replay skips nothing (empty ledger)"
    );

    let second = harness
        .replay(&source, full_range())
        .await
        .expect("second replay");
    assert_eq!(
        second.written, 0,
        "the second replay writes ZERO new rows (content-hash / ON CONFLICT)"
    );
    assert_eq!(
        second.skipped_idempotent, first.written,
        "every row the first run wrote is skipped as idempotent on the second"
    );

    // Hard floor: the beliefs table holds exactly `first.written` rows, not 2x.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM beliefs")
        .fetch_one(&pool)
        .await
        .expect("count beliefs");
    assert_eq!(
        count, first.written as i64,
        "no duplicate belief rows after a re-run"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn replay_is_deterministic(pool: PgPool) {
    // Same source + same injected SimClock → identical ledger rows. Two fresh
    // databases (two #[sqlx::test] would be separate; here we use one pool but
    // replay into it once, capture the belief_ids, then prove a second harness
    // with the SAME clock start computes the SAME ids → all skipped).
    let source = MemSource {
        triples: four_triples(),
    };
    let harness_a = ReplayHarness::new(pool.clone(), SimClock::new(ts(20_000_000)), MIN_N);
    let report_a = harness_a
        .replay(&source, full_range())
        .await
        .expect("replay a");

    let ids_a: Vec<String> = sqlx::query_scalar("SELECT belief_id FROM beliefs ORDER BY belief_id")
        .fetch_all(&pool)
        .await
        .expect("ids a");

    // A second harness, identical clock, identical source. Because ids are
    // content-derived (NOT wall-clock), every id collides → ON CONFLICT skips
    // all → written == 0, and the id set is byte-identical.
    let harness_b = ReplayHarness::new(pool.clone(), SimClock::new(ts(20_000_000)), MIN_N);
    let report_b = harness_b
        .replay(&source, full_range())
        .await
        .expect("replay b");
    assert_eq!(
        report_b.written, 0,
        "a deterministic re-run produces the SAME content-derived ids → all conflict"
    );
    assert_eq!(report_a.written, report_b.skipped_idempotent);

    let ids_b: Vec<String> = sqlx::query_scalar("SELECT belief_id FROM beliefs ORDER BY belief_id")
        .fetch_all(&pool)
        .await
        .expect("ids b");
    assert_eq!(
        ids_a, ids_b,
        "the ledger belief_id set is identical across deterministic replays"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn replay_stamps_historical_import(pool: PgPool) {
    let source = MemSource {
        triples: four_triples(),
    };
    let harness = ReplayHarness::new(pool.clone(), SimClock::new(ts(20_000_000)), MIN_N);
    harness.replay(&source, full_range()).await.expect("replay");

    // Every written belief row carries provenance.source == "historical-import".
    let sources: Vec<Option<String>> =
        sqlx::query_scalar("SELECT provenance->>'source' FROM beliefs")
            .fetch_all(&pool)
            .await
            .expect("provenance sources");
    assert!(
        !sources.is_empty(),
        "the replay wrote at least one belief row"
    );
    for s in &sources {
        assert_eq!(
            s.as_deref(),
            Some(SOURCE_HISTORICAL_IMPORT),
            "every replayed row is source-stamped historical-import"
        );
    }

    // Original timestamps preserved: created_at equals the ORIGINAL decided_at
    // (not the wall clock / SimClock now). We read created_at back and assert it
    // equals one of the source decided_at values, never the harness clock.
    let created_ats: Vec<String> = sqlx::query_scalar("SELECT created_at FROM beliefs")
        .fetch_all(&pool)
        .await
        .expect("created_ats");
    let expected: std::collections::HashSet<String> = four_triples()
        .iter()
        .map(|t| ts(t.decided_ms).to_iso8601())
        .collect();
    for c in &created_ats {
        assert!(
            expected.contains(c),
            "created_at {c} must be a PRESERVED original decided_at, not the wall/sim clock"
        );
    }
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn parity_seam_backtest_equals_live(pool: PgPool) {
    // G-PARITY: the backtest replay scores IDENTICALLY to the live path.
    //
    // (a) LIVE: assemble a Scorecard from the ground-truth (p, outcome) set with
    //     the live `assemble_from_samples` (the same fn the daemon's
    //     recompute_scorecards path uses).
    // (b) BACKTEST: run ReplayHarness::replay against the in-memory source, which
    //     STREAMS → as-of-joins → scores via the SAME assembler internally, and
    //     surfaces the produced Scorecard in the report.
    //
    // Assert (a) and (b) are byte-identical modulo the `window`/source field.
    let triples = four_triples();

    // Ground-truth samples in event order. baseline_losses / clv are empty here
    // (no de-vig baseline modeled in S2); the parity claim is that BOTH paths
    // feed the assembler the SAME inputs, so both must omit them identically.
    let samples: Vec<(f64, bool)> = triples.iter().map(|t| (t.p, t.outcome >= 0.5)).collect();
    let live = assemble_from_samples(SCOPE, Some(PRODUCER), "forward", &samples, &[], &[], MIN_N);

    let source = MemSource {
        triples: four_triples(),
    };
    let harness = ReplayHarness::new(pool.clone(), SimClock::new(ts(20_000_000)), MIN_N);
    let report = harness.replay(&source, full_range()).await.expect("replay");

    let mut backtest = report
        .scorecard
        .expect("replay produced a parity scorecard for the scope");

    // The ONLY permitted difference is the window/source label.
    assert_ne!(
        backtest.window, live.window,
        "precondition: the backtest window is the historical-import label, the live one is 'forward'"
    );
    assert_eq!(
        backtest.window, SOURCE_HISTORICAL_IMPORT,
        "the replay stamps the scorecard window with the historical-import source label"
    );
    backtest.window = live.window.clone();

    assert_eq!(
        backtest, live,
        "G-PARITY: the replay-produced Scorecard is byte-identical to the live one \
         modulo the window/source label"
    );

    // Cross-check against the persisted card too: the ledger write path stored
    // the same parity card under the historical-import window.
    let repo = ScorecardsRepo::new(pool.clone());
    let stored = repo
        .latest_scorecard(SCOPE, Some(PRODUCER), SOURCE_HISTORICAL_IMPORT)
        .await
        .expect("latest_scorecard")
        .expect("the parity scorecard was persisted");
    let mut stored_norm = stored;
    stored_norm.window = live.window.clone();
    assert_eq!(
        stored_norm, live,
        "the persisted scorecard also matches the live card modulo the window label"
    );

    // Also confirm beliefs were written (the replay drives the real ledger path,
    // not a twin assemble-only call).
    let belief_repo = BeliefsRepo::new(pool.clone());
    let _ = &belief_repo; // constructed to assert the repo type is the live one
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM beliefs")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(
        count, 4,
        "the replay wrote the four beliefs through the ledger"
    );
}
