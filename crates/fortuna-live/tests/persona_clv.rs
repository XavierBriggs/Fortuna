//! W5 (G1 CLV-for-persona) — the head-to-head completer.
//!
//! Today a meteorologist (persona) belief's `clv_bps` is ALWAYS `None` at
//! resolution: its `event_id` namespace (`weather:KNYC:tmax:DATE#ge87`) has no
//! `market_event_edge` row, so the producer-agnostic CLV resolver hits
//! `current_edges_for_event(persona_event_id) == []` and returns `None`. The
//! ONLY missing link is the edge row.
//!
//! W5's `link_persona_market_edges` closes that gap at persona belief-formation:
//! it parses the persona draft's `…#ge<thr>` token (station/variable/date/
//! threshold from the event_id + provenance), finds the EXISTING edge for the
//! SAME bracket on ANOTHER event (the Aeolus event's `market_event_edge`), and
//! inserts the persona_event_id → that SAME market_id edge. Then the existing,
//! UNCHANGED resolver computes `clv_bps` for the meteorologist exactly as it
//! does for Aeolus.
//!
//! ## Honesty (carried into the assertion — NOT just a comment)
//!
//! Because the persona edge points to the SAME market as Aeolus, the resolver
//! computes CLV from the SAME earliest fill on that shared market. The
//! meteorologist's `clv_bps` is therefore IDENTICAL to Aeolus's — this is
//! MARKET-LEVEL drift, NOT an independent per-producer confirmation. The test
//! asserts EQUALITY (`persona clv_bps == aeolus clv_bps`, both `Some`), not
//! merely non-null, so any future change that made them silently diverge (e.g.
//! the persona getting its own separate market) would RED here and force the
//! honesty story to be re-examined. Brier — not CLV — is the per-producer
//! differentiator.

use fortuna_core::book::Fill;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, VenueOrderId};
use fortuna_core::money::Cents;
use fortuna_ledger::{BeliefsRepo, EdgesRepo, EventsRepo, FillsRepo, SignalsRepo, SnapshotsRepo};
use fortuna_live::daemon::{link_persona_market_edges, resolve_and_score_weather_beliefs};
use serde_json::json;
use sqlx::PgPool;

const CLINYC_CLI: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/nws_climate/cli_product_clinyc.json"
));

/// The shared bracket: NYC daily high (`tmax`), 2026-06-13, `ge87`.
const TARGET_DATE: &str = "2026-06-13";
/// Grading station; the recorded CLINYC product (MAXIMUM 91°F) routes to it.
const GRADING_STATION: &str = "NYC";
/// The belief horizon AND the event benchmark_at (the resolver reads
/// `benchmark_at` off each belief's own event; both events share it so the
/// CLV benchmark window is identical for the equality claim).
const BENCHMARK_AT: &str = "2026-06-14T10:00:00.000Z";
/// The Aeolus and meteorologist event ids for the SAME bracket. Same station/
/// variable/date/threshold; only the producer grammar differs.
const AEOLUS_EVENT_ID: &str = "aeolus:knyc-2026-06-13-tmax-ge87";
const METEO_EVENT_ID: &str = "weather:NYC:tmax:2026-06-13#ge87";
/// The shared Kalshi market both producers' edges point at.
const SHARED_MARKET_ID: &str = "KXHIGHNY-26JUN13-T87";
const VENUE: &str = "kalshi";

/// `now` strictly after the horizon (so the beliefs are DUE).
fn now() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-15T00:00:00.000Z").unwrap()
}

fn product_text(fixture: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(fixture).unwrap();
    v["productText"].as_str().unwrap().to_string()
}

async fn insert_cli_signal(pool: &PgPool, signal_id: &str, text: &str) {
    SignalsRepo::new(pool.clone())
        .insert(
            signal_id,
            "nws_cli",
            "nws.cli",
            "2026-06-14T11:00:00.000Z",
            signal_id,
            &json!({ "productText": text }),
        )
        .await
        .unwrap();
}

/// Create a weather event row for `event_id` with the shared benchmark_at.
async fn create_event(pool: &PgPool, event_id: &str, created_at: &str) {
    EventsRepo::new(pool.clone())
        .create(
            event_id,
            "weather bracket",
            "official NWS daily maximum",
            "nws_observed_high",
            Some(BENCHMARK_AT),
            BENCHMARK_AT,
            "weather",
            created_at,
        )
        .await
        .unwrap();
}

