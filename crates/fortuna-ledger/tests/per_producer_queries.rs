//! WS1 Task 6 — Per-producer scoring queries (TDD — tests written BEFORE
//! implementation per DoD).
//!
//! Two integration tests:
//!   1. Binary per-producer: `resolved_stats_for_producer` returns ONLY the
//!      requesting producer's beliefs; the existing `resolved_stats` (unfiltered)
//!      returns both. Mutation-resistant: removing the producer filter makes this
//!      test RED (the filtered call would return both instead of one).
//!   2. Scalar per-producer: `scores_for_producer(p, rule, n)` returns only that
//!      producer's `BeliefScoreRow` entries from `belief_scores ⋈ scalar_beliefs`.

use fortuna_ledger::{BeliefScoresRepo, BeliefsRepo, ScalarBeliefsRepo};
use serde_json::json;
use sqlx::PgPool;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Seed a minimal event row for belief foreign-key constraints.
async fn seed_event(pool: &PgPool, event_id: &str, category: &str) {
    sqlx::query(
        "INSERT INTO events (event_id, statement, resolution_criteria,
                             resolution_source, benchmark_at, category,
                             unscoreable, created_at)
         VALUES ($1, 'stmt', 'crit', 'nws',
                 '2026-06-20T00:00:00.000Z', $2, FALSE,
                 '2026-06-19T00:00:00.000Z')",
    )
    .bind(event_id)
    .bind(category)
    .execute(pool)
    .await
    .unwrap();
}

// ─── Test 1: binary per-producer resolved stats ───────────────────────────────

