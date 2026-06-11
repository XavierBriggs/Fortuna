//! T4.1 hard requirements 3 + 4 (kickoff) — the two standing GAPS
//! residue lines, closed by composition code that actually runs:
//!   req 3: the degrade-alert SCRAPE-DELTA consumer (diff counters per
//!     scrape, feed fortuna_ops::alerts::degrade_alerts);
//!   req 4: CalibrationParamsRepo.latest + BeliefsRepo.resolved_stats ->
//!     CalibrationContext + calibration_quality for the synthesis scope.
//! Written red-first against a compose module that did not exist. The
//! full strategy-through-daemon flow is the requirement-10 DST smoke;
//! these tests pin the fetch->build->feed seams themselves.

use fortuna_ledger::{BeliefsRepo, CalibrationParamsRepo};
use fortuna_live::compose::{calibration_for_scope, DegradeScrape};
use fortuna_ops::alerts::DegradeThresholds;
use sqlx::PgPool;

#[test]
fn degrade_scrape_diffs_totals_and_alerts_once() {
    let mut scrape = DegradeScrape::new(DegradeThresholds {
        failure_alert_threshold: 3,
    });

    // First scrape: two budget breaches since boot -> alerts (every
    // breach alerts); two failures (below threshold 3) -> silent.
    let alerts = scrape.scrape(2, 2);
    assert_eq!(
        alerts.len(),
        1,
        "breaches alert, sub-threshold failures do not: {alerts:?}"
    );
    assert!(
        alerts[0].1.contains('2'),
        "alert carries the scrape count: {alerts:?}"
    );

    // Same totals again: deltas are zero -> nothing re-alerts.
    let alerts = scrape.scrape(2, 2);
    assert!(alerts.is_empty(), "no deltas, no alerts: {alerts:?}");

    // Failure burst at/over threshold -> failure alert fires.
    let alerts = scrape.scrape(2, 6);
    assert_eq!(alerts.len(), 1, "{alerts:?}");

    // Counter RESET (process restart wrote fresh counters): saturating
    // diff must not underflow or false-alert.
    let alerts = scrape.scrape(0, 0);
    assert!(alerts.is_empty(), "reset is not a burst: {alerts:?}");
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn calibration_scope_with_no_params_is_none_and_quality_zero(pool: PgPool) {
    let params = CalibrationParamsRepo::new(pool.clone());
    let beliefs = BeliefsRepo::new(pool.clone());
    let (ctx, quality) = calibration_for_scope(
        &params,
        &beliefs,
        "claude-fable-5",
        "synth_events",
        "weather",
        "platt",
    )
    .await
    .unwrap();
    assert!(
        ctx.is_none(),
        "no params row -> None (the strategy prices no edge; fail closed)"
    );
    assert_eq!(
        quality, 0.0,
        "no resolved history -> zero quality -> sizes zero"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn calibration_scope_builds_context_and_quality_from_the_ledger(pool: PgPool) {
    let params = CalibrationParamsRepo::new(pool.clone());
    let beliefs = BeliefsRepo::new(pool.clone());

    // Seed the scope's params row (identity-ish platt) via the repo.
    let p = serde_json::json!({
        "version": 1,
        "method": { "Platt": { "a": 0.0, "b": 1.0 } },
        "extremization_k": 1.0,
        "fitted_on_n": 10
    });
    params
        .insert(
            "01PARAM0000000000000000001",
            "claude-fable-5",
            "synth_events",
            "weather",
            "platt",
            &p,
            1,
            "2026-06-11T00:00:00.000Z",
            "2026-06-11T00:00:00.000Z",
        )
        .await
        .unwrap();

    // Seed resolved, scoreable history: an event + well-calibrated
    // resolved beliefs (p=0.7 resolving true ~70% of the time).
    sqlx::query(
        "INSERT INTO events (event_id, statement, resolution_criteria,
                             resolution_source, benchmark_at, category,
                             unscoreable, created_at)
         VALUES ('01EVENT000000000000000001', 'seed', 'seed', 'nws',
                 '2026-06-12T00:00:00.000Z', 'weather', FALSE,
                 '2026-06-11T00:00:00.000Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    for i in 0..10 {
        let outcome: i32 = if i < 7 { 1 } else { 0 };
        let brier = if outcome == 1 {
            (1.0f64 - 0.7).powi(2)
        } else {
            (0.7f64).powi(2)
        };
        sqlx::query(
            "INSERT INTO beliefs (belief_id, event_id, p, p_raw, horizon, status,
                                  outcome, brier, evidence, provenance, created_at)
             VALUES ($1, '01EVENT000000000000000001', 0.7, 0.7,
                     '2026-06-12T00:00:00.000Z', 'resolved', $2, $3,
                     '[]'::jsonb, '{}'::jsonb, $4)",
        )
        .bind(format!("01BELIEF0000000000000000{i:02}"))
        .bind(outcome)
        .bind(brier)
        .bind(format!("2026-06-11T00:00:{i:02}.000Z"))
        .execute(&pool)
        .await
        .unwrap();
    }

    let (ctx, quality) = calibration_for_scope(
        &params,
        &beliefs,
        "claude-fable-5",
        "synth_events",
        "weather",
        "platt",
    )
    .await
    .unwrap();
    let ctx = ctx.expect("seeded params row -> Some(context)");
    assert_eq!(ctx.resolved_n, 10, "resolved_n counts the scored history");
    assert!(
        quality > 0.0 && quality <= 1.0,
        "well-calibrated history yields positive ramped quality (got {quality})"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn corrupt_params_row_is_a_loud_error_never_a_silent_none(pool: PgPool) {
    let params = CalibrationParamsRepo::new(pool.clone());
    let beliefs = BeliefsRepo::new(pool.clone());
    params
        .insert(
            "01PARAM0000000000000000002",
            "claude-fable-5",
            "synth_events",
            "weather",
            "platt",
            &serde_json::json!({"not": "a params shape"}),
            1,
            "2026-06-11T00:00:00.000Z",
            "2026-06-11T00:00:00.000Z",
        )
        .await
        .unwrap();
    let result = calibration_for_scope(
        &params,
        &beliefs,
        "claude-fable-5",
        "synth_events",
        "weather",
        "platt",
    )
    .await;
    assert!(
        result.is_err(),
        "a params row that does not parse is corrupt config: loud error, \
         never silently treated as uncalibrated"
    );
}
