//! WS1 Task 7 — Best-calibrated producer selection (TDD — tests written BEFORE
//! implementation per DoD).
//!
//! These integration tests cover the THESIS PAYOFF: synthesis prices the
//! best-calibrated producer, chosen data-driven from the ledger. No producer
//! literal ("aeolus", "meteorologist", etc.) appears anywhere in the selection
//! helper — producers are DATA.
//!
//! Tests:
//!   1. Better-producer wins: producer A calibrated better than B → A is chosen.
//!   2. Flip: make B better → B wins. (Proves data-driven, not hardcoded.)
//!   3. Tie (equal quality) → producer-id ASC.
//!   4. Cold-start (no resolved beliefs) → (None, None, None).
//!   5. Regression: calibration_for_scope(None) is back-compat.
//!
//! All DB-backed (sqlx::test with the ledger migrations).

use fortuna_ledger::{BeliefsRepo, CalibrationParamsRepo};
use fortuna_live::compose::{best_calibrated_producer, calibration_for_scope};
use serde_json::json;
use sqlx::PgPool;

const CATEGORY: &str = "weather";

// ─── helpers ──────────────────────────────────────────────────────────────────

async fn seed_event(pool: &PgPool, event_id: &str) {
    sqlx::query(
        "INSERT INTO events (event_id, statement, resolution_criteria,
                             resolution_source, benchmark_at, category,
                             unscoreable, created_at)
         VALUES ($1, 'stmt', 'crit', 'nws',
                 '2026-06-20T00:00:00.000Z', 'weather', FALSE,
                 '2026-06-19T00:00:00.000Z')",
    )
    .bind(event_id)
    .execute(pool)
    .await
    .unwrap();
}

/// Seed `n` resolved beliefs for `producer` in `event_id`.
/// `good=true`  → p=0.9, outcome=true  (well-calibrated, low Brier ≈ 0.01)
/// `good=false` → p=0.1, outcome=true  (poorly calibrated, high Brier ≈ 0.81)
async fn seed_resolved_beliefs(
    pool: &PgPool,
    event_id: &str,
    producer: &str,
    belief_id_prefix: &str, // 6-char prefix to keep belief_id unique across calls
    n: usize,
    good: bool,
) {
    let repo = BeliefsRepo::new(pool.clone());
    let p = if good { 0.9_f64 } else { 0.1_f64 };
    let brier: f64 = (1.0_f64 - p).powi(2);
    for i in 0..n {
        // Build a 26-char ULID-shaped id: 6-char prefix + 20-char suffix
        let belief_id = format!("{}{:020}", belief_id_prefix, i);
        let ts = format!("2026-06-19T10:{:02}:{:02}.000Z", i / 60, i % 60);
        let provenance = json!({ "producer": producer, "model_id": "test-model" });
        repo.insert(
            &belief_id,
            &ts,
            event_id,
            p,
            p,
            "2026-06-20T00:00:00.000Z",
            &json!({"reasoning": "test"}),
            &provenance,
            None,
        )
        .await
        .unwrap();
        // resolve + score via direct SQL (same pattern as per_producer_queries.rs)
        let outcome_i32: i32 = 1; // always resolves true
        sqlx::query(
            "UPDATE beliefs SET status='resolved', outcome=$1, brier=$2
             WHERE belief_id=$3",
        )
        .bind(outcome_i32)
        .bind(brier)
        .bind(&belief_id)
        .execute(pool)
        .await
        .unwrap();
    }
}

// ─── Test 1: better producer wins ────────────────────────────────────────────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn better_calibrated_producer_is_chosen(pool: PgPool) {
    // Event A → "bravo" (well-calibrated); Event B → "alpha" (poorly calibrated).
    seed_event(&pool, "01EVTBPA000000000000000001").await;
    seed_event(&pool, "01EVTBPB000000000000000001").await;

    seed_resolved_beliefs(
        &pool,
        "01EVTBPA000000000000000001",
        "bravo",
        "BLFA00", // 6-char prefix
        15,
        true, // well-calibrated
    )
    .await;
    seed_resolved_beliefs(
        &pool,
        "01EVTBPB000000000000000001",
        "alpha",
        "BLFB00", // distinct 6-char prefix
        15,
        false, // poorly calibrated
    )
    .await;

    let params = CalibrationParamsRepo::new(pool.clone());
    let beliefs = BeliefsRepo::new(pool.clone());
    let (winner, _ctx, _quality) = best_calibrated_producer(
        &params,
        &beliefs,
        "test-model",
        "synth_events",
        CATEGORY,
        "platt",
    )
    .await
    .unwrap();

    assert_eq!(
        winner.as_deref(),
        Some("bravo"),
        "best-calibrated producer must win; got {:?}",
        winner
    );
}

