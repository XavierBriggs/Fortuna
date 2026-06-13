//! T4.2 item 2(iii) — Kalshi adapter paper-clearance, CLUSTER 2: transport
//! ROUND-TRIPS. Drive the adapter's place / cancel / fills flows through a
//! scripted `MockKalshiTransport` whose responses are the OPERATOR-RECORDED
//! fixtures (fixtures/kalshi/), and assert the canonical result + `VenueError`
//! routing the real wire produces. Cluster 1 (kalshi_recorded.rs) asserts DTO /
//! error-body parsing; this asserts the adapter's exec flows over that wire.
//!
//! The headline is the CANCEL-RECONCILE STALE-READ RACE (wire finding F16/F3):
//! the recorded DELETE acked `reduced_by:"1.00"`, but the reconcile GET ~360ms
//! later still read `status:"resting"` with `last_update_time` UNCHANGED. The
//! adapter must NOT report a false success off that stale read — it returns
//! Timeout (effect-unknown; the caller reconciles), which is the safe outcome.
//!
//! Clearance record: docs/design/track-a-kalshi-paper-clearance.md (Cluster 2
//! covers items 6, 8-routing, 15, 19-roundtrip).

use fortuna_core::book::{FeeError, FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{
    Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId, VenueOrderId,
};
use fortuna_core::money::Cents;
use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline, GatedOrder};
use fortuna_venues::kalshi::client::MockKalshiTransport;
use fortuna_venues::kalshi::KalshiVenue;
use fortuna_venues::{Cursor, Venue, VenueError};
use futures::executor::block_on;
use std::collections::BTreeSet;
use std::sync::Arc;

const RECORDED_TICKER: &str = "KXWTACHALLENGERMATCH-26JUN11JIMLEP-LEP";

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-11T06:31:00.000Z").unwrap()
}

/// Load an OPERATOR-RECORDED response body (verbatim venue wire).
fn recorded(name: &str) -> serde_json::Value {
    let path = format!(
        "{}/../../fixtures/kalshi/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse fixture {path}: {e}"))
}

fn venue_with(mock: &Arc<MockKalshiTransport>) -> KalshiVenue {
    KalshiVenue::new(
        VenueId::new("kalshi").unwrap(),
        Arc::clone(mock) as Arc<dyn fortuna_venues::kalshi::client::KalshiTransport>,
        Arc::new(SimClock::new(t0())),
        vec!["KXWTACHALLENGERMATCH".to_string()],
    )
    .unwrap()
}

struct ZeroFees;
impl FeeModel for ZeroFees {
    fn fee(
        &self,
        _role: FillRole,
        _price: Cents,
        _qty: Contracts,
        _category: Option<&str>,
        _at: UtcTimestamp,
    ) -> Result<Cents, FeeError> {
        Ok(Cents::ZERO)
    }
}

fn permissive_config() -> GateConfig {
    toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 1000000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 49
        max_cross_cents = 98
        per_market_exposure_cents = 1000000
        per_event_exposure_cents = 1000000
        require_event_mapping = false

        [per_strategy.t1]
        max_exposure_cents = 1000000
        max_order_notional_cents = 1000000
        min_net_edge_bps = 0

        [rate.kalshi]
        burst = 100000
        sustained_per_min = 100000
        market_burst = 100000
        market_sustained_per_min = 100000
        "#,
    )
    .unwrap()
}

fn yes_book(market: &MarketId) -> OrderBook {
    OrderBook {
        market: market.clone(),
        as_of: t0(),
        yes_bids: vec![PriceLevel {
            price: Cents::new(47),
            qty: Contracts::new(1000),
        }],
        yes_asks: vec![PriceLevel {
            price: Cents::new(52),
            qty: Contracts::new(1000),
        }],
    }
}

