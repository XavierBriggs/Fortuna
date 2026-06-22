//! W2.1 — event-scoped chain reads for the chain-view endpoint (WS4).
//!
//! Five new reads, each tested red→green:
//!   1. `EventsRepo::by_linkage`     — resolve event_linkage → EventRow
//!   2. `BeliefsRepo::beliefs_for_event` — per-event, per-producer belief list
//!   3. `SignalsRepo::signals_for_event` — signals linked to an event
//!   4. `ProposalsRepo::for_event`   — first intent (proposal) for an event
//!      + gate-decision audit row keyed to the same event
//!   5. Fill/settlement via event→market resolution (reuse existing market-keyed reads)
//!
//! Tests are black-box: they seed rows through public repo methods (or raw
//! sqlx where no public writer exists), then assert on the NEW read shapes.
//! No test knows how the implementation picks the rows — it only knows the
//! contract (what comes back).

use fortuna_ledger::{
    BeliefsRepo, EdgesRepo, EventsRepo, FillsRepo, ProposalsRepo, SettlementsRepo, SignalsRepo,
};
use serde_json::json;
use sqlx::PgPool;

// ──────────────────────────────────────────────────────────────────────────────
// Shared seed helpers
// ──────────────────────────────────────────────────────────────────────────────

/// The canonical event_linkage used across most tests in this file.
const LINKAGE: &str = "weather:NYC:tmax:2026-07-04#ge90";