// ─── Test 2: flip → other producer wins ──────────────────────────────────────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn flip_better_calibrated_picks_the_other_producer(pool: PgPool) {
    // Flip: now "alpha" is well-calibrated; "bravo" is poorly calibrated.
    seed_event(&pool, "01EVTFPA000000000000000001").await;
    seed_event(&pool, "01EVTFPB000000000000000001").await;

    seed_resolved_beliefs(
        &pool,
        "01EVTFPA000000000000000001",
        "alpha",
        "BLFC00",
        15,
        true, // well-calibrated this time
    )
    .await;
    seed_resolved_beliefs(
        &pool,
        "01EVTFPB000000000000000001",
        "bravo",
        "BLFD00",
        15,
        false, // poorly calibrated this time
    )
    .await;

    let params = CalibrationParamsRepo::new(pool.clone());
    let beliefs = BeliefsRepo::new(pool.clone());
    let (winner, _ctx, _quality) = best_calibrated_producer(
        &params,
        &beliefs,
        "test-model",
        "synth_events",
        CATEGORY,
        "platt",
    )
    .await
    .unwrap();

    assert_eq!(
        winner.as_deref(),
        Some("alpha"),
        "flipped seeds must pick the OTHER producer (data-driven, not hardcoded); got {:?}",
        winner
    );
}

// ─── Test 3: tie → producer-id ASC ───────────────────────────────────────────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn tie_in_quality_breaks_by_producer_id_asc(pool: PgPool) {
    // Both producers get IDENTICAL belief patterns → equal quality.
    seed_event(&pool, "01EVTTIA000000000000000001").await;
    seed_event(&pool, "01EVTTIB000000000000000001").await;

    // Both "well calibrated" with the same p/outcome pattern.
    seed_resolved_beliefs(
        &pool,
        "01EVTTIA000000000000000001",
        "zebra_producer",
        "BLFE00",
        10,
        true,
    )
    .await;
    seed_resolved_beliefs(
        &pool,
        "01EVTTIB000000000000000001",
        "apple_producer",
        "BLFF00",
        10,
        true,
    )
    .await;

    let params = CalibrationParamsRepo::new(pool.clone());
    let beliefs = BeliefsRepo::new(pool.clone());
    let (winner, _ctx, _quality) = best_calibrated_producer(
        &params,
        &beliefs,
        "test-model",
        "synth_events",
        CATEGORY,
        "platt",
    )
    .await
    .unwrap();

    // With equal quality, the tie-break is producer ASC: "apple_producer" < "zebra_producer".
    assert_eq!(
        winner.as_deref(),
        Some("apple_producer"),
        "tie must break by producer-id ASC; got {:?}",
        winner
    );
}

// ─── Test 4: cold-start — no resolved beliefs → (None, None, None) ───────────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn cold_start_no_resolved_beliefs_returns_none(pool: PgPool) {
    // No beliefs at all in the DB.
    let params = CalibrationParamsRepo::new(pool.clone());
    let beliefs = BeliefsRepo::new(pool.clone());
    let (producer, ctx, quality) = best_calibrated_producer(
        &params,
        &beliefs,
        "test-model",
        "synth_events",
        CATEGORY,
        "platt",
    )
    .await
    .unwrap();

    assert!(
        producer.is_none(),
        "cold-start: no resolved beliefs → producer must be None; got {:?}",
        producer
    );
    assert!(ctx.is_none(), "cold-start: ctx must be None; got {:?}", ctx);
    assert!(
        quality.is_none(),
        "cold-start: quality must be None; got {:?}",
        quality
    );
}

// ─── Test 5: calibration_for_scope None producer is back-compat ──────────────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn calibration_for_scope_with_none_producer_is_back_compat(pool: PgPool) {
    // None producer → merged resolved_stats (the legacy / back-compat path).
    let params = CalibrationParamsRepo::new(pool.clone());
    let beliefs = BeliefsRepo::new(pool.clone());
    let (ctx, quality) = calibration_for_scope(
        &params,
        &beliefs,
        "claude-fable-5",
        "synth_events",
        CATEGORY,
        "platt",
        None, // back-compat: None -> merged resolved_stats
    )
    .await
    .unwrap();
    assert!(ctx.is_none(), "no params → None (fail-closed)");
    assert_eq!(quality, 0.0, "no resolved history → zero quality");
}