/// Mint a real GatedOrder through the gate pipeline (type-level I1).
fn gated(price: i64, qty: i64, fair: i64, coid: &str) -> GatedOrder {
    let mut pipeline = GatePipeline::new(permissive_config()).unwrap();
    let market = MarketId::new(RECORDED_TICKER).unwrap();
    let book = yes_book(&market);
    let fees = ZeroFees;
    let recent = BTreeSet::new();
    let mut idgen = IdGen::new(7);
    let candidate = CandidateOrder {
        intent_id: IntentId::new(idgen.next(t0()).unwrap()),
        strategy: StrategyId::new("t1").unwrap(),
        venue: VenueId::new("kalshi").unwrap(),
        market,
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(price),
        qty: Contracts::new(qty),
        fair_value: Cents::new(fair),
        client_order_id: ClientOrderId::new(coid).unwrap(),
    };
    let inputs = GateInputs {
        now: t0(),
        open_exposure_cents: Cents::ZERO,
        market_exposure_cents: Cents::ZERO,
        strategy_exposure_cents: Cents::ZERO,
        event_exposure_cents: Cents::ZERO,
        event_id: None,
        book: Some(&book),
        last_trade_price: None,
        fee_model: &fees,
        category: None,
        own_resting: &[],
        recent_client_order_ids: &recent,
    };
    pipeline
        .evaluate(&candidate, &inputs)
        .gated
        .unwrap_or_else(|r| panic!("gate rejected test order: {r:?}"))
}

// ===========================================================================
// place() — the recorded V2 create response round-trips to a venue order id
// ===========================================================================

#[test]
fn recorded_place_taker_ioc_returns_the_venue_order_id() {
    // orders__create_v2_taker_ioc.json: the real 201 for the recorded taker IOC
    // (order_id 97ec18b7..., fill_count 1.00). place() parses it to a VenueOrderId.
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(201, recorded("orders__create_v2_taker_ioc.json"));
    let venue = venue_with(&mock);

    let id =
        block_on(venue.place(gated(52, 1, 60, "11111111-1111-4111-8111-111111111111"))).unwrap();
    assert_eq!(id.as_str(), "97ec18b7-10d3-4557-9de0-8598aad625f0");

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].method.as_str(), calls[0].path.as_str()),
        ("POST", "/portfolio/events/orders")
    );
}

// ===========================================================================
// place() — a recorded nested 4xx routes to Rejected with a STRUCTURED reason
// (validates G1 end-to-end through the exec path, not just error_reason)
// ===========================================================================

#[test]
fn recorded_place_insufficient_balance_is_rejected_with_structured_reason() {
    // orders__insufficient_balance.json: the real nested 400
    // {"error":{"code":"insufficient_balance",...}}.
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(400, recorded("orders__insufficient_balance.json"));
    let venue = venue_with(&mock);

    let err = block_on(venue.place(gated(52, 1, 60, "22222222-2222-4222-8222-222222222222")))
        .unwrap_err();
    match err {
        VenueError::Rejected { reason } => {
            // G1: the nested code is structure-extracted into the reason, not a
            // raw-JSON dump — the venue's code reaches diagnostics legibly.
            assert!(
                reason.contains("insufficient_balance"),
                "reason should carry the venue code: {reason}"
            );
        }
        other => panic!("a recorded 400 must route to Rejected, got {other:?}"),
    }
}

// ===========================================================================
// place() — the RECORDED duplicate-409 routes to AlreadyExists (item 7).
// The logic is covered by kalshi_adapter.rs's place_duplicate_409_* tests, but
// those used a SYNTHETIC sample whose code was the placeholder
// `PLACEHOLDER_DUPLICATE_CODE_FIXTURE_NEEDED` (the real code was undocumented).
// This drives the OPERATOR-RECORDED body — the NESTED
// {"error":{"code":"order_already_exists",...}} the placeholder was awaiting —
// proving the real wire shape resolves to the existing order (idempotent place;
// never a false success).
// ===========================================================================