async fn seed_event_with_linkage(pool: &PgPool, event_id: &str, linkage: &str, category: &str) {
    // The harness stores event_linkage in the `statement` column (see
    // fortuna-backtest/src/harness.rs:300: `linkage` passed as `statement`).
    sqlx::query(
        "INSERT INTO events
         (event_id, statement, resolution_criteria, resolution_source,
          benchmark_at, category, unscoreable, created_at)
         VALUES ($1, $2, 'replayed historical event', 'historical',
                 '2026-07-05T00:00:00.000Z', $3, FALSE,
                 '2026-07-03T00:00:00.000Z')",
    )
    .bind(event_id)
    .bind(linkage) // statement = event_linkage (the harness contract)
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
    mind_id: Option<&str>,
    mind_version: Option<i64>,
    rationale: Option<&str>,
) {
    let repo = BeliefsRepo::new(pool.clone());
    let provenance = json!({
        "producer": producer,
        "producer_type": producer_type,
        "mind_id": mind_id,
        "mind_version": mind_version,
        "event_linkage": LINKAGE,
        "rationale": rationale,
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
// Test 1 — EventsRepo::by_linkage
// ──────────────────────────────────────────────────────────────────────────────

/// Seed an event whose statement == event_linkage, then assert
/// `EventsRepo::by_linkage` finds it and returns the right row.
///
/// Also tests the negative case: an absent linkage returns `None`.
#[sqlx::test(migrations = "./migrations")]
async fn by_linkage_returns_matching_event_row(pool: PgPool) {
    let event_id = "01EVTLNK000000000000000001";
    let linkage = LINKAGE;
    seed_event_with_linkage(&pool, event_id, linkage, "weather").await;

    let repo = EventsRepo::new(pool.clone());

    // Positive: known linkage → Some(row) with the right event_id.
    let row = repo
        .by_linkage(linkage)
        .await
        .expect("by_linkage query failed");
    assert!(
        row.is_some(),
        "expected Some(row) for seeded linkage, got None"
    );
    let row = row.unwrap();
    assert_eq!(row.event_id, event_id, "event_id mismatch");
    assert_eq!(row.category, "weather", "category mismatch");

    // Negative: unknown linkage → None (no row seeded for this string).
    let absent = repo
        .by_linkage("weather:NYC:tmax:1900-01-01#ge999")
        .await
        .expect("by_linkage absent query failed");
    assert!(
        absent.is_none(),
        "expected None for unknown linkage, got Some"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 2 — BeliefsRepo::beliefs_for_event (two-producer seed)
// ──────────────────────────────────────────────────────────────────────────────

/// Seed TWO producers' beliefs on one event, then assert both come back with
/// the right producer attribution and scoring columns.
///
/// Design (from the spec brief):
/// - `producer_id`  from `provenance->>'producer'`
/// - `producer_type` from `provenance->>'producer_type'`
/// - `mind_id` / `mind_version` from provenance
/// - `p_raw` = the `p` column (the emitted probability; NOT recalibrated here)
/// - `p_cal` = `p` column when the belief was calibrated; we seed p != p_raw
///             to exercise both fields independently — but for rows inserted
///             via `BeliefsRepo::insert`, `p` IS the post-calibration value
///             and `p_raw` is the raw value. So `p_cal = p` (the `p` column).
/// - `rationale` from `provenance->>'rationale'`
/// - `belief_at` = `created_at`
/// - scoring: `status`, `outcome` (Option), `brier` (Option), `clv_bps` (Option)
#[sqlx::test(migrations = "./migrations")]
async fn beliefs_for_event_returns_both_producers(pool: PgPool) {
    let event_id = "01EVTBFE000000000000000001";
    seed_event_with_linkage(&pool, event_id, LINKAGE, "weather").await;

    // Aeolus — a model producer.
    seed_belief(
        &pool,
        "01BLFBFE000000000000000A01",
        event_id,
        0.72, // p (post-cal)
        0.68, // p_raw
        "aeolus",
        "model",
        Some("aeolus-v3"),
        Some(3),
        Some("TMAX forecast for NYC 4 July above 90°F"),
    )
    .await;

    // Meteorologist — a human producer.
    seed_belief(
        &pool,
        "01BLFBFE000000000000000M01",
        event_id,
        0.65, // p (post-cal) — identical to p_raw for human
        0.65, // p_raw
        "meteorologist",
        "human",
        None,
        None,
        Some("Expert assessment: ridge holds"),
    )
    .await;

    let repo = BeliefsRepo::new(pool.clone());
    let beliefs = repo
        .beliefs_for_event(event_id)
        .await
        .expect("beliefs_for_event failed");

    assert_eq!(
        beliefs.len(),
        2,
        "expected 2 beliefs (one per producer), got {}: {beliefs:?}",
        beliefs.len()
    );

    // Find by producer_id (order not guaranteed).
    let aeolus = beliefs
        .iter()
        .find(|b| b.producer_id.as_deref() == Some("aeolus"))
        .expect("aeolus belief missing");
    let meteo = beliefs
        .iter()
        .find(|b| b.producer_id.as_deref() == Some("meteorologist"))
        .expect("meteorologist belief missing");

    // Aeolus: verify all fields.
    assert_eq!(
        aeolus.producer_type.as_deref(),
        Some("model"),
        "aeolus producer_type"
    );
    assert_eq!(
        aeolus.mind_id.as_deref(),
        Some("aeolus-v3"),
        "aeolus mind_id"
    );
    assert_eq!(aeolus.mind_version, Some(3), "aeolus mind_version");
    assert!(
        (aeolus.p_raw - 0.68).abs() < 1e-9,
        "aeolus p_raw expected 0.68, got {}",
        aeolus.p_raw
    );
    assert!(
        (aeolus.p_cal - 0.72).abs() < 1e-9,
        "aeolus p_cal expected 0.72 (the `p` column), got {}",
        aeolus.p_cal
    );
    assert_eq!(
        aeolus.rationale.as_deref(),
        Some("TMAX forecast for NYC 4 July above 90°F"),
        "aeolus rationale"
    );
    assert_eq!(aeolus.status, "open", "aeolus initial status");
    assert!(aeolus.outcome.is_none(), "outcome not set yet");
    assert!(aeolus.brier.is_none(), "brier not set yet");
    assert!(aeolus.clv_bps.is_none(), "clv_bps not set yet");

    // Meteorologist: verify fields.
    assert_eq!(
        meteo.producer_type.as_deref(),
        Some("human"),
        "meteo producer_type"
    );
    assert!(meteo.mind_id.is_none(), "meteo mind_id should be None");
    assert!(
        meteo.mind_version.is_none(),
        "meteo mind_version should be None"
    );
    assert!(
        (meteo.p_raw - 0.65).abs() < 1e-9,
        "meteo p_raw expected 0.65, got {}",
        meteo.p_raw
    );
    assert_eq!(
        meteo.rationale.as_deref(),
        Some("Expert assessment: ridge holds"),
        "meteo rationale"
    );

    // Post-resolution: resolve aeolus belief and verify scoring columns return.
    repo.resolve_and_score("01BLFBFE000000000000000A01", true, 0.07_f64, Some(320.0))
        .await
        .expect("resolve_and_score failed");

    let beliefs_after = repo
        .beliefs_for_event(event_id)
        .await
        .expect("beliefs_for_event after resolution failed");
    let aeolus_after = beliefs_after
        .iter()
        .find(|b| b.producer_id.as_deref() == Some("aeolus"))
        .expect("aeolus missing after resolution");
    assert_eq!(aeolus_after.status, "resolved", "status after resolve");
    assert_eq!(
        aeolus_after.outcome,
        Some(1),
        "outcome after resolve: YES=1"
    );
    assert!(
        aeolus_after.brier.is_some(),
        "brier should be set after resolution"
    );
    assert!(
        aeolus_after.clv_bps.is_some(),
        "clv_bps should be set after resolution"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3 — SignalsRepo::signals_for_event
// ──────────────────────────────────────────────────────────────────────────────

/// Seed an event + two signals linked via `event_source_evidence`, then assert
/// `signals_for_event` returns exactly those signals.
#[sqlx::test(migrations = "./migrations")]
async fn signals_for_event_returns_linked_signals(pool: PgPool) {
    let event_id = "01EVTSFE000000000000000001";
    seed_event_with_linkage(&pool, event_id, LINKAGE, "weather").await;

    // Seed two signal rows (via raw SQL — SignalsRepo::insert is private to the
    // production insert path; the content_hash unique constraint just needs to
    // differ per (source, content_hash, received_at)).
    let sig_a = "01SIGSFEA00000000000000001";
    let sig_b = "01SIGSFE000000000000000001";
    let at_a = "2026-07-03T08:00:00.000Z";
    let at_b = "2026-07-03T09:00:00.000Z";

    let signals_repo = SignalsRepo::new(pool.clone());
    signals_repo
        .insert(
            sig_a,
            "nws",
            "afd",
            at_a,
            "hash-a-001",
            &json!({"text": "AFD bulletin A"}),
        )
        .await
        .expect("insert signal A");
    signals_repo
        .insert(
            sig_b,
            "aeolus",
            "forecast",
            at_b,
            "hash-b-001",
            &json!({"text": "Aeolus forecast"}),
        )
        .await
        .expect("insert signal B");

    // Link signals to the event via event_source_evidence.
    sqlx::query(
        "INSERT INTO event_source_evidence
         (event_id, signal_id, signal_received_at, source, signal_type, content_hash,
          relation, created_at)
         VALUES ($1, $2, $3, 'nws', 'afd', 'hash-a-001', 'model_context', $3)",
    )
    .bind(event_id)
    .bind(sig_a)
    .bind(at_a)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO event_source_evidence
         (event_id, signal_id, signal_received_at, source, signal_type, content_hash,
          relation, created_at)
         VALUES ($1, $2, $3, 'aeolus', 'forecast', 'hash-b-001', 'model_context', $3)",
    )
    .bind(event_id)
    .bind(sig_b)
    .bind(at_b)
    .execute(&pool)
    .await
    .unwrap();

    // Seed a THIRD signal NOT linked to this event (must not appear).
    let sig_unrelated = "01SIGSFEU00000000000000001";
    signals_repo
        .insert(
            sig_unrelated,
            "nws",
            "afd",
            "2026-07-03T07:00:00.000Z",
            "hash-u-001",
            &json!({"text": "Unrelated"}),
        )
        .await
        .expect("insert unrelated signal");

    let rows = signals_repo
        .signals_for_event(event_id)
        .await
        .expect("signals_for_event failed");

    assert_eq!(
        rows.len(),
        2,
        "expected 2 signals for event, got {}: {rows:?}",
        rows.len()
    );

    let ids: Vec<&str> = rows.iter().map(|r| r.signal_id.as_str()).collect();
    assert!(
        ids.contains(&sig_a),
        "signal A missing from result: {ids:?}"
    );
    assert!(
        ids.contains(&sig_b),
        "signal B missing from result: {ids:?}"
    );

    // Verify shape fields are populated.
    let row_a = rows.iter().find(|r| r.signal_id == sig_a).unwrap();
    assert_eq!(row_a.source, "nws", "signal A source");
    assert_eq!(row_a.kind, "afd", "signal A kind");
    assert_eq!(row_a.received_at, at_a, "signal A received_at");
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 4 — ProposalsRepo::for_event + gate_decision audit row
// ──────────────────────────────────────────────────────────────────────────────

/// Seed an event, an intent (proposal) referencing the event's market via
/// `intent_events`, and a `gate_decision` audit row referencing the same intent.
/// Assert that `ProposalsRepo::for_event` returns the proposal and the
/// gate-decision row.
#[sqlx::test(migrations = "./migrations")]
async fn proposal_and_gate_decision_for_event(pool: PgPool) {
    let event_id = "01EVTPGE000000000000000001";
    let market_id = "KXNYC-TMAX-GE90-20260704";
    seed_event_with_linkage(&pool, event_id, LINKAGE, "weather").await;

    // Insert a market_event_edge so the event→market link exists.
    let edges = EdgesRepo::new(pool.clone());
    edges
        .insert_edge(
            "01EDGPGE000000000000000001",
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
        .expect("insert edge");

    // Seed the intent_events row carrying the proposal payload for this market.
    let intent_id = "01INTP00000000000000000001";
    let proposal_payload = json!({
        "market": market_id,
        "side": "yes",
        "max_price_cents": 72,
        "size": 5,
        "thesis": "Aeolus: 72% YES; ridge persists",
        "belief_ref": "01BLFPGE000000000000000001",
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

    // Seed the gate_decision audit row referencing the intent.
    let gate_payload = json!({
        "check": "EdgeFloor",
        "verdict": "Pass",
        "reason": "net edge 220bps >= floor 100bps",
        "intent_id": intent_id,
        "client_order_id": "coid-test-001",
        "at": "2026-07-03T13:00:01.000Z",
    });
    sqlx::query(
        "INSERT INTO audit (audit_id, at, kind, actor, ref_id, payload)
         VALUES ($1, $2, 'gate_decision', NULL, $3, $4::jsonb)",
    )
    .bind("01AUDPGE000000000000000001")
    .bind("2026-07-03T13:00:01.000Z")
    .bind(intent_id)
    .bind(gate_payload.to_string())
    .execute(&pool)
    .await
    .unwrap();

    let repo = ProposalsRepo::new(pool.clone());

    // Assert the proposal row is found.
    let proposal = repo.for_event(event_id).await.expect("for_event failed");
    assert!(
        proposal.is_some(),
        "expected Some(ProposalRow) for seeded event"
    );
    let p = proposal.unwrap();
    assert_eq!(p.market_id, market_id, "proposal market_id");
    assert_eq!(p.intent_id, intent_id, "proposal intent_id");
    assert_eq!(
        p.payload["side"].as_str().unwrap(),
        "yes",
        "proposal side from payload"
    );

    // Assert the gate_decision row is found.
    let gate = repo
        .gate_decision_for_event(event_id)
        .await
        .expect("gate_decision_for_event failed");
    assert!(gate.is_some(), "expected Some gate_decision row");
    let g = gate.unwrap();
    assert_eq!(
        g.payload["verdict"].as_str().unwrap(),
        "Pass",
        "gate verdict"
    );
    assert_eq!(
        g.payload["check"].as_str().unwrap(),
        "EdgeFloor",
        "gate check name"
    );

    // Negative: an event with no proposal returns None.
    let event_no_prop = "01EVTPGE000000000000000002";
    seed_event_with_linkage(
        &pool,
        event_no_prop,
        "weather:LAX:tmax:2026-07-04#ge95",
        "weather",
    )
    .await;
    let absent = repo
        .for_event(event_no_prop)
        .await
        .expect("for_event absent failed");
    assert!(absent.is_none(), "expected None for event with no proposal");
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 5 — Fill/settlement via event→market resolution
// ──────────────────────────────────────────────────────────────────────────────

/// Seed event → edge (event→market) → fill + settlement. Then:
/// 1. `EventsRepo::market_id_for_event` resolves event→market_id.
/// 2. `FillsRepo::first_fill_for_market` returns the seeded fill.
/// 3. `SettlementsRepo::chain` returns the seeded settlement.
#[sqlx::test(migrations = "./migrations")]
async fn fill_and_settlement_resolve_via_event_market(pool: PgPool) {
    let event_id = "01EVTFSE000000000000000001";
    let market_id = "KXNYC-TMAX-GE90-20260704-SETTLE";
    seed_event_with_linkage(&pool, event_id, LINKAGE, "weather").await;

    // Edge: event→market.
    let edges = EdgesRepo::new(pool.clone());
    edges
        .insert_edge(
            "01EDGFSE000000000000000001",
            market_id,
            "kalshi",
            event_id,
            "direct",
            0.98,
            "system",
            Some("system"),
            None,
            "2026-07-03T06:00:00.000Z",
        )
        .await
        .expect("insert edge for fill test");

    // Insert a fill for the market.
    sqlx::query(
        "INSERT INTO fills
         (fill_id, venue, venue_order_id, client_order_id, market_id,
          side, action, price_cents, qty, fee_cents, is_maker, at)
         VALUES ($1, 'kalshi', 'vo-001', 'co-001', $2,
                 'yes', 'buy', 72, 5, 1, TRUE, $3)",
    )
    .bind("01FILFSE000000000000000001")
    .bind(market_id)
    .bind("2026-07-03T13:05:00.000Z")
    .execute(&pool)
    .await
    .unwrap();

    // Insert a settlement entry for the market.
    let settle_id = "01SETFSE000000000000000001";
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

    // 1. Resolve event_id → market_id.
    let events = EventsRepo::new(pool.clone());
    let mid = events
        .market_id_for_event(event_id)
        .await
        .expect("market_id_for_event failed");
    assert!(mid.is_some(), "expected Some(market_id) for seeded edge");
    let mid = mid.unwrap();
    assert_eq!(mid, market_id, "market_id mismatch");

    // 2. First fill via market_id.
    let fills = FillsRepo::new(pool.clone());
    let fill = fills
        .first_fill_for_market(&mid)
        .await
        .expect("first_fill_for_market failed");
    assert!(fill.is_some(), "expected Some fill");
    let fill = fill.unwrap();
    assert_eq!(fill.price_cents, 72, "fill price_cents");
    assert_eq!(fill.side, "yes", "fill side");

    // 3. Settlement chain.
    let settlements = SettlementsRepo::new(pool.clone());
    let chain = settlements.chain(&mid).await.expect("chain failed");
    assert_eq!(chain.len(), 1, "expected 1 settlement entry");
    assert_eq!(chain[0].settlement_id, settle_id, "settlement_id");
    assert_eq!(chain[0].amount_cents, 360, "settlement amount_cents");

    // Negative: an event with no edge has no market_id.
    let event_no_edge = "01EVTFSE000000000000000002";
    seed_event_with_linkage(
        &pool,
        event_no_edge,
        "weather:LAX:tmax:2026-07-04#ge100",
        "weather",
    )
    .await;
    let absent_mid = events
        .market_id_for_event(event_no_edge)
        .await
        .expect("market_id_for_event absent failed");
    assert!(
        absent_mid.is_none(),
        "expected None for event with no edge, got {absent_mid:?}"
    );
}
