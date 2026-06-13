//! Track C slice 1b tests: the scalar-belief storage layer (spec
//! docs/design/perp-strategies-and-scalar-claims.md §1.3/§1.4, §9.1).
//!
//! These tests are written FROM the spec text BEFORE the implementation
//! (TDD). They cover, adversarially:
//!   - insert -> get round-trip, including JSONB quantile fidelity and the
//!     first-class `producer` column the ROTA §9.1 scorecard groups by;
//!   - exactly-once scalar resolution (a second resolve is refused — mirrors
//!     `beliefs_insert_supersede_and_score_exactly_once`);
//!   - the DB-level append-only guard: a raw content UPDATE is refused, a raw
//!     DELETE is refused, but the resolution UPDATE (realized_value/resolved_at
//!     from NULL) succeeds exactly once;
//!   - `belief_scores`: one (belief_id, rule_id) per rule, a duplicate errors
//!     (unique), a different rule for the same belief succeeds (two scorers
//!     side by side), and the table is fully immutable (UPDATE/DELETE refused);
//!   - `recent` newest-first ordering and the `scores_for_*` reads.
//!
//! Each test gets an isolated, migrated database via #[sqlx::test].

use fortuna_ledger::{BeliefScoresRepo, ScalarBeliefsRepo};
use serde_json::json;
use sqlx::PgPool;

// The canonical 0.1/0.5/0.9 quantile fan over a funding rate (rate units).
fn quantiles_fan() -> serde_json::Value {
    json!([
        {"q": 0.1, "v": 0.000_05},
        {"q": 0.5, "v": 0.000_10},
        {"q": 0.9, "v": 0.000_18}
    ])
}

fn provenance() -> serde_json::Value {
    json!({"strategy": "funding_forecast", "model_id": "estimate_twap_v1"})
}

// ─── scalar_beliefs: insert -> get round-trip ─────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn scalar_belief_insert_get_round_trip(pool: PgPool) {
    let repo = ScalarBeliefsRepo::new(pool);
    let quantiles = quantiles_fan();
    let prov = provenance();

    repo.insert(
        "sb-1",
        "funding_forecast",
        "KXBTCPERP:2026-06-13T16:00:00Z",
        &quantiles,
        "rate",
        "2026-06-13T16:00:00.000Z",
        &prov,
        "2026-06-13T15:00:00.000Z",
    )
    .await
    .unwrap();

    let row = repo.get("sb-1").await.unwrap();
    assert_eq!(row.belief_id, "sb-1");
    assert_eq!(row.producer, "funding_forecast");
    assert_eq!(row.event_key, "KXBTCPERP:2026-06-13T16:00:00Z");
    assert_eq!(row.unit, "rate");
    assert_eq!(row.horizon, "2026-06-13T16:00:00.000Z");
    assert_eq!(row.created_at, "2026-06-13T15:00:00.000Z");
    // JSONB fidelity: the quantile fan round-trips verbatim.
    assert_eq!(row.quantiles, quantiles);
    assert_eq!(row.provenance, prov);
    // Unresolved on insert.
    assert!(row.realized_value.is_none());
    assert!(row.resolved_at.is_none());
}

// ─── scalar_beliefs: exactly-once resolution ──────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn scalar_belief_resolve_exactly_once(pool: PgPool) {
    let repo = ScalarBeliefsRepo::new(pool);
    repo.insert(
        "sb-1",
        "funding_forecast",
        "KXBTCPERP:win-1",
        &quantiles_fan(),
        "rate",
        "2026-06-13T16:00:00.000Z",
        &provenance(),
        "2026-06-13T15:00:00.000Z",
    )
    .await
    .unwrap();

    // First resolution writes the realized value + resolved_at.
    repo.resolve("sb-1", 0.000_12, "2026-06-13T16:00:01.000Z")
        .await
        .unwrap();
    let row = repo.get("sb-1").await.unwrap();
    assert!((row.realized_value.unwrap() - 0.000_12).abs() < 1e-12);
    assert_eq!(row.resolved_at.as_deref(), Some("2026-06-13T16:00:01.000Z"));

    // A SECOND resolution of the same belief is refused (exactly-once;
    // rows_affected 0 -> CorruptRow), mirroring resolve_and_score.
    let second = repo
        .resolve("sb-1", 0.000_99, "2026-06-13T17:00:00.000Z")
        .await;
    assert!(second.is_err());
    // The realized value is unchanged by the refused second write.
    let row = repo.get("sb-1").await.unwrap();
    assert!((row.realized_value.unwrap() - 0.000_12).abs() < 1e-12);

    // Resolving a belief that does not exist is also refused.
    assert!(repo
        .resolve("sb-nope", 0.1, "2026-06-13T17:00:00.000Z")
        .await
        .is_err());
}

