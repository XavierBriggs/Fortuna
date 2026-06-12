//! R7 — the two ROTA cognition-panel ledger queries (T4.3-owned; design
//! docs/design/rota-dashboard.md amendment R7), written BEFORE the
//! implementation per DoD. Populated-path seeds per the verifier's
//! vacuous-test rule: every assertion checks REAL seeded values, and the
//! seeds are NON-ZERO so a fabricated/empty result cannot pass.

use fortuna_ledger::{BeliefsRepo, CalibrationParamsRepo};
use sqlx::PgPool;

async fn seed_event(pool: &PgPool, event_id: &str) {
    sqlx::query(
        "INSERT INTO events (event_id, statement, resolution_criteria,
                             resolution_source, benchmark_at, category,
                             unscoreable, created_at)
         VALUES ($1, 'seed', 'seed', 'nws',
                 '2026-06-12T00:00:00.000Z', 'weather', FALSE,
                 '2026-06-11T00:00:00.000Z')",
    )
    .bind(event_id)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn beliefs_recent_lists_newest_first_with_evidence_and_provenance(pool: PgPool) {
    let event = "01EVENT000000000000000001";
    seed_event(&pool, event).await;
    let repo = BeliefsRepo::new(pool.clone());
    // ULIDs order lexically == chronologically; seed three in order.
    let ids = [
        "01BELIEFAAAAAAAAAAAAAAAA01",
        "01BELIEFAAAAAAAAAAAAAAAA02",
        "01BELIEFAAAAAAAAAAAAAAAA03",
    ];
    for (i, id) in ids.iter().enumerate() {
        repo.insert(
            id,
            "2026-06-12T01:00:00.000Z",
            event,
            0.60 + (i as f64) * 0.05,
            0.70,
            "2026-06-13T00:00:00.000Z",
            &serde_json::json!({"reasoning": format!("evidence-{i}"), "n": i}),
            &serde_json::json!({"model_id": "claude-fable-5", "cost_cents": 12 + i as i64}),
            None,
        )
        .await
        .unwrap();
    }

    let rows = repo.recent(2).await.unwrap();
    assert_eq!(rows.len(), 2, "limit respected");
    assert_eq!(rows[0].belief_id, ids[2], "newest first");
    assert_eq!(rows[1].belief_id, ids[1]);
    // Real seeded values — a zeroed/fabricated panel cannot satisfy these.
    assert!(
        (rows[0].p - 0.70).abs() < 1e-9,
        "p roundtrips: {}",
        rows[0].p
    );
    assert!((rows[0].p_raw - 0.70).abs() < 1e-9);
    assert_eq!(rows[0].status, "open");
    assert_eq!(rows[0].event_id, event);
    assert_eq!(rows[0].created_at, "2026-06-12T01:00:00.000Z");
    assert_eq!(
        rows[0].evidence["reasoning"],
        serde_json::json!("evidence-2"),
        "evidence JSONB roundtrips"
    );
    assert_eq!(
        rows[0].provenance["cost_cents"],
        serde_json::json!(14),
        "provenance JSONB roundtrips"
    );
    assert!(rows[0].brier.is_none(), "unscored belief has null brier");
}

#[sqlx::test]
async fn beliefs_recent_clamps_a_nonpositive_limit(pool: PgPool) {
    let event = "01EVENT000000000000000002";
    seed_event(&pool, event).await;
    let repo = BeliefsRepo::new(pool.clone());
    repo.insert(
        "01BELIEFBBBBBBBBBBBBBBBB01",
        "2026-06-12T01:00:00.000Z",
        event,
        0.5,
        0.5,
        "2026-06-13T00:00:00.000Z",
        &serde_json::json!({}),
        &serde_json::json!({}),
        None,
    )
    .await
    .unwrap();
    let rows = repo.recent(0).await.unwrap();
    assert_eq!(rows.len(), 1, "limit 0 clamps to 1, never errors");
}

#[sqlx::test]
async fn calibration_scopes_enumerates_distinct_scopes_at_max_version(pool: PgPool) {
    let repo = CalibrationParamsRepo::new(pool.clone());
    let scope_a = ("claude-fable-5", "synth_events", "weather", "platt");
    for (version, effective_at) in [
        (1, "2026-06-10T00:00:00.000Z"),
        (2, "2026-06-11T00:00:00.000Z"),
    ] {
        repo.insert(
            &format!("01CAL00000000000000000A0{version}"),
            scope_a.0,
            scope_a.1,
            scope_a.2,
            scope_a.3,
            &serde_json::json!({"a": 0.1, "b": 0.9}),
            version,
            effective_at,
            "2026-06-11T00:00:00.000Z",
        )
        .await
        .unwrap();
    }
    repo.insert(
        "01CAL00000000000000000B01",
        "claude-haiku-4-5",
        "synth_events",
        "weather",
        "shrinkage",
        &serde_json::json!({"lambda": 0.2}),
        1,
        "2026-06-11T00:00:00.000Z",
        "2026-06-11T00:00:00.000Z",
    )
    .await
    .unwrap();

    let scopes = repo.scopes().await.unwrap();
    assert_eq!(scopes.len(), 2, "one row per distinct scope: {scopes:?}");
    let a = scopes
        .iter()
        .find(|s| s.model_id == "claude-fable-5")
        .expect("scope A present");
    assert_eq!(a.version, 2, "max version wins");
    assert_eq!(a.effective_at, "2026-06-11T00:00:00.000Z");
    assert_eq!(
        (a.strategy.as_str(), a.category.as_str(), a.kind.as_str()),
        ("synth_events", "weather", "platt")
    );
    let b = scopes
        .iter()
        .find(|s| s.model_id == "claude-haiku-4-5")
        .expect("scope B present");
    assert_eq!(b.version, 1);
    assert_eq!(b.kind, "shrinkage");
}

#[sqlx::test]
async fn calibration_scopes_is_empty_not_an_error_on_a_fresh_db(pool: PgPool) {
    let repo = CalibrationParamsRepo::new(pool);
    let scopes = repo.scopes().await.unwrap();
    assert!(scopes.is_empty());
}
