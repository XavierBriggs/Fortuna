//! W2.2 — integration tests for `GET /api/rota/v1/chain?event=<event_linkage>`.
//!
//! Black-box: seeds rows through public repo methods, calls the HTTP endpoint,
//! asserts on the JSON contract (`ChainView`) shape. The handler always returns
//! HTTP 200 (ROTA R1); absent data degrades to `{"status":"unavailable"}`.
//!
//! No access to `assemble_chain` internals — only the serialized JSON contract
//! is inspected. The I1 invariant (gate never re-invoked here — only the
//! persisted audit row is read for display) is asserted by verifying the gate
//! field is populated from seeded data, never freshly computed.

use fortuna_ledger::{BeliefsRepo, EdgesRepo, ScorecardsRepo, SignalsRepo, ValidationRunsRepo};
use fortuna_ops::dashboard::DashboardSnapshot;
use fortuna_ops::rota::{rota_router, RotaState};
use fortuna_scoring::{assemble_scorecard, CalibrationSample};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::RwLock;

// ──────────────────────────────────────────────────────────────────────────────
// Shared seed helpers
// ──────────────────────────────────────────────────────────────────────────────

/// The canonical event_linkage used across the chain-endpoint tests.
const LINKAGE: &str = "weather:NYC:tmax:2026-07-04#ge90";
const SCOPE: &str = "weather:NYC:tmax";

fn empty_snapshot() -> Arc<RwLock<DashboardSnapshot>> {
    Arc::new(RwLock::new(DashboardSnapshot {
        generated_at: "2026-07-04T00:00:00.000Z".to_string(),
        stage: "sim".to_string(),
        metrics_text: String::new(),
        boards: serde_json::json!({}),
        views: serde_json::json!({}),
    }))
}

/// Start a loopback ROTA server with a real Postgres pool and return its address.
/// The caller must abort the join handle when done.
async fn start_rota_with_pool(pool: PgPool) -> (String, tokio::task::JoinHandle<()>) {
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
        execution_mode: "paper_ledger".to_string(),
        order_mutation_enabled: false,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, h)
}