#[test]
fn recorded_place_duplicate_client_order_id_resolves_to_already_exists() {
    let coid = "33333333-3333-4333-8333-333333333333";
    let existing_id = "e641ead7-badf-41d1-a4bf-04a6cb202714";

    let mock = Arc::new(MockKalshiTransport::new());
    // POST -> the recorded 409 (nested {"error":{"code":"order_already_exists"}}).
    mock.push_ok(409, recorded("orders__duplicate_client_order_id.json"));
    // The adapter then lists orders by client_order_id (resting bucket first); the
    // existing order carries OUR coid, so the lookup resolves on the first page.
    let mut existing = recorded("orders__get_after_create.json")["order"].clone();
    existing["client_order_id"] = serde_json::json!(coid);
    existing["order_id"] = serde_json::json!(existing_id);
    existing["status"] = serde_json::json!("resting");
    mock.push_ok(
        200,
        serde_json::json!({ "orders": [existing], "cursor": "" }),
    );
    let venue = venue_with(&mock);

    let err = block_on(venue.place(gated(52, 1, 60, coid)))
        .expect_err("a duplicate place must not succeed");
    match err {
        VenueError::AlreadyExists { existing } => assert_eq!(
            existing.as_str(),
            existing_id,
            "the recorded duplicate resolves to the EXISTING order"
        ),
        other => {
            panic!("recorded 409 order_already_exists must route to AlreadyExists, got {other:?}")
        }
    }

    // POST then ONE lookup GET (found in the first status bucket → short-circuit).
    let calls = mock.calls();
    assert_eq!(calls.len(), 2, "POST then one resolve-by-coid GET");
    assert_eq!(
        (calls[0].method.as_str(), calls[0].path.as_str()),
        ("POST", "/portfolio/events/orders")
    );
    assert_eq!(
        (calls[1].method.as_str(), calls[1].path.as_str()),
        ("GET", "/portfolio/orders")
    );
}

// ===========================================================================
// cancel() — the recorded STALE-READ RACE (wire finding F16/F3). The single-
// order GET surface lags the cancel surface; the adapter now reconciles ONCE
// against the order LIST (the authoritative terminal surface) before giving up,
// and never reports a false success off a stale read.
// ===========================================================================

#[test]
fn recorded_cancel_stale_read_race_is_timeout_not_false_success() {
    // Wire finding F16/F3 replayed verbatim: the DELETE acks (orders__cancel_v2,
    // reduced_by 1.00), but the reconcile GET (orders__get_after_cancel) still
    // reads status:"resting" remaining 1.00 (the read surface lagged the cancel
    // surface ~360ms). The adapter then reconciles ONCE against the order LIST;
    // here the list ALSO lags (the same order still reads `resting`), so the
    // effect stays UNKNOWN — Timeout, never a false success off either surface.
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, recorded("orders__cancel_v2.json")); // DELETE ack
    mock.push_ok(200, recorded("orders__get_after_cancel.json")); // single-GET: still resting
    let still_resting = recorded("orders__get_after_cancel.json")["order"].clone();
    mock.push_ok(
        200,
        serde_json::json!({ "orders": [still_resting], "cursor": "" }),
    ); // list: still resting
    let venue = venue_with(&mock);

    let id = VenueOrderId::new("2597b999-f887-4195-8bac-c3f97a1f2021").unwrap();
    let err = block_on(venue.cancel(&id)).unwrap_err();
    assert!(
        matches!(err, VenueError::Timeout { .. }),
        "the recorded stale-read race must be Timeout (effect unknown), got {err:?}"
    );

    let calls = mock.calls();
    assert_eq!(
        calls.len(),
        3,
        "DELETE, stale single-GET, then ONE list reconcile"
    );
    assert_eq!(
        (calls[0].method.as_str(), calls[0].path.as_str()),
        (
            "DELETE",
            "/portfolio/events/orders/2597b999-f887-4195-8bac-c3f97a1f2021"
        )
    );
    assert_eq!(
        (calls[1].method.as_str(), calls[1].path.as_str()),
        (
            "GET",
            "/portfolio/orders/2597b999-f887-4195-8bac-c3f97a1f2021"
        )
    );
    assert_eq!(
        (calls[2].method.as_str(), calls[2].path.as_str()),
        ("GET", "/portfolio/orders"),
        "the list reconcile is GET /portfolio/orders"
    );
}