// ─── scalar_beliefs: the DB-level append-only guard ───────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn scalar_belief_guard_refuses_content_mutation_and_delete(pool: PgPool) {
    let repo = ScalarBeliefsRepo::new(pool.clone());
    repo.insert(
        "sb-1",
        "funding_forecast",
        "KXBTCPERP:win-1",
        &quantiles_fan(),
        "rate",
        "2026-06-13T16:00:00.000Z",
        &provenance(),
        "2026-06-13T15:00:00.000Z",
    )
    .await
    .unwrap();

    // A raw content UPDATE (changing `unit`) is refused by the trigger.
    let bad_unit = sqlx::query("UPDATE scalar_beliefs SET unit = 'forged' WHERE belief_id = $1")
        .bind("sb-1")
        .execute(&pool)
        .await;
    assert!(bad_unit
        .unwrap_err()
        .to_string()
        .contains("content is immutable"));

    // A raw content UPDATE of the quantiles is likewise refused.
    let bad_q =
        sqlx::query("UPDATE scalar_beliefs SET quantiles = '[]'::jsonb WHERE belief_id = $1")
            .bind("sb-1")
            .execute(&pool)
            .await;
    assert!(bad_q.is_err());

    // A raw DELETE is refused.
    let del = sqlx::query("DELETE FROM scalar_beliefs WHERE belief_id = $1")
        .bind("sb-1")
        .execute(&pool)
        .await;
    assert!(del.unwrap_err().to_string().contains("append-only"));

    // But the resolution path — an UPDATE of ONLY realized_value/resolved_at
    // from NULL — succeeds (the one-time transition the guard allows).
    repo.resolve("sb-1", 0.000_15, "2026-06-13T16:00:01.000Z")
        .await
        .unwrap();
    let row = repo.get("sb-1").await.unwrap();
    assert!((row.realized_value.unwrap() - 0.000_15).abs() < 1e-12);

    // A raw UPDATE that tries to RE-WRITE realized_value once it is already
    // set is refused too (the guard only allows the NULL -> value transition).
    let rewrite =
        sqlx::query("UPDATE scalar_beliefs SET realized_value = 0.5 WHERE belief_id = $1")
            .bind("sb-1")
            .execute(&pool)
            .await;
    assert!(rewrite.unwrap_err().to_string().contains("set once"));
}

// ─── scalar_beliefs: recent newest-first ──────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn scalar_belief_recent_is_newest_first(pool: PgPool) {
    let repo = ScalarBeliefsRepo::new(pool);
    // ULIDs sort lexically == chronologically; insert ascending ids.
    for (id, key) in [("sb-1", "k1"), ("sb-2", "k2"), ("sb-3", "k3")] {
        repo.insert(
            id,
            "funding_forecast",
            key,
            &quantiles_fan(),
            "rate",
            "2026-06-13T16:00:00.000Z",
            &provenance(),
            "2026-06-13T15:00:00.000Z",
        )
        .await
        .unwrap();
    }

    let recent = repo.recent(10).await.unwrap();
    assert_eq!(recent.len(), 3);
    // Newest-first: sb-3, sb-2, sb-1.
    assert_eq!(recent[0].belief_id, "sb-3");
    assert_eq!(recent[1].belief_id, "sb-2");
    assert_eq!(recent[2].belief_id, "sb-1");

    // The limit clamps to >=1 (a bad 0/negative limit still returns one row,
    // never errors, never fetches unboundedly — read-only panel discipline).
    let one = repo.recent(0).await.unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].belief_id, "sb-3");
}

// ─── belief_scores: one row per (belief, rule); duplicate errors ──────────────

