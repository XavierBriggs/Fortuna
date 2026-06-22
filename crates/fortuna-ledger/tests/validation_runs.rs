//! WS3 S5 tests: the append-only `validation_runs` store + `ValidationRunsRepo`.
//!
//! Written FROM the plan text (S5) + spec §7 BEFORE the implementation (TDD).
//! `validation_runs` is an APPEND-ONLY store of the deflated G-TRUTH GO surface
//! (one immutable row per sweep), mirroring the `scorecards` pattern (a re-run is
//! a NEW row, never an edit). Coverage, adversarially:
//!   - insert -> read-back: `latest` returns the EXACT ValidationRun, the JSONB
//!     payload round-tripping every G-TRUTH field (serde equality);
//!   - newest-wins: two runs for the SAME (scope, producer) at different
//!     `computed_at` -> `latest` returns the newer one;
//!   - the DB-level append-only guard: a raw UPDATE and a raw DELETE on a
//!     validation_runs row are both refused by `fortuna_refuse_mutation` (the
//!     scorecards immutability proof);
//!   - absent key -> `None`.
//!
//! Each test gets an isolated, migrated database via #[sqlx::test].

use fortuna_ledger::ValidationRunsRepo;
use fortuna_scoring::GoDecision;
use serde_json::json;
use sqlx::PgPool;

/// A representative serialized ValidationRun payload for one (scope, producer).
/// The repo treats the payload as opaque JSONB (the typed `ValidationRun` lives
/// in `fortuna-backtest`, which the ledger does not depend on), so the test
/// constructs the whole-truth surface directly as JSON.
fn sample_payload(verdict: &str, family_n_trials: i64) -> serde_json::Value {
    json!({
        "run_id": "01VALIDRUN000000000000000A",
        "scope": "weather:KNYC",
        "producer": "aeolus",
        "trial_space": {
            "calibration_windows": [30, 60],
            "recal_methods": ["platt", "isotonic"],
            "scopes": ["weather:KNYC"],
            "go_thresholds": [0.5, 0.55]
        },
        "n_trials": 8,
        "family_n_trials": family_n_trials,
        "selected_config": { "calibration_window": 60, "recal_method": "platt", "go_threshold": 0.55 },
        "brier_edge": 0.04,
        "brier_pbo": 0.01,
        "brier_spa_p": 0.01,
        "clv_edge": 0.0,
        "clv_pbo": 1.0,
        "clv_spa_p": 1.0,
        "effective_n": 120.0,
        "mintrl_ok": true,
        "sharpe_dsr": 0.99,
        "verdict": verdict,
        "computed_at": "2026-06-21T00:00:00.000Z"
    })
}

#[sqlx::test(migrations = "./migrations")]
async fn insert_then_latest_round_trips_the_run(pool: PgPool) {
    let repo = ValidationRunsRepo::new(pool.clone());
    let payload = sample_payload("go", 24);

    repo.insert(
        "01VALIDRUN000000000000000A",
        "weather:KNYC",
        Some("aeolus"),
        &payload,
        "2026-06-21T00:00:00.000Z",
    )
    .await
    .expect("insert validation_run");

    let got = repo
        .latest("weather:KNYC", Some("aeolus"))
        .await
        .expect("latest")
        .expect("a validation_run is present after insert");

    assert_eq!(got, payload, "latest returns the exact inserted payload");
    // The whole-truth verdict survived the JSONB round-trip.
    assert_eq!(got["verdict"], json!("go"));
    assert_eq!(got["family_n_trials"], json!(24));
}

#[sqlx::test(migrations = "./migrations")]
async fn latest_returns_the_newest_computed_at(pool: PgPool) {
    let repo = ValidationRunsRepo::new(pool.clone());

    repo.insert(
        "01VALIDRUN0000000000000010",
        "weather:KNYC",
        Some("aeolus"),
        &sample_payload("no_go", 8),
        "2026-06-20T00:00:00.000Z",
    )
    .await
    .expect("insert older");
    repo.insert(
        "01VALIDRUN0000000000000011",
        "weather:KNYC",
        Some("aeolus"),
        &sample_payload("go", 24),
        "2026-06-21T00:00:00.000Z",
    )
    .await
    .expect("insert newer");

    let got = repo
        .latest("weather:KNYC", Some("aeolus"))
        .await
        .expect("latest")
        .expect("present");
    assert_eq!(
        got["verdict"],
        json!("go"),
        "the newest computed_at run wins"
    );
    assert_eq!(got["family_n_trials"], json!(24));
}

#[sqlx::test(migrations = "./migrations")]
async fn latest_absent_key_is_none(pool: PgPool) {
    let repo = ValidationRunsRepo::new(pool.clone());
    let got = repo.latest("nope", Some("nobody")).await.expect("latest");
    assert!(got.is_none(), "an absent (scope, producer) reads None");
}

#[sqlx::test(migrations = "./migrations")]
async fn validation_runs_append_only(pool: PgPool) {
    let repo = ValidationRunsRepo::new(pool.clone());
    repo.insert(
        "01VALIDRUN0000000000000030",
        "weather:KNYC",
        Some("aeolus"),
        &sample_payload("go", 24),
        "2026-06-21T00:00:00.000Z",
    )
    .await
    .expect("insert");

    // A raw UPDATE is refused by the append-only trigger (I5).
    let upd = sqlx::query("UPDATE validation_runs SET verdict = 'tampered' WHERE run_id = $1")
        .bind("01VALIDRUN0000000000000030")
        .execute(&pool)
        .await;
    assert!(
        upd.is_err(),
        "UPDATE on a validation_runs row is refused (append-only)"
    );

    // A raw DELETE is refused too.
    let del = sqlx::query("DELETE FROM validation_runs WHERE run_id = $1")
        .bind("01VALIDRUN0000000000000030")
        .execute(&pool)
        .await;
    assert!(
        del.is_err(),
        "DELETE on a validation_runs row is refused (append-only)"
    );

    // The row is still readable, untouched.
    let got = repo
        .latest("weather:KNYC", Some("aeolus"))
        .await
        .expect("latest")
        .expect("present");
    assert_eq!(
        got["verdict"],
        json!("go"),
        "the row survived the refused mutations"
    );
    // The verdict deserializes back to a real GoDecision (whole-truth contract).
    let verdict: GoDecision =
        serde_json::from_value(got["verdict"].clone()).expect("verdict is a GoDecision");
    assert_eq!(verdict, GoDecision::Go);
}