// ===========================================================================
// cancel() — F16 HARDENING: the order LIST resolves the stale-read race. The
// single-order GET lagged, but the list carries the authoritative terminal
// status, so a genuinely-canceled order resolves to Ok instead of a false
// Timeout. All three bodies are verbatim operator recordings.
// ===========================================================================

#[test]
fn recorded_cancel_stale_then_list_canceled_resolves_ok() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, recorded("orders__cancel_v2.json")); // DELETE ack
    mock.push_ok(200, recorded("orders__get_after_cancel.json")); // single-GET: stale resting
                                                                  // portfolio__orders_list carries the SAME order_id 2597b999 as `canceled`
                                                                  // (remaining 0.00) — the read surface that had caught up when the single-GET
                                                                  // had not. The adapter reconciles against it and resolves the cancel.
    mock.push_ok(200, recorded("portfolio__orders_list.json"));
    let venue = venue_with(&mock);

    let id = VenueOrderId::new("2597b999-f887-4195-8bac-c3f97a1f2021").unwrap();
    block_on(venue.cancel(&id)).expect("the list resolves the canceled order to Ok");

    let calls = mock.calls();
    assert_eq!(
        calls.len(),
        3,
        "DELETE, stale single-GET, then ONE list reconcile"
    );
    assert_eq!(
        (calls[2].method.as_str(), calls[2].path.as_str()),
        ("GET", "/portfolio/orders")
    );
}

// ===========================================================================
// cancel() — SAFETY HEADLINE + mutation proof: a re-cancel 404 is byte-identical
// for already-canceled / already-EXECUTED / never-existed (orders__cancel_*),
// so "treat recancel-404 as canceled" (fixtures README finding-16) would MASK a
// fill. The adapter instead reads the terminal status from the LIST. Order
// 97ec18b7 is genuinely `executed` in the recorded list, so a stale-read cancel
// of it routes to Rejected (fills via fills_since), NEVER a false canceled-Ok.
// If the Executed arm is ever changed to Ok(()), this test reds.
// ===========================================================================

#[test]
fn recorded_cancel_stale_then_list_executed_is_rejected_never_a_false_cancel() {
    let executed_id = "97ec18b7-10d3-4557-9de0-8598aad625f0";
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, recorded("orders__cancel_v2.json")); // DELETE ack (advisory body)
                                                           // single-GET: stale resting. The adapter reads status only (not the id) from
                                                           // the reconcile body, so re-point the recorded resting body at our order id.
    let mut stale = recorded("orders__get_after_cancel.json");
    stale["order"]["order_id"] = serde_json::json!(executed_id);
    mock.push_ok(200, stale);
    mock.push_ok(200, recorded("portfolio__orders_list.json")); // list: 97ec18b7 executed
    let venue = venue_with(&mock);

    let id = VenueOrderId::new(executed_id).unwrap();
    let result = block_on(venue.cancel(&id));
    assert!(
        !matches!(result, Ok(())),
        "an executed order must NEVER resolve to a false canceled-Ok"
    );
    match result {
        Err(VenueError::Rejected { reason }) => assert!(
            reason.contains("executed"),
            "the list-confirmed executed status must surface: {reason}"
        ),
        other => panic!("a list-confirmed executed order must be Rejected, got {other:?}"),
    }
}

// ===========================================================================
// cancel() — if the order is absent from the list page (and the single-GET was
// stale), the effect stays UNKNOWN: Timeout, never a fabricated success.
// ===========================================================================

#[test]
fn recorded_cancel_stale_then_absent_from_list_is_timeout() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, recorded("orders__cancel_v2.json"));
    mock.push_ok(200, recorded("orders__get_after_cancel.json"));
    mock.push_ok(200, serde_json::json!({ "orders": [], "cursor": "" }));
    let venue = venue_with(&mock);

    let id = VenueOrderId::new("2597b999-f887-4195-8bac-c3f97a1f2021").unwrap();
    let err = block_on(venue.cancel(&id)).unwrap_err();
    assert!(matches!(err, VenueError::Timeout { .. }), "got {err:?}");
}