#[sqlx::test(migrations = "./migrations")]
async fn belief_scores_insert_and_unique_per_rule(pool: PgPool) {
    let beliefs = ScalarBeliefsRepo::new(pool.clone());
    beliefs
        .insert(
            "sb-1",
            "funding_forecast",
            "KXBTCPERP:win-1",
            &quantiles_fan(),
            "rate",
            "2026-06-13T16:00:00.000Z",
            &provenance(),
            "2026-06-13T15:00:00.000Z",
        )
        .await
        .unwrap();

    let scores = BeliefScoresRepo::new(pool);

    // First (belief, rule) score lands.
    scores
        .insert(
            "score-1",
            "sb-1",
            "crps_pinball",
            0.000_03,
            "2026-06-13T16:00:02.000Z",
        )
        .await
        .unwrap();

    // A DUPLICATE (belief_id, rule_id) is refused (the unique constraint
    // bubbles as an error — NOT silently ignored: exactly-once per rule).
    let dup = scores
        .insert(
            "score-1b",
            "sb-1",
            "crps_pinball",
            0.000_09,
            "2026-06-13T16:00:03.000Z",
        )
        .await;
    assert!(dup.is_err());

    // A DIFFERENT rule for the SAME belief succeeds — two scorers side by side
    // (the swappable-ScoringRule payoff: re-score the immutable facts).
    scores
        .insert(
            "score-2",
            "sb-1",
            "crps_weighted",
            0.000_05,
            "2026-06-13T16:00:04.000Z",
        )
        .await
        .unwrap();

    let for_belief = scores.scores_for_belief("sb-1").await.unwrap();
    assert_eq!(for_belief.len(), 2);
    let rules: Vec<&str> = for_belief.iter().map(|r| r.rule_id.as_str()).collect();
    assert!(rules.contains(&"crps_pinball"));
    assert!(rules.contains(&"crps_weighted"));
    // The crps_pinball score is the FIRST write (the refused duplicate did
    // not overwrite it).
    let pinball = for_belief
        .iter()
        .find(|r| r.rule_id == "crps_pinball")
        .unwrap();
    assert!((pinball.score - 0.000_03).abs() < 1e-12);
    assert_eq!(pinball.belief_id, "sb-1");
}

// ─── belief_scores: scores_for_rule and full immutability ─────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn belief_scores_for_rule_and_append_only(pool: PgPool) {
    let beliefs = ScalarBeliefsRepo::new(pool.clone());
    for (id, key) in [("sb-1", "k1"), ("sb-2", "k2")] {
        beliefs
            .insert(
                id,
                "funding_forecast",
                key,
                &quantiles_fan(),
                "rate",
                "2026-06-13T16:00:00.000Z",
                &provenance(),
                "2026-06-13T15:00:00.000Z",
            )
            .await
            .unwrap();
    }

    let scores = BeliefScoresRepo::new(pool.clone());
    scores
        .insert(
            "score-1",
            "sb-1",
            "crps_pinball",
            0.000_03,
            "2026-06-13T16:00:02.000Z",
        )
        .await
        .unwrap();
    scores
        .insert(
            "score-2",
            "sb-2",
            "crps_pinball",
            0.000_07,
            "2026-06-13T16:00:05.000Z",
        )
        .await
        .unwrap();

    // scores_for_rule returns every score for one rule across beliefs.
    let by_rule = scores.scores_for_rule("crps_pinball", 10).await.unwrap();
    assert_eq!(by_rule.len(), 2);
    let belief_ids: Vec<&str> = by_rule.iter().map(|r| r.belief_id.as_str()).collect();
    assert!(belief_ids.contains(&"sb-1"));
    assert!(belief_ids.contains(&"sb-2"));

    // An unseen rule returns nothing.
    assert!(scores
        .scores_for_rule("no_such_rule", 10)
        .await
        .unwrap()
        .is_empty());

    // belief_scores is FULLY immutable: a raw UPDATE is refused.
    let upd = sqlx::query("UPDATE belief_scores SET score = 9.0 WHERE score_id = $1")
        .bind("score-1")
        .execute(&pool)
        .await;
    assert!(upd.unwrap_err().to_string().contains("append-only"));

    // A raw DELETE is refused.
    let del = sqlx::query("DELETE FROM belief_scores WHERE score_id = $1")
        .bind("score-1")
        .execute(&pool)
        .await;
    assert!(del.unwrap_err().to_string().contains("append-only"));
}

// ─── belief_scores: referential integrity (the FK) ────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn belief_score_for_unknown_belief_is_refused(pool: PgPool) {
    // The FK belief_scores.belief_id -> scalar_beliefs(belief_id) makes an
    // orphan score impossible: a score for a non-existent belief is refused.
    // The table is immutable, so an orphan row could never be corrected — the
    // FK stops it at write time.
    let scores = BeliefScoresRepo::new(pool);
    let orphan = scores
        .insert(
            "score-x",
            "sb-does-not-exist",
            "crps_pinball",
            0.1,
            "2026-06-13T16:00:00.000Z",
        )
        .await;
    assert!(orphan.is_err());
}
