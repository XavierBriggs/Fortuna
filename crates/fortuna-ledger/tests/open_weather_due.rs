//! `BeliefsRepo::open_aeolus_weather_due` — the work queue for the weather
//! "close-the-loop" resolver (source contract §5 Layer 3). Proves the filters:
//! only OPEN + Aeolus-produced + DUE (`horizon <= now`) beliefs are listed, with
//! the grading fields lifted out of provenance.

use fortuna_ledger::{BeliefsRepo, EventsRepo, PgPool};
use serde_json::json;

fn aeolus_provenance(nws_station_id: &str, variable: &str, target_date: &str) -> serde_json::Value {
    json!({
        "model_id": "aeolus",
        "station": "KNYC",
        "nws_station_id": nws_station_id,
        "variable": variable,
        "target_date": target_date,
        "run_at": "2026-06-13T00:00:00.000Z",
        "model_version": "sar-semos-v1",
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
async fn lists_only_open_aeolus_due_beliefs(pool: PgPool) {
    let beliefs = BeliefsRepo::new(pool.clone());
    let prov = aeolus_provenance("NYC", "tmax", "2026-06-13");

    // (1) DUE + OPEN + Aeolus -> listed.
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
    // (2) Aeolus but NOT due (horizon in the future) -> excluded.
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
    // (3) DUE Aeolus but already RESOLVED -> excluded.
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
    // (4) DUE + OPEN but NOT Aeolus (different producer) -> excluded.
    insert_belief(
        &pool,
        "due-other",
        "other:some-event",
        0.5,
        "2026-06-14T10:00:00.000Z",
        &json!({"model_id": "llm-mind", "variable": "tmax"}),
        false,
    )
    .await;

    let now = "2026-06-15T00:00:00.000Z";
    let due = beliefs.open_aeolus_weather_due(now, 256).await.unwrap();

    assert_eq!(due.len(), 1, "only the due+open+aeolus belief qualifies");
    let b = &due[0];
    assert_eq!(b.belief_id, "due-open");
    assert_eq!(b.event_id, "aeolus:knyc-2026-06-13-tmax-ge87");
    assert!((b.p - 0.67).abs() < 1e-12);
    assert_eq!(b.variable, "tmax");
    assert_eq!(b.nws_station_id, "NYC");
    assert_eq!(b.target_date, "2026-06-13");
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
    let due = beliefs.open_aeolus_weather_due(now, 2).await.unwrap();
    assert_eq!(due.len(), 2, "limit caps the batch");
    // Oldest horizon first.
    assert_eq!(due[0].horizon, "2026-06-14T08:00:00.000Z");
    assert_eq!(due[1].horizon, "2026-06-14T09:00:00.000Z");
}