async fn seed_event(pool: &PgPool, event_id: &str, linkage: &str, category: &str) {
    sqlx::query(
        "INSERT INTO events
         (event_id, statement, resolution_criteria, resolution_source,
          benchmark_at, category, unscoreable, created_at)
         VALUES ($1, $2, 'replayed event', 'historical',
                 '2026-07-05T00:00:00.000Z', $3, FALSE,
                 '2026-07-03T00:00:00.000Z')",
    )
    .bind(event_id)
    .bind(linkage)
    .bind(category)
    .execute(pool)
    .await
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn seed_belief(
    pool: &PgPool,
    belief_id: &str,
    event_id: &str,
    p: f64,
    p_raw: f64,
    producer: &str,
    producer_type: &str,
    scope: &str,
) {
    let repo = BeliefsRepo::new(pool.clone());
    let provenance = json!({
        "producer": producer,
        "producer_type": producer_type,
        "scope": scope,
        "event_linkage": LINKAGE,
    });
    repo.insert(
        belief_id,
        "2026-07-03T12:00:00.000Z",
        event_id,
        p,
        p_raw,
        "2026-07-04T00:00:00.000Z",
        &json!({"kind": "test"}),
        &provenance,
        None,
    )
    .await
    .unwrap();
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 1 — chain_assembles_seeded_event
// ──────────────────────────────────────────────────────────────────────────────

/// Seed a full event: two producers' beliefs + proposal + gate_decision audit
/// row + a paper fill + a settlement entry + a scorecard. Then assert the chain
/// endpoint returns every stage correctly.
///
/// Design:
/// - Both producers must appear in `producers[]` with the correct `producer_id`.
/// - `proposal.side` and `gate.decision` must match what was seeded.
/// - `fill.price_cents` must match seeded value; `fill.qty` must match the seeded
///   qty (not a hardcoded sentinel).
/// - `settlement.realized_pnl_cents` must match; `settlement.outcome` must come
///   from the belief's resolved outcome (not hardcoded 0.0).
/// - `scorecard` must be present (the scope is non-empty, window is "forward").
/// - I1: gate field is the PERSISTED payload verbatim — the GatePipeline is
///   never re-invoked (only the seeded audit row is read).
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn chain_assembles_seeded_event(pool: PgPool) {
    let event_id = "01EVTCEP000000000000000001";
    let market_id = "KXNYC-TMAX-GE90-20260704";
    seed_event(&pool, event_id, LINKAGE, "weather").await;

    // Seed TWO producers' beliefs (the head-to-head showpiece).
    seed_belief(
        &pool,
        "01BLFCEP000000000000000A01",
        event_id,
        0.72,
        0.68,
        "aeolus",
        "model",
        SCOPE,
    )
    .await;
    seed_belief(
        &pool,
        "01BLFCEP000000000000000M01",
        event_id,
        0.65,
        0.65,
        "meteorologist",
        "human",
        SCOPE,
    )
    .await;

    // Resolve aeolus belief so settlement.outcome can be sourced from it.
    // outcome=true (YES=1), brier=0.0784, clv=None
    BeliefsRepo::new(pool.clone())
        .resolve_and_score("01BLFCEP000000000000000A01", true, 0.0784_f64, None)
        .await
        .unwrap();

    // Seed one signal linked to the event.
    let sig_id = "01SIGCEP000000000000000001";
    SignalsRepo::new(pool.clone())
        .insert(
            sig_id,
            "aeolus",
            "forecast",
            "2026-07-03T08:00:00.000Z",
            "hash-chain-001",
            &json!({"mu": 91.2, "sigma": 2.1}),
        )
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO event_source_evidence
         (event_id, signal_id, signal_received_at, source, signal_type, content_hash,
          relation, created_at)
         VALUES ($1, $2, $3, 'aeolus', 'forecast', 'hash-chain-001', 'model_context', $3)",
    )
    .bind(event_id)
    .bind(sig_id)
    .bind("2026-07-03T08:00:00.000Z")
    .execute(&pool)
    .await
    .unwrap();

    // Seed edge (event → market).
    EdgesRepo::new(pool.clone())
        .insert_edge(
            "01EDGCEP000000000000000001",
            market_id,
            "kalshi",
            event_id,
            "direct",
            0.95,
            "system",
            Some("system"),
            None,
            "2026-07-03T06:00:00.000Z",
        )
        .await
        .unwrap();

    // Seed proposal in intent_events.
    let intent_id = "01INTCEP000000000000000001";
    let proposal_payload = json!({
        "market": market_id,
        "side": "yes",
        "max_price_cents": 72,
        "size": 5,
        "thesis": "Aeolus: 72% YES; ridge persists",
        "belief_ref": "01BLFCEP000000000000000A01",
        "urgency": "normal",
        "at": "2026-07-03T13:00:00.000Z",
    });
    sqlx::query("INSERT INTO intent_events (intent_id, event, at) VALUES ($1, $2::jsonb, $3)")
        .bind(intent_id)
        .bind(proposal_payload.to_string())
        .bind("2026-07-03T13:00:00.000Z")
        .execute(&pool)
        .await
        .unwrap();

    // Seed gate_decision audit row (persisted verbatim for I1 render-only display).
    let gate_payload = json!({
        "decision": "accept",
        "checks": [
            {"name": "DrawdownHalt", "passed": true},
            {"name": "EdgeFloor", "passed": true, "detail": "net edge 220bps >= floor 100bps"},
        ],
        "intent_id": intent_id,
        "at": "2026-07-03T13:00:01.000Z",
    });
    sqlx::query(
        "INSERT INTO audit (audit_id, at, kind, actor, ref_id, payload)
         VALUES ($1, $2, 'gate_decision', NULL, $3, $4::jsonb)",
    )
    .bind("01AUDCEP000000000000000001")
    .bind("2026-07-03T13:00:01.000Z")
    .bind(intent_id)
    .bind(gate_payload.to_string())
    .execute(&pool)
    .await
    .unwrap();

    // Seed fill — qty=5 (the verifier found qty was hardcoded to 1; must match real).
    sqlx::query(
        "INSERT INTO fills
         (fill_id, venue, venue_order_id, client_order_id, market_id,
          side, action, price_cents, qty, fee_cents, is_maker, at)
         VALUES ($1, 'kalshi', 'vo-cep-001', 'co-cep-001', $2,
                 'yes', 'buy', 72, 5, 1, TRUE, $3)",
    )
    .bind("01FILCEP000000000000000001")
    .bind(market_id)
    .bind("2026-07-03T13:05:00.000Z")
    .execute(&pool)
    .await
    .unwrap();

    // Seed settlement entry.
    let settle_id = "01SETCEP000000000000000001";
    sqlx::query(
        "INSERT INTO settlement_entries
         (settlement_id, market_id, venue, amount_cents, status, detail, at)
         VALUES ($1, $2, 'kalshi', 360, 'confirmed', '{}'::jsonb, $3)",
    )
    .bind(settle_id)
    .bind(market_id)
    .bind("2026-07-05T14:00:00.000Z")
    .execute(&pool)
    .await
    .unwrap();

    // Seed a scorecard for the scope using window="forward" — the EXACT window
    // the live writer (recompute_scorecards, fortuna-live/src/daemon.rs ~line 5740)
    // persists. Using any other window (e.g. "30") means the scorecard stage is
    // always None on real data.
    let samples: Vec<CalibrationSample> = vec![
        CalibrationSample {
            p: 0.72,
            outcome: true,
        },
        CalibrationSample {
            p: 0.65,
            outcome: true,
        },
        CalibrationSample {
            p: 0.40,
            outcome: false,
        },
    ];
    let scorecard = assemble_scorecard(
        SCOPE,
        None,
        "forward", // MUST match the live writer's window literal
        &samples,
        0.25,
        None,
        None,
        None,
        0,
        None,
        &[],
        vec![],
        2,
    );
    ScorecardsRepo::new(pool.clone())
        .insert_scorecard(
            "01SCOCEP000000000000000001",
            &scorecard,
            "2026-07-03T14:00:00.000Z",
        )
        .await
        .unwrap();

    // Serve the endpoint.
    let (addr, h) = start_rota_with_pool(pool).await;
    // URL-encode '#' as '%23' so the fragment separator does not truncate the linkage.
    let encoded_linkage = LINKAGE.replace('#', "%23");
    let url = format!("http://{addr}/api/rota/v1/chain?event={encoded_linkage}");
    let j: Value = reqwest::get(&url).await.unwrap().json().await.unwrap();
    h.abort();

    // Assert top-level structure.
    assert!(
        j.get("status").is_none(),
        "chain must not return an unavailable status, got: {j}"
    );

    // event stage.
    let ev = &j["event"];
    assert_eq!(ev["event_linkage"], LINKAGE, "event_linkage mismatch: {j}");
    assert_eq!(ev["category"], "weather", "event category mismatch: {j}");
    assert_eq!(
        ev["scope"], SCOPE,
        "scope derived from belief provenance: {j}"
    );
    assert_eq!(
        ev["target_date"], "2026-07-04",
        "target_date parsed from linkage: {j}"
    );
    assert_eq!(
        ev["market_ticker"], market_id,
        "market_ticker from edge (maps to market_id — events table has no ticker column): {j}"
    );

    // safety pills.
    let safety = &j["safety"];
    assert_eq!(
        safety["execution_mode"], "paper_ledger",
        "safety execution_mode: {j}"
    );
    assert_eq!(
        safety["order_mutation_enabled"], false,
        "safety order_mutation_enabled: {j}"
    );

    // signals.
    let sigs = j["signals"].as_array().expect("signals must be array");
    assert_eq!(sigs.len(), 1, "expected 1 signal: {j}");
    assert_eq!(sigs[0]["source"], "aeolus", "signal source: {j}");
    assert_eq!(sigs[0]["kind"], "forecast", "signal kind: {j}");

    // producers — BOTH must be present.
    let producers = j["producers"].as_array().expect("producers must be array");
    assert_eq!(
        producers.len(),
        2,
        "expected 2 producers (head-to-head): {j}"
    );
    let prod_ids: Vec<&str> = producers
        .iter()
        .filter_map(|p| p["producer_id"].as_str())
        .collect();
    assert!(prod_ids.contains(&"aeolus"), "aeolus producer missing: {j}");
    assert!(
        prod_ids.contains(&"meteorologist"),
        "meteorologist producer missing: {j}"
    );

    // Aeolus producer: verify p_raw / p_cal.
    let aeolus = producers
        .iter()
        .find(|p| p["producer_id"] == "aeolus")
        .expect("aeolus not found in producers");
    assert!(
        (aeolus["p_raw"].as_f64().unwrap() - 0.68).abs() < 1e-9,
        "aeolus p_raw: {j}"
    );
    assert!(
        (aeolus["p_cal"].as_f64().unwrap() - 0.72).abs() < 1e-9,
        "aeolus p_cal: {j}"
    );

    // proposal stage.
    let prop = &j["proposal"];
    assert!(prop.is_object(), "proposal must be present: {j}");
    assert_eq!(prop["side"], "yes", "proposal side: {j}");
    assert_eq!(prop["max_price_cents"], 72, "proposal max_price_cents: {j}");

    // gate stage (I1: read from persisted audit payload, never re-invoked).
    let gate = &j["gate"];
    assert!(gate.is_object(), "gate must be present: {j}");
    assert_eq!(
        gate["decision"], "accept",
        "gate decision (I1 render-only): {j}"
    );
    let checks = gate["checks"]
        .as_array()
        .expect("gate checks must be array");
    assert_eq!(checks.len(), 2, "expected 2 gate checks: {j}");
    let check_names: Vec<&str> = checks.iter().filter_map(|c| c["name"].as_str()).collect();
    assert!(
        check_names.contains(&"DrawdownHalt"),
        "DrawdownHalt check missing: {j}"
    );
    assert!(
        check_names.contains(&"EdgeFloor"),
        "EdgeFloor check missing: {j}"
    );

    // fill stage — qty must come from the fills table (seeded qty=5), NOT a hardcoded sentinel.
    let fill = &j["fill"];
    assert!(fill.is_object(), "fill must be present: {j}");
    assert_eq!(fill["price_cents"], 72, "fill price_cents: {j}");
    assert_eq!(
        fill["qty"], 5,
        "fill.qty must match seeded qty (5), not hardcoded sentinel (1): {j}"
    );

    // settlement stage — outcome must come from the resolved belief (YES=1 → 1.0),
    // NOT hardcoded 0.0.
    let sett = &j["settlement"];
    assert!(sett.is_object(), "settlement must be present: {j}");
    assert_eq!(
        sett["realized_pnl_cents"], 360,
        "settlement realized_pnl_cents: {j}"
    );
    // Aeolus belief was resolved YES (outcome=1), so settlement.outcome must be 1.0.
    assert!(
        (sett["outcome"].as_f64().unwrap() - 1.0).abs() < 1e-9,
        "settlement.outcome must be sourced from resolved belief outcome (1.0 for YES), got: {j}"
    );

    // scorecard stage (scope is non-empty; window="forward" matches the live writer).
    assert!(
        j["scorecard"].is_object(),
        "scorecard must be present when scope is non-empty and window='forward': {j}"
    );
    assert_eq!(j["scorecard"]["scope"], SCOPE, "scorecard scope: {j}");
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 2 — chain_carries_validation_when_run_exists
// ──────────────────────────────────────────────────────────────────────────────

/// Seed a minimal event with one belief (to provide the scope), then insert a
/// ValidationRun payload using the REAL ValidationRun field names from
/// `fortuna_backtest::sweep::ValidationRun`. Assert that `chain.validation` is
/// Some and its REAL fields (`brier_pbo`, `brier_spa_p`, `verdict`) round-trip.
///
/// This pins the WS3→WS4 contract: the endpoint accepts any JSON shape and
/// forwards it verbatim (the `validation` field is `Option<serde_json::Value>`).
/// The seeded payload uses REAL ValidationRun field names, not the prior
/// fictional pbo/spa_p_c names.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn chain_carries_validation_when_run_exists(pool: PgPool) {
    let event_id = "01EVTVAL000000000000000001";
    let linkage = "weather:BOS:tmax:2026-07-10#ge85";
    let scope = "weather:BOS:tmax";
    seed_event(&pool, event_id, linkage, "weather").await;

    // One belief to establish the scope (so validation is looked up).
    seed_belief(
        &pool,
        "01BLFVAL000000000000000001",
        event_id,
        0.55,
        0.55,
        "aeolus",
        "model",
        scope,
    )
    .await;

    // Insert a ValidationRun payload using the REAL ValidationRun field names
    // (fortuna_backtest::sweep::ValidationRun). The prior placeholder used fictional
    // pbo/spa_p_c — replaced here with brier_pbo/brier_spa_p from the real type.
    let validation_payload = json!({
        "run_id": "01SWEEPVAL00000000000000001",
        "scope": scope,
        "producer": null,
        "trial_space": {
            "calibration_windows": [30],
            "recal_methods": ["platt"],
            "scopes": [scope],
            "go_thresholds": [0.5]
        },
        "n_trials": 1,
        "family_n_trials": 2,
        "selected_config": {
            "calibration_window": 30,
            "recal_method": "platt",
            "go_threshold": 0.5
        },
        "brier_edge": 0.042,
        "brier_pbo": 0.03,
        "brier_spa_p": 0.02,
        "clv_edge": 0.011,
        "clv_pbo": 0.08,
        "clv_spa_p": 0.12,
        "effective_n": 18.0,
        "mintrl_ok": true,
        "sharpe_dsr": 0.71,
        "verdict": "go",
        "computed_at": "2026-07-03T00:00:00.000Z",
    });
    ValidationRunsRepo::new(pool.clone())
        .insert(
            "01RUNVAL000000000000000001",
            scope,
            None,
            &validation_payload,
            "2026-07-03T00:00:00.000Z",
        )
        .await
        .unwrap();

    let (addr, h) = start_rota_with_pool(pool).await;
    // URL-encode '#' as '%23' so the fragment separator does not truncate the linkage.
    let encoded_linkage = linkage.replace('#', "%23");
    let url = format!("http://{addr}/api/rota/v1/chain?event={encoded_linkage}");
    let j: Value = reqwest::get(&url).await.unwrap().json().await.unwrap();
    h.abort();

    // The validation field must be present and its REAL fields must round-trip.
    assert!(
        j.get("status").is_none(),
        "chain must not return unavailable: {j}"
    );
    let val = &j["validation"];
    assert!(
        val.is_object(),
        "validation must be present when a ValidationRun exists: {j}"
    );
    // Assert REAL ValidationRun fields (not the fictional pbo/spa_p_c).
    assert!(
        val["brier_pbo"].is_number(),
        "brier_pbo must be a number: {j}"
    );
    assert_eq!(
        val["brier_pbo"].as_f64().unwrap(),
        0.03,
        "brier_pbo must round-trip: {j}"
    );
    assert!(
        val["brier_spa_p"].is_number(),
        "brier_spa_p must be a number (not fictional spa_p_c): {j}"
    );
    assert_eq!(
        val["brier_spa_p"].as_f64().unwrap(),
        0.02,
        "brier_spa_p round-trips: {j}"
    );
    // verdict is the GoDecision snake_case string.
    assert_eq!(
        val["verdict"], "go",
        "verdict must be 'go' (snake_case GoDecision): {j}"
    );
    assert_eq!(
        val["family_n_trials"], 2,
        "family_n_trials round-trips: {j}"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3 — chain_unavailable_for_unknown_event
// ──────────────────────────────────────────────────────────────────────────────

/// When no event exists for the requested linkage, the spec requires:
/// HTTP 200 + `{"status":"unavailable","detail":"event not found"}`.
/// This is the unavailable envelope the UI branches on (ROTA R1 — never 404/500).
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn chain_unavailable_for_unknown_event(pool: PgPool) {
    let (addr, h) = start_rota_with_pool(pool).await;
    let url =
        format!("http://{addr}/api/rota/v1/chain?event=weather:NOWHERE:tmax:1970-01-01%23ge99");
    let resp = reqwest::get(&url).await.unwrap();

    // Must be HTTP 200 (ROTA R1: never 404/500 for absent data).
    assert_eq!(resp.status(), 200, "must be HTTP 200 for unknown event");

    let j: Value = resp.json().await.unwrap();
    h.abort();

    // Must carry the unavailable envelope so the UI can branch.
    assert_eq!(
        j["status"], "unavailable",
        "must return status:unavailable for unknown event: {j}"
    );
    let detail = j["detail"].as_str().unwrap_or("");
    assert!(
        detail.contains("event not found"),
        "detail must mention 'event not found': {j}"
    );
}