// ===========================================================================
// fills_since() — the recorded fills page round-trips to canonical Fills
// ===========================================================================

#[test]
fn recorded_fills_round_trip_maps_the_taker_fill() {
    // fills__after_taker.json: 3 real taker fills, no client_order_id on the wire
    // (removed 2026-01-28), so the adapter resolves each via a GET order. Script
    // the 3 lookups (distinct order_ids) with a minimal resting order carrying a
    // coid; assert the canonical mapping of the headline taker fill (YES 0.52).
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, recorded("fills__after_taker.json"));
    for (oid, coid) in [
        ("97ec18b7-10d3-4557-9de0-8598aad625f0", "coid-a"),
        ("e8051642-92f6-4296-a244-0b02e456b6a1", "coid-b"),
        ("99ca79c3-4c96-4bfa-97c1-f924f37cc285", "coid-c"),
    ] {
        let mut order = recorded("orders__get_after_create.json");
        order["order"]["order_id"] = serde_json::json!(oid);
        order["order"]["client_order_id"] = serde_json::json!(coid);
        mock.push_ok(200, order);
    }
    let venue = venue_with(&mock);

    let page = block_on(venue.fills_since(Cursor::start())).unwrap();
    assert_eq!(page.fills.len(), 3, "all three recorded fills map");

    let taker = &page.fills[0];
    assert_eq!(taker.market.as_str(), RECORDED_TICKER);
    assert_eq!((taker.side, taker.action), (Side::Yes, Action::Buy));
    assert_eq!(taker.price, Cents::new(52), "yes_price_dollars 0.5200");
    assert_eq!(taker.qty.raw(), 1);
    assert!(!taker.is_maker, "is_taker=true on the wire");
    assert_eq!(taker.fee, Cents::new(2), "fee_cost 0.017500 -> 2c ceil");
    assert_eq!(
        taker.venue_order_id.as_str(),
        "97ec18b7-10d3-4557-9de0-8598aad625f0"
    );
    assert_eq!(
        taker.client_order_id.as_str(),
        "coid-a",
        "resolved via GET order"
    );
}

// ===========================================================================
// authenticated GET over a recorded 401 — the venue's auth-error body routes
// to Rejected with the code/detail surfaced (item 3; G1 on the auth path)
// ===========================================================================

#[test]
fn recorded_auth_401_bodies_route_to_rejected_with_the_code_surfaced() {
    // The recorded 401 auth-gateway bodies are all nested {"error":{...}}. An
    // authenticated GET (balance) over each must map to VenueError::Rejected,
    // and the venue's code/detail must reach diagnostics structured (G1) — not a
    // raw-JSON dump. (header_timestamp_expired is the skew-rejection body, the
    // adapter-mapping half of checklist #2's >5s/<30s window finding.)
    for (file, needle) in [
        ("auth__bad_signature.json", "INCORRECT_API_KEY_SIGNATURE"),
        ("auth__unknown_key_id.json", "NOT_FOUND"),
        (
            "auth__missing_signature_header.json",
            // `code=` prefix only appears under G1 STRUCTURED extraction, not a
            // raw-JSON dump (which would carry `"code":` with a colon) — so this
            // needle also proves G1 on the auth path, not just that the code leaks.
            "code=signature_is_missing_from_headers",
        ),
        ("auth__skew_minus30s.json", "code=header_timestamp_expired"),
    ] {
        let mock = Arc::new(MockKalshiTransport::new());
        mock.push_ok(401, recorded(file));
        let venue = venue_with(&mock);
        let err = block_on(venue.balance()).unwrap_err();
        match err {
            VenueError::Rejected { reason } => assert!(
                reason.contains(needle),
                "{file}: the auth error must surface in diagnostics: {reason}"
            ),
            other => panic!("{file}: a recorded 401 must route to Rejected, got {other:?}"),
        }
    }
}
