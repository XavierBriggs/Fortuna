//! WS2 S6b tests: the append-only `scorecards` snapshot store + repo.
//!
//! Written FROM the plan text (Task 6 Step 5 + the V&V-2 advisory) BEFORE the
//! implementation (TDD). The `scorecards` table is an APPEND-ONLY snapshot of the
//! pure `fortuna_scoring::Scorecard` (one immutable row per recompute), mirroring
//! the `scalar_beliefs`/`belief_scores` append-only posture (a recompute is a NEW
//! row, never an edit). Coverage, adversarially:
//!   - insert -> read-back: `latest_scorecard` returns the EXACT Scorecard,
//!     payload JSONB round-tripping every field (serde equality);
//!   - newest-wins: two snapshots of the SAME (scope, producer, window) at
//!     different `computed_at` -> `latest_scorecard` returns the newer one;
//!   - producer scoping: `latest_scorecard(scope, None, window)` and
//!     `latest_scorecard(scope, Some(p), window)` are distinct rows (the UNIQUE
//!     key includes producer, and NULL producer is its own bucket);
//!   - absent key -> `None`;
//!   - the DB-level append-only guard: a raw UPDATE and a raw DELETE on a
//!     scorecards row are both refused by the trigger (mirrors the scalar_beliefs
//!     immutability proof).
//!
//! Each test gets an isolated, migrated database via #[sqlx::test].

use fortuna_ledger::ScorecardsRepo;
use fortuna_scoring::{assemble_scorecard, CalibrationSample, GoDecision, Scorecard};
use sqlx::PgPool;

/// A representative Scorecard for the weather demo scope: enough samples to be
/// `Go` (model Brier strictly beats the baseline), with CLV and a DM test so the
/// JSONB payload exercises the `Option`/nested-struct fields.
fn sample_scorecard(window: &str) -> Scorecard {
    // Model is well-calibrated on these 6 binary samples; the baseline losses are
    // deliberately worse so the GO is `Go` and DM has a real differential.
    let samples = vec![
        CalibrationSample {
            p: 0.1,
            outcome: false,
        },
        CalibrationSample {
            p: 0.2,
            outcome: false,
        },
        CalibrationSample {
            p: 0.8,
            outcome: true,
        },
        CalibrationSample {
            p: 0.9,
            outcome: true,
        },
        CalibrationSample {
            p: 0.3,
            outcome: false,
        },
        CalibrationSample {
            p: 0.7,
            outcome: true,
        },
    ];
    // Per-sample baseline (market) Brier losses — uniformly worse than the model.
    let baseline_losses = vec![0.25, 0.25, 0.25, 0.25, 0.25, 0.25];
    let baseline_brier = baseline_losses.iter().sum::<f64>() / baseline_losses.len() as f64;
    let clv = vec![12.0, 8.0, 15.0];
    assemble_scorecard(
        "weather:KNYC",
        Some("aeolus"),
        window,
        &samples,
        baseline_brier,
        Some(&baseline_losses),
        None, // rps (binary scope)
        Some(0.30),
        0,    // log_tail_events
        None, // crps
        &clv,
        Vec::new(), // pit_bins
        3,          // min_n
    )
}

