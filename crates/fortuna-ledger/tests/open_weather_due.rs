//! `BeliefsRepo::open_weather_bracket_due` — the producer-agnostic work queue
//! for the weather "close-the-loop" resolver (source contract §5 Layer 3).
//! Proves the filters: OPEN + grading-keys-present + DUE (`horizon <= now`),
//! with no producer/domain literal in the SQL — both Aeolus and meteorologist
//! beliefs qualify; persona MACRO beliefs (no grading keys) are excluded.

use fortuna_ledger::{BeliefsRepo, EventsRepo, PgPool};
use serde_json::json;

fn aeolus_provenance(nws_station_id: &str, variable: &str, target_date: &str) -> serde_json::Value {
    json!({
        "model_id": "aeolus",
        "producer": "aeolus",
        "station": "KNYC",
        "nws_station_id": nws_station_id,
        "variable": variable,
        "target_date": target_date,
        "run_at": "2026-06-13T00:00:00.000Z",
        "model_version": "sar-semos-v1",
    })
}

fn meteo_provenance(nws_station_id: &str, variable: &str, target_date: &str) -> serde_json::Value {
    json!({
        "producer": "meteorologist",
        "nws_station_id": nws_station_id,
        "variable": variable,
        "target_date": target_date,
    })
}

/// Insert an event + one belief with the given status-driving setup. `horizon` is
/// the settles_after gate; `resolve` flips it out of 'open'.
#[allow(clippy::too_many_arguments)]
async fn insert_belief(
    pool: &PgPool,
    belief_id: &str,
    event_id: &str,
    p: f64,
    horizon: &str,
    provenance: &serde_json::Value,
    resolve: bool,
) {
    let events = EventsRepo::new(pool.clone());
    let beliefs = BeliefsRepo::new(pool.clone());
    events
        .create(
            event_id,
            "weather bracket",
            "official NWS daily maximum",
            "nws_observed_high",
            Some(horizon),
            horizon,
            "weather",
            "2026-06-13T01:00:00.000Z",
        )
        .await
        .unwrap();
    beliefs
        .insert(
            belief_id,
            "2026-06-13T01:00:00.000Z",
            event_id,
            p,
            p,
            horizon,
            &json!([]),
            provenance,
            None,
        )
        .await
        .unwrap();
    if resolve {
        beliefs
            .resolve_and_score(belief_id, true, 0.1, None)
            .await
            .unwrap();
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn lists_only_open_grading_key_due_beliefs(pool: PgPool) {
    let beliefs = BeliefsRepo::new(pool.clone());
    let prov = aeolus_provenance("NYC", "tmax", "2026-06-13");

    // (1) DUE + OPEN + grading keys present (Aeolus) -> listed.
    insert_belief(
        &pool,
        "due-open",
        "aeolus:knyc-2026-06-13-tmax-ge87",
        0.67,
        "2026-06-14T10:00:00.000Z",
        &prov,
        false,
    )
    .await;
    // (2) Grading keys present but NOT due (horizon in the future) -> excluded.
    insert_belief(
        &pool,
        "not-due",
        "aeolus:knyc-2026-06-20-tmax-ge87",
        0.67,
        "2026-06-21T10:00:00.000Z",
        &aeolus_provenance("NYC", "tmax", "2026-06-20"),
        false,
    )
    .await;
    // (3) DUE + grading keys present but already RESOLVED -> excluded.
    insert_belief(
        &pool,
        "due-resolved",
        "aeolus:knyc-2026-06-13-tmax-ge88",
        0.47,
        "2026-06-14T10:00:00.000Z",
        &prov,
        true,
    )
    .await;
    // (4) DUE + OPEN but NO grading keys (persona MACRO) -> excluded because
    // nws_station_id/variable/target_date are absent — not because of producer.
    insert_belief(
        &pool,
        "due-no-grading-keys",
        "macro:some-event",
        0.5,
        "2026-06-14T10:00:00.000Z",
        &json!({"producer": "llm-mind"}),
        false,
    )
    .await;

    let now = "2026-06-15T00:00:00.000Z";
    let due = beliefs.open_weather_bracket_due(now, 256).await.unwrap();

    assert_eq!(
        due.len(),
        1,
        "only due+open+grading-keys-present belief qualifies"
    );
    let b = &due[0];
    assert_eq!(b.belief_id, "due-open");
    assert_eq!(b.event_id, "aeolus:knyc-2026-06-13-tmax-ge87");
    assert!((b.p - 0.67).abs() < 1e-12);
    assert_eq!(b.variable, "tmax");
    assert_eq!(b.nws_station_id, "NYC");
    assert_eq!(b.target_date, "2026-06-13");
    assert_eq!(b.producer.as_deref(), Some("aeolus"));
}

#[sqlx::test(migrations = "./migrations")]
async fn limit_caps_the_batch_oldest_due_first(pool: PgPool) {
    let beliefs = BeliefsRepo::new(pool.clone());
    let prov = aeolus_provenance("NYC", "tmax", "2026-06-13");
    // Three due+open beliefs with distinct horizons.
    for (i, horizon) in [
        "2026-06-14T08:00:00.000Z",
        "2026-06-14T09:00:00.000Z",
        "2026-06-14T10:00:00.000Z",
    ]
    .iter()
    .enumerate()
    {
        insert_belief(
            &pool,
            &format!("b{i}"),
            &format!("aeolus:knyc-2026-06-13-tmax-ge8{i}"),
            0.5,
            horizon,
            &prov,
            false,
        )
        .await;
    }
    let now = "2026-06-15T00:00:00.000Z";
    let due = beliefs.open_weather_bracket_due(now, 2).await.unwrap();
    assert_eq!(due.len(), 2, "limit caps the batch");
    // Oldest horizon first.
    assert_eq!(due[0].horizon, "2026-06-14T08:00:00.000Z");
    assert_eq!(due[1].horizon, "2026-06-14T09:00:00.000Z");
}

/// TDD WS1.2 — Integration: both Aeolus and meteorologist beliefs (two distinct
/// producers) are returned; persona MACRO belief (no grading keys) is excluded.
/// Mutation-check: re-adding `AND provenance->>'model_id'='aeolus'` in the query
/// would cause b_meteo to be EXCLUDED and this test would RED.
#[sqlx::test(migrations = "./migrations")]
async fn multi_producer_both_returned_persona_excluded(pool: PgPool) {
    let beliefs = BeliefsRepo::new(pool.clone());

    // (a) Aeolus weather belief: grading keys + producer=aeolus.
    insert_belief(
        &pool,
        "b-aeolus",
        "aeolus:knyc-2026-06-13-tmax-ge87",
        0.70,
        "2026-06-14T10:00:00.000Z",
        &aeolus_provenance("NYC", "tmax", "2026-06-13"),
        false,
    )
    .await;

    // (b) Meteorologist weather belief: grading keys + producer=meteorologist,
    // NO model_id key — selected solely because grading keys are present.
    insert_belief(
        &pool,
        "b-meteo",
        "meteo:knyc-2026-06-13-tmax-ge87",
        0.55,
        "2026-06-14T10:00:00.000Z",
        &meteo_provenance("NYC", "tmax", "2026-06-13"),
        false,
    )
    .await;

    // (c) Persona MACRO belief: has producer, but NO grading keys -> excluded.
    insert_belief(
        &pool,
        "b-persona",
        "macro:some-macro-event",
        0.80,
        "2026-06-14T10:00:00.000Z",
        &json!({"producer": "llm-mind", "persona": "bull"}),
        false,
    )
    .await;

    let now = "2026-06-15T00:00:00.000Z";
    let due = beliefs.open_weather_bracket_due(now, 256).await.unwrap();

    assert_eq!(
        due.len(),
        2,
        "both weather producers included; persona MACRO excluded"
    );

    // Collect belief_ids and producers returned — order is by horizon then belief_id.
    let ids: Vec<&str> = due.iter().map(|b| b.belief_id.as_str()).collect();
    assert!(ids.contains(&"b-aeolus"), "Aeolus belief must be returned");
    assert!(
        ids.contains(&"b-meteo"),
        "Meteorologist belief must be returned"
    );
    assert!(
        !ids.contains(&"b-persona"),
        "Persona MACRO must be excluded"
    );

    // Verify the producer field is correctly populated on each returned row.
    let aeolus_row = due.iter().find(|b| b.belief_id == "b-aeolus").unwrap();
    assert_eq!(aeolus_row.producer.as_deref(), Some("aeolus"));

    let meteo_row = due.iter().find(|b| b.belief_id == "b-meteo").unwrap();
    assert_eq!(meteo_row.producer.as_deref(), Some("meteorologist"));
}

/// TDD WS1.2 — Decoupling: no producer/domain literal in the method source.
/// This is a compile-time / static assertion: we grep the source file to confirm
/// no `'aeolus'` or `'meteorologist'` string literal appears inside the SQL query.
/// The behavioral equivalent is `multi_producer_both_returned_persona_excluded`.
#[test]
fn no_producer_literal_in_query_source() {
    let src = include_str!("../src/repos.rs");
    // Find the open_weather_bracket_due function body.
    let fn_start = src
        .find("fn open_weather_bracket_due")
        .expect("method must exist");
    // The function ends at the next `pub async fn` or `pub fn` at the top level.
    let fn_body = &src[fn_start..];
    let fn_end = fn_body.find("\n    pub ").unwrap_or(fn_body.len());
    let body = &fn_body[..fn_end];

    assert!(
        !body.contains("'aeolus'"),
        "SQL must not contain the literal 'aeolus'; found in open_weather_bracket_due body"
    );
    assert!(
        !body.contains("'meteorologist'"),
        "SQL must not contain the literal 'meteorologist'; found in open_weather_bracket_due body"
    );
    assert!(
        !body.contains("model_id"),
        "SQL must not filter by model_id; found in open_weather_bracket_due body"
    );
}