/// Insert ONE liquid pre-benchmark snapshot for `(market_id, event_id)`.
///
/// In production a market's cadence snapshots are captured under exactly ONE
/// event_id (the runner tracks a market under its CONFIRMED edge's event), and
/// the `(market_id, at)` unique index permits only one row per timestamp. So the
/// shared market has ONE benchmark snapshot, tagged with the Aeolus (confirmed-
/// edge) event_id — NEVER the persona's PROPOSED-edge event_id. The resolver
/// therefore reads the benchmark by MARKET (any event_id), which is how both
/// producers see the SAME shared-market mid (the W5 equality).
async fn insert_liquid_snapshot(
    pool: &PgPool,
    snapshot_id: &str,
    market_id: &str,
    event_id: &str,
    at: &str,
) {
    SnapshotsRepo::new(pool.clone())
        .insert(
            snapshot_id,
            market_id,
            VENUE,
            Some(event_id),
            "t1h",
            Some(70), // best_bid
            Some(74), // best_ask (spread 4c ≤ 10c policy; mid = 72c)
            Some(80), // bid_qty
            Some(80), // ask_qty
            true,     // liquidity_ok
            at,
        )
        .await
        .unwrap();
}

/// Seed one fill on the shared market (the earliest/entry fill the CLV uses).
async fn insert_fill(pool: &PgPool, market_id: &str, at: &str) {
    let fill = Fill {
        fill_id: format!("fill-{market_id}"),
        venue_order_id: VenueOrderId::new("vo-1").unwrap(),
        client_order_id: ClientOrderId::new("co-1").unwrap(),
        market: MarketId::new(market_id).unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        price: Cents::new(60), // entry 60c; benchmark mid 72c → +20% ≈ +2000 bps
        qty: Contracts::new(1),
        fee: Cents::new(0),
        is_maker: true,
        at: UtcTimestamp::parse_iso8601(at).unwrap(),
    };
    FillsRepo::new(pool.clone())
        .insert(VENUE, &fill, Some("aeolus"), Some("aeolus_mech"))
        .await
        .unwrap();
}

/// The persona belief draft (built the way `map_persona_analysis` builds it):
/// event_id in the persona grammar + provenance carrying the grading keys.
fn meteo_draft(p: f64) -> fortuna_cognition::beliefs::BeliefDraft {
    let horizon = UtcTimestamp::parse_iso8601(BENCHMARK_AT).unwrap();
    fortuna_cognition::beliefs::BeliefDraft {
        event_id: METEO_EVENT_ID.to_string(),
        p,
        p_raw: p,
        horizon,
        evidence: json!([{"source": "persona:meteorologist@1", "ref": "01METEOANALYSIS"}]),
        provenance: json!({
            "producer": "meteorologist",
            "persona_id": "meteorologist",
            "persona_version": 1,
            "analysis_id": "01METEOANALYSIS",
            "analysis_content_hash": "ch-meteo-1",
            "nws_station_id": GRADING_STATION,
            "variable": "tmax",
            "target_date": TARGET_DATE,
        }),
    }
}