#[sqlx::test(migrations = "./migrations")]
async fn insert_then_latest_round_trips_the_scorecard(pool: PgPool) {
    let repo = ScorecardsRepo::new(pool.clone());
    let card = sample_scorecard("forward");
    // Sanity: the fixture is the `Go` path so the round-trip exercises a full card.
    assert_eq!(card.go.decision, GoDecision::Go);

    repo.insert_scorecard(
        "01SCORECARD0000000000000001",
        &card,
        "2026-06-21T00:00:00.000Z",
    )
    .await
    .expect("insert_scorecard");

    let got = repo
        .latest_scorecard("weather:KNYC", Some("aeolus"), "forward")
        .await
        .expect("latest_scorecard")
        .expect("a scorecard is present after insert");

    // The whole Scorecard round-trips through the JSONB payload verbatim.
    assert_eq!(
        got, card,
        "latest_scorecard returns the exact inserted Scorecard"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn latest_scorecard_returns_the_newest_computed_at(pool: PgPool) {
    let repo = ScorecardsRepo::new(pool.clone());
    let older = sample_scorecard("forward");
    let mut newer = sample_scorecard("forward");
    // Mutate a field so the two snapshots are distinguishable on read-back.
    newer.n = 99;

    repo.insert_scorecard(
        "01SCORECARD0000000000000010",
        &older,
        "2026-06-20T00:00:00.000Z",
    )
    .await
    .expect("insert older");
    repo.insert_scorecard(
        "01SCORECARD0000000000000011",
        &newer,
        "2026-06-21T00:00:00.000Z",
    )
    .await
    .expect("insert newer");

    let got = repo
        .latest_scorecard("weather:KNYC", Some("aeolus"), "forward")
        .await
        .expect("latest_scorecard")
        .expect("present");
    assert_eq!(got.n, 99, "the newest computed_at snapshot wins");
}

#[sqlx::test(migrations = "./migrations")]
async fn producer_none_and_some_are_distinct_buckets(pool: PgPool) {
    let repo = ScorecardsRepo::new(pool.clone());
    // A producer-attributed card and a merged-scope (producer = None) card share
    // scope+window but are distinct rows keyed on producer (NULL is its own bucket).
    let attributed = sample_scorecard("forward");
    let merged = assemble_scorecard(
        "weather:KNYC",
        None,
        "forward",
        &[CalibrationSample {
            p: 0.5,
            outcome: true,
        }],
        0.25,
        None,
        None,
        None,
        0,
        None,
        &[],
        Vec::new(),
        3,
    );

    repo.insert_scorecard(
        "01SCORECARD0000000000000020",
        &attributed,
        "2026-06-21T00:00:00.000Z",
    )
    .await
    .expect("insert attributed");
    repo.insert_scorecard(
        "01SCORECARD0000000000000021",
        &merged,
        "2026-06-21T00:00:00.000Z",
    )
    .await
    .expect("insert merged");

    let got_attr = repo
        .latest_scorecard("weather:KNYC", Some("aeolus"), "forward")
        .await
        .expect("latest attributed")
        .expect("present");
    assert_eq!(got_attr.producer.as_deref(), Some("aeolus"));

    let got_merged = repo
        .latest_scorecard("weather:KNYC", None, "forward")
        .await
        .expect("latest merged")
        .expect("present");
    assert_eq!(
        got_merged.producer, None,
        "the producer=None bucket is distinct"
    );
    assert_eq!(got_merged.n, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn latest_scorecard_absent_key_is_none(pool: PgPool) {
    let repo = ScorecardsRepo::new(pool.clone());
    let got = repo
        .latest_scorecard("nope", Some("nobody"), "forward")
        .await
        .expect("latest_scorecard");
    assert!(
        got.is_none(),
        "an absent (scope, producer, window) reads None"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn scorecards_are_append_only_update_and_delete_refused(pool: PgPool) {
    let repo = ScorecardsRepo::new(pool.clone());
    let card = sample_scorecard("forward");
    repo.insert_scorecard(
        "01SCORECARD0000000000000030",
        &card,
        "2026-06-21T00:00:00.000Z",
    )
    .await
    .expect("insert");

    // A raw UPDATE is refused by the append-only trigger (I5).
    let upd = sqlx::query("UPDATE scorecards SET scope = 'tampered' WHERE id = $1")
        .bind("01SCORECARD0000000000000030")
        .execute(&pool)
        .await;
    assert!(
        upd.is_err(),
        "UPDATE on a scorecards row is refused (append-only)"
    );

    // A raw DELETE is refused too.
    let del = sqlx::query("DELETE FROM scorecards WHERE id = $1")
        .bind("01SCORECARD0000000000000030")
        .execute(&pool)
        .await;
    assert!(
        del.is_err(),
        "DELETE on a scorecards row is refused (append-only)"
    );

    // The row is still readable, untouched.
    let got = repo
        .latest_scorecard("weather:KNYC", Some("aeolus"), "forward")
        .await
        .expect("latest")
        .expect("present");
    assert_eq!(
        got.scope, "weather:KNYC",
        "the row survived the refused mutations"
    );
}