/// Seed one binary belief with a specific `provenance.producer`, then resolve
/// and score it (sets `status='resolved'`, `outcome`, `brier`).
async fn seed_binary_belief(
    pool: &PgPool,
    belief_id: &str,
    event_id: &str,
    p: f64,
    producer: &str,
) {
    let repo = BeliefsRepo::new(pool.clone());
    let provenance = json!({ "producer": producer, "model_id": "test-model" });
    repo.insert(
        belief_id,
        "2026-06-19T10:00:00.000Z",
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
    // Resolve + score it so it appears in `resolved_stats*`.
    repo.resolve_and_score(belief_id, true, 0.25, Some(150.0))
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn binary_resolved_stats_for_producer_filters_by_producer(pool: PgPool) {
    let cat = "weather";
    let event_a = "01EVTPPA000000000000000001";
    let event_b = "01EVTPPB000000000000000001";

    seed_event(&pool, event_a, cat).await;
    seed_event(&pool, event_b, cat).await;

    // Belief A: producer = "aeolus"
    seed_binary_belief(&pool, "01BLFPP0000000000000000A1", event_a, 0.70, "aeolus").await;
    // Belief B: producer = "meteorologist"
    seed_binary_belief(
        &pool,
        "01BLFPP0000000000000000B1",
        event_b,
        0.55,
        "meteorologist",
    )
    .await;

    let repo = BeliefsRepo::new(pool.clone());

    // Per-producer: "aeolus" gets exactly 1 row with p ≈ 0.70.
    let aeolus_stats = repo
        .resolved_stats_for_producer("aeolus", cat)
        .await
        .unwrap();
    assert_eq!(
        aeolus_stats.len(),
        1,
        "aeolus filter must return exactly 1 row, got {}",
        aeolus_stats.len()
    );
    assert!(
        (aeolus_stats[0].p - 0.70).abs() < 1e-9,
        "aeolus belief p should be 0.70, got {}",
        aeolus_stats[0].p
    );

    // Per-producer: "meteorologist" gets exactly 1 row with p ≈ 0.55.
    let met_stats = repo
        .resolved_stats_for_producer("meteorologist", cat)
        .await
        .unwrap();
    assert_eq!(
        met_stats.len(),
        1,
        "meteorologist filter must return exactly 1 row, got {}",
        met_stats.len()
    );
    assert!(
        (met_stats[0].p - 0.55).abs() < 1e-9,
        "meteorologist belief p should be 0.55, got {}",
        met_stats[0].p
    );

    // Unfiltered resolved_stats returns BOTH rows (regression guard: merged baseline
    // is not accidentally broken).
    let all_stats = repo.resolved_stats(cat).await.unwrap();
    assert_eq!(
        all_stats.len(),
        2,
        "unfiltered resolved_stats must return 2 rows for both producers"
    );

    // Mutation-resistance: if the producer filter were removed, `resolved_stats_for_producer`
    // would return 2 rows, not 1 — the assertions above (`len() == 1`) would fail.
}

// ─── Test 2: scalar per-producer belief scores ────────────────────────────────

/// Seed one scalar belief with the given `producer`, then insert a score row.
async fn seed_scalar_belief_with_score(
    pool: &PgPool,
    belief_id: &str,
    score_id: &str,
    producer: &str,
    rule_id: &str,
    score: f64,
) {
    let scalar_repo = ScalarBeliefsRepo::new(pool.clone());
    let scores_repo = BeliefScoresRepo::new(pool.clone());

    // Insert the scalar belief (producer is first-class column).
    scalar_repo
        .insert(
            belief_id,
            producer,
            &format!("KXBTCPERP:{belief_id}:event-key"),
            &json!([{"q": 0.5, "v": 0.0001}]),
            "rate",
            "2026-06-20T00:00:00.000Z",
            &json!({"strategy": "test"}),
            "2026-06-19T10:00:00.000Z",
        )
        .await
        .unwrap();

    // Insert a belief score linking to this scalar belief.
    scores_repo
        .insert(
            score_id,
            belief_id,
            rule_id,
            score,
            "2026-06-19T11:00:00.000Z",
        )
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn scalar_scores_for_producer_filters_by_producer(pool: PgPool) {
    let rule = "crps_pinball";

    // Producer 1: "aeolus_funding"
    seed_scalar_belief_with_score(
        &pool,
        "01SBLFPP00000000000000A1",
        "01SCOREPP0000000000000A1",
        "aeolus_funding",
        rule,
        0.002,
    )
    .await;

    // Producer 2: "kairos_v1"
    seed_scalar_belief_with_score(
        &pool,
        "01SBLFPP00000000000000B1",
        "01SCOREPP0000000000000B1",
        "kairos_v1",
        rule,
        0.007,
    )
    .await;

    let scores_repo = BeliefScoresRepo::new(pool.clone());

    // scores_for_producer("aeolus_funding", rule, 10) → only p1's score.
    let aeolus_scores = scores_repo
        .scores_for_producer("aeolus_funding", rule, 10)
        .await
        .unwrap();
    assert_eq!(
        aeolus_scores.len(),
        1,
        "aeolus_funding producer filter must return exactly 1 score, got {}",
        aeolus_scores.len()
    );
    assert_eq!(aeolus_scores[0].belief_id, "01SBLFPP00000000000000A1");
    assert!(
        (aeolus_scores[0].score - 0.002).abs() < 1e-9,
        "aeolus score should be 0.002, got {}",
        aeolus_scores[0].score
    );
    assert_eq!(aeolus_scores[0].rule_id, rule);

    // scores_for_producer("kairos_v1", rule, 10) → only p2's score.
    let kairos_scores = scores_repo
        .scores_for_producer("kairos_v1", rule, 10)
        .await
        .unwrap();
    assert_eq!(
        kairos_scores.len(),
        1,
        "kairos_v1 producer filter must return exactly 1 score, got {}",
        kairos_scores.len()
    );
    assert_eq!(kairos_scores[0].belief_id, "01SBLFPP00000000000000B1");
    assert!(
        (kairos_scores[0].score - 0.007).abs() < 1e-9,
        "kairos score should be 0.007, got {}",
        kairos_scores[0].score
    );
    assert_eq!(kairos_scores[0].rule_id, rule);

    // Mutation-resistance: if the producer filter were removed, both producers'
    // scores would be returned for either query, making the `len() == 1` assertion
    // fail and the `belief_id` assertion fail.
}