// ── the W5 thesis: the meteorologist belief gets a NON-NULL clv_bps EQUAL to ──
// ── Aeolus's, via the threshold-match edge join (no resolver change) ──────────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn meteorologist_belief_gets_nonnull_clv(pool: PgPool) {
    let p_ge87 = 0.6719055375922601_f64;

    // The independent grader: CLINYC (MAXIMUM 91°F) routes to station NYC.
    insert_cli_signal(&pool, "cli-nyc", &product_text(CLINYC_CLI)).await;

    // --- Aeolus side: event + belief + the EXISTING market edge + fill + snap ---
    create_event(&pool, AEOLUS_EVENT_ID, "2026-06-13T01:00:00.000Z").await;
    BeliefsRepo::new(pool.clone())
        .insert(
            "w5-aeolus-ge87",
            "2026-06-13T01:00:00.000Z",
            AEOLUS_EVENT_ID,
            p_ge87,
            p_ge87,
            BENCHMARK_AT,
            &json!([{"source": "aeolus", "ref": "knyc-tmax"}]),
            &json!({
                "model_id": "aeolus",
                "producer": "aeolus",
                "station": "KNYC",
                "nws_station_id": GRADING_STATION,
                "variable": "tmax",
                "target_date": TARGET_DATE,
            }),
            None,
        )
        .await
        .unwrap();
    // The Aeolus discovery edge → the shared market (the row W5 must FIND).
    EdgesRepo::new(pool.clone())
        .insert_edge(
            "01EDGAEOLUS0000000000001",
            SHARED_MARKET_ID,
            VENUE,
            AEOLUS_EVENT_ID,
            "direct",
            0.9,
            "aeolus_mech",
            Some("discovery:auto"),
            None,
            "2026-06-13T02:00:00.000Z",
        )
        .await
        .unwrap();
    // The entry fill on the shared market + the SINGLE liquid benchmark snapshot,
    // tagged with the Aeolus (confirmed-edge) event_id — exactly as production
    // captures it (one snapshot per market, under the tracking event). The
    // persona belief reads this SAME shared-market snapshot via the market-keyed
    // resolver read; there is NO separate persona-event snapshot (the
    // `(market_id, at)` unique index forbids a duplicate, and PROPOSED edges
    // aren't tracked for capture).
    insert_fill(&pool, SHARED_MARKET_ID, "2026-06-13T03:00:00.000Z").await;
    insert_liquid_snapshot(
        &pool,
        "snap-shared",
        SHARED_MARKET_ID,
        AEOLUS_EVENT_ID,
        "2026-06-13T04:00:00.000Z",
    )
    .await;

    // --- Meteorologist side: event + belief on the SAME bracket, NO edge yet ---
    let draft = meteo_draft(p_ge87);
    create_event(&pool, METEO_EVENT_ID, "2026-06-13T01:30:00.000Z").await;
    BeliefsRepo::new(pool.clone())
        .insert(
            "w5-meteo-ge87",
            "2026-06-13T01:30:00.000Z",
            METEO_EVENT_ID,
            draft.p,
            draft.p_raw,
            BENCHMARK_AT,
            &draft.evidence,
            &draft.provenance,
            None,
        )
        .await
        .unwrap();

    // Sanity: BEFORE the join, the persona event has NO edge → resolver would
    // give it None CLV. (The exact condition the resolver short-circuits on.)
    let before = EdgesRepo::new(pool.clone())
        .current_edges_for_event(METEO_EVENT_ID)
        .await
        .unwrap();
    assert!(
        before.is_empty(),
        "precondition: persona event has no edge before the W5 join"
    );

    // --- W5: the threshold-match edge join (at persona belief-formation) ------
    let linked = link_persona_market_edges(
        &pool,
        std::slice::from_ref(&draft),
        "meteorologist",
        0,
        now(),
    )
    .await
    .expect("link_persona_market_edges must not error");
    assert_eq!(
        linked, 1,
        "exactly one persona edge linked to the shared Aeolus market"
    );

    // The persona event now resolves to the SAME market the Aeolus edge points at.
    let after = EdgesRepo::new(pool.clone())
        .current_edges_for_event(METEO_EVENT_ID)
        .await
        .unwrap();
    assert_eq!(after.len(), 1, "persona event has exactly one current edge");
    assert_eq!(
        after[0].market_id, SHARED_MARKET_ID,
        "persona edge points at the SAME market as Aeolus (market-level drift)"
    );

    // --- Run the producer-agnostic resolver (UNCHANGED by W5) ----------------
    let resolved = resolve_and_score_weather_beliefs(&pool, now(), 0)
        .await
        .expect("resolve_and_score_weather_beliefs");
    assert_eq!(resolved, 2, "both Aeolus + meteorologist brackets resolve");

    let beliefs = BeliefsRepo::new(pool.clone());
    let aeolus = beliefs.get("w5-aeolus-ge87").await.unwrap();
    let meteo = beliefs.get("w5-meteo-ge87").await.unwrap();

    // Both resolved with the realized outcome (91 ≥ 87 → true).
    assert_eq!(aeolus.outcome, Some(1));
    assert_eq!(meteo.outcome, Some(1));

    // The thesis: the meteorologist belief carries a NON-NULL clv_bps.
    let aeolus_clv = aeolus
        .clv_bps
        .expect("Aeolus belief has clv_bps (it always did)");
    let meteo_clv = meteo
        .clv_bps
        .expect("meteorologist belief now carries clv_bps (W5) — today this is None");

    // HONESTY: identical, because it is the SAME market entry — market-level
    // drift, NOT a second independent confirmation. EQUALITY, not just non-null.
    assert!(
        (aeolus_clv - meteo_clv).abs() < 1e-9,
        "persona clv_bps == aeolus clv_bps (same shared market, same earliest fill): \
         aeolus={aeolus_clv}, meteo={meteo_clv}"
    );
}
