//! T1.1 tests: KalshiVenue adapter against a scripted MockKalshiTransport,
//! written from docs/research/venue/kalshi-api-2026-06-10/research.md BEFORE
//! the implementation.
//!
//! All scripted bodies are DOC-DERIVED samples (tests/kalshi_doc_samples/,
//! see its README): they pin the adapter to the documented contract, not to
//! observed venue behavior. The adapter is NOT cleared for paper/live until
//! operator-recorded fixtures in fixtures/kalshi/ confirm the research doc's
//! 27-item Uncertainties checklist.
//!
//! Orders are minted through the REAL gate pipeline (`GatedOrder` is sealed;
//! type-level I1), so `Venue::place` is exercised exactly as production will.

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
use fortuna_venues::{Cursor, Fill, MarketFilter, MarketStatus, Venue, VenueError};
use futures::executor::block_on;
use std::collections::BTreeSet;
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

fn sample(name: &str) -> serde_json::Value {
    let path = format!(
        "{}/tests/kalshi_doc_samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn venue_with(mock: &Arc<MockKalshiTransport>, series: &[&str]) -> KalshiVenue {
    KalshiVenue::new(
        VenueId::new("kalshi").unwrap(),
        Arc::clone(mock) as Arc<dyn fortuna_venues::kalshi::client::KalshiTransport>,
        Arc::new(SimClock::new(t0())),
        series.iter().map(|s| s.to_string()).collect(),
    )
    .unwrap()
}

// ---- minting real GatedOrders through the gate pipeline ----

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
            price: Cents::new(55),
            qty: Contracts::new(1000),
        }],
        yes_asks: vec![PriceLevel {
            price: Cents::new(56),
            qty: Contracts::new(1000),
        }],
    }
}

fn gated(
    market: &str,
    side: Side,
    action: Action,
    price: i64,
    qty: i64,
    fair: i64,
    coid: &str,
) -> GatedOrder {
    let mut pipeline = GatePipeline::new(permissive_config()).unwrap();
    let market = MarketId::new(market).unwrap();
    let book = yes_book(&market);
    let fees = ZeroFees;
    let recent = BTreeSet::new();
    let mut idgen = IdGen::new(7);
    let candidate = CandidateOrder {
        intent_id: IntentId::new(idgen.next(t0()).unwrap()),
        strategy: StrategyId::new("t1").unwrap(),
        venue: VenueId::new("kalshi").unwrap(),
        market,
        side,
        action,
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
    let out = pipeline.evaluate(&candidate, &inputs);
    out.gated
        .unwrap_or_else(|r| panic!("gate rejected test order: {r:?}"))
}

fn doc_coid() -> &'static str {
    "8c35ecb3-328f-4f52-8c7c-0f4b9862f8d1"
}

fn test_fill(market: &str, price: i64, qty: i64, fee: i64, is_maker: bool) -> Fill {
    Fill {
        fill_id: "test-fill".into(),
        venue_order_id: VenueOrderId::new("test-order").unwrap(),
        client_order_id: ClientOrderId::new("test-coid").unwrap(),
        market: MarketId::new(market).unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        price: Cents::new(price),
        qty: Contracts::new(qty),
        fee: Cents::new(fee),
        is_maker,
        at: t0(),
    }
}

// ---- markets(): list + paginate + filter structures ----

#[test]
fn markets_paginates_filters_structures_and_maps_fields() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("series_response.json"));
    mock.push_ok(200, sample("markets_response_page1.json"));
    mock.push_ok(200, sample("markets_response_page2.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let markets = block_on(venue.markets(MarketFilter::default())).unwrap();

    // tapered_deci_cent, deci_cent, and scalar markets are filtered OUT
    // (GAPS rule: integer-cent core trades linear_cent binaries only).
    assert_eq!(markets.len(), 2);
    let m = &markets[0];
    assert_eq!(m.id.as_str(), "HIGHNY-24JAN01-T60");
    assert_eq!(m.venue.as_str(), "kalshi");
    assert_eq!(m.category, "Climate and Weather");
    assert_eq!(m.status, MarketStatus::Trading);
    assert_eq!(m.payout_per_contract, Cents::new(100));
    assert_eq!(
        m.close_at,
        Some(UtcTimestamp::parse_iso8601("2024-01-01T23:59:00Z").unwrap())
    );
    assert_eq!(m.settlement.oracle_type, "kalshi_rulebook");
    assert_eq!(m.settlement.resolution_source, "National Weather Service");
    assert_eq!(m.settlement.expected_lag_hours, 1);
    // finalized -> Settled; 7200s settlement timer -> 2h.
    assert_eq!(markets[1].status, MarketStatus::Settled);
    assert_eq!(markets[1].settlement.expected_lag_hours, 2);
    // volume_fp maps ceil-rounded ("33896.50" -> 33897: over-stating keeps
    // sub-volume strategy filters conservative); whole counts map exactly.
    assert_eq!(m.volume_contracts, Some(33_897));
    assert_eq!(markets[1].volume_contracts, Some(500));

    // Call sequence: series info, then cursor-paged market listing.
    let calls = mock.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        (calls[0].method.as_str(), calls[0].path.as_str()),
        ("GET", "/series/KXHIGHNY")
    );
    assert_eq!(calls[1].path, "/markets");
    let q1 = calls[1].query.clone().unwrap_or_default();
    assert!(q1.contains("series_ticker=KXHIGHNY"), "query: {q1}");
    assert!(
        !q1.contains("cursor="),
        "first page must not carry a cursor: {q1}"
    );
    let q2 = calls[2].query.clone().unwrap_or_default();
    assert!(
        q2.contains("cursor=CUR-PAGE-2"),
        "second page follows the cursor: {q2}"
    );
}

#[test]
fn markets_passes_status_filter_and_filters_client_side() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("series_response.json"));
    mock.push_ok(200, sample("markets_response_page1.json"));
    mock.push_ok(200, sample("markets_response_page2.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let markets = block_on(venue.markets(MarketFilter {
        category: None,
        status: Some(MarketStatus::Trading),
    }))
    .unwrap();

    // Only the active linear_cent market survives.
    assert_eq!(markets.len(), 1);
    assert_eq!(markets[0].id.as_str(), "HIGHNY-24JAN01-T60");
    // Filter vocabulary mapping Trading -> open (fixture checklist #21).
    let q = mock.calls()[1].query.clone().unwrap_or_default();
    assert!(q.contains("status=open"), "query: {q}");
}

#[test]
fn markets_with_no_configured_series_is_empty_and_silent() {
    let mock = Arc::new(MockKalshiTransport::new());
    let venue = venue_with(&mock, &[]);
    let markets = block_on(venue.markets(MarketFilter::default())).unwrap();
    assert!(markets.is_empty());
    assert!(mock.calls().is_empty());
}

// ---- book(): Kalshi yes/no bid arrays -> canonical YES book ----

#[test]
fn book_maps_no_bids_to_yes_asks_at_mirrored_prices() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("orderbook_response.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let market = MarketId::new("HIGHNY-24JAN01-T60").unwrap();
    let book = block_on(venue.book(&market)).unwrap();

    // Research §4 (doc-verbatim): the book "returns yes bids and no bids
    // only (no asks are returned)... a bid for yes at price X is equivalent
    // to an ask for no at price (100-X)". Canonical form: NO bid at q ==
    // YES ask at 100-q.
    assert_eq!(book.yes_bids.len(), 2);
    assert_eq!(
        (book.yes_bids[0].price, book.yes_bids[0].qty.raw()),
        (Cents::new(55), 300)
    );
    assert_eq!(
        (book.yes_bids[1].price, book.yes_bids[1].qty.raw()),
        (Cents::new(54), 120)
    );
    assert_eq!(book.yes_asks.len(), 2);
    assert_eq!(
        (book.yes_asks[0].price, book.yes_asks[0].qty.raw()),
        (Cents::new(56), 150)
    );
    assert_eq!(
        (book.yes_asks[1].price, book.yes_asks[1].qty.raw()),
        (Cents::new(57), 75)
    );
    book.validate().unwrap();

    let calls = mock.calls();
    assert_eq!(calls[0].path, "/markets/HIGHNY-24JAN01-T60/orderbook");
}

#[test]
fn book_with_sub_cent_level_is_a_hard_error() {
    let mut body = sample("orderbook_response.json");
    body["orderbook_fp"]["yes_dollars"][0][0] = serde_json::json!("0.5550");
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, body);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let market = MarketId::new("HIGHNY-24JAN01-T60").unwrap();
    let err = block_on(venue.book(&market)).unwrap_err();
    assert!(matches!(err, VenueError::Invalid { .. }), "got {err:?}");
    assert!(err.to_string().contains("0.5550"));
}

// ---- place(): V2 create ----

#[test]
fn place_sends_the_documented_v2_body_exactly() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(201, sample("create_order_v2_response.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let order = gated(
        "HIGHNY-24JAN01-T60",
        Side::Yes,
        Action::Buy,
        56,
        10,
        60,
        doc_coid(),
    );
    let id = block_on(venue.place(order)).unwrap();
    assert_eq!(id.as_str(), "3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d");

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].method.as_str(), calls[0].path.as_str()),
        ("POST", "/portfolio/events/orders")
    );
    // Body must equal the doc-archived V2 example EXACTLY (bid/buy-YES at
    // the YES-leg price; string count and price).
    assert_eq!(
        calls[0].body.clone().unwrap(),
        sample("create_order_v2_request.json")
    );
}

#[test]
fn place_no_side_quotes_the_yes_leg() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(201, sample("create_order_v2_response.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    // Buy NO at 44c == ask on the YES leg at 56c (research §5a BookSide).
    let order = gated(
        "HIGHNY-24JAN01-T60",
        Side::No,
        Action::Buy,
        44,
        10,
        50,
        doc_coid(),
    );
    block_on(venue.place(order)).unwrap();

    let body = mock.calls()[0].body.clone().unwrap();
    assert_eq!(body["side"], "ask");
    assert_eq!(body["price"], "0.5600");
    assert_eq!(body["count"], "10.00");
}

#[test]
fn place_duplicate_409_resolves_existing_order() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(409, sample("error_409_duplicate.json"));
    // Adapter looks the order up by client_order_id (the 409 body is
    // UNDOCUMENTED — fixture checklist #7 — so it must not be parsed for
    // anything load-bearing).
    mock.push_ok(200, sample("orders_response.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let order = gated(
        "HIGHNY-24JAN01-T60",
        Side::Yes,
        Action::Buy,
        56,
        10,
        60,
        doc_coid(),
    );
    let err = block_on(venue.place(order)).unwrap_err();
    match err {
        VenueError::AlreadyExists { existing } => {
            assert_eq!(existing.as_str(), "3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d");
        }
        other => panic!("expected AlreadyExists, got {other:?}"),
    }
    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    let q = calls[1].query.clone().unwrap_or_default();
    assert!(
        q.contains("status=resting"),
        "lookup starts with resting orders: {q}"
    );
}

#[test]
fn place_duplicate_409_unresolvable_is_timeout_semantics() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(409, sample("error_409_duplicate.json"));
    let empty = serde_json::json!({ "orders": [], "cursor": "" });
    // One empty page per lookup status bucket (resting, executed, canceled).
    mock.push_ok(200, empty.clone());
    mock.push_ok(200, empty.clone());
    mock.push_ok(200, empty);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let order = gated(
        "HIGHNY-24JAN01-T60",
        Side::Yes,
        Action::Buy,
        56,
        10,
        60,
        doc_coid(),
    );
    let err = block_on(venue.place(order)).unwrap_err();
    // Venue says the order exists but we cannot locate it: the only safe
    // claim is "effect unknown; reconcile" (VenueError::Timeout semantics).
    assert!(matches!(err, VenueError::Timeout { .. }), "got {err:?}");
}

#[test]
fn place_400_is_rejected_with_the_venue_reason() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(400, sample("error_envelope.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let order = gated(
        "HIGHNY-24JAN01-T60",
        Side::Yes,
        Action::Buy,
        56,
        10,
        60,
        doc_coid(),
    );
    let err = block_on(venue.place(order)).unwrap_err();
    match err {
        VenueError::Rejected { reason } => {
            assert!(reason.contains("insufficient_balance_PLACEHOLDER_FIXTURE_NEEDED"));
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[test]
fn place_5xx_is_effect_unknown_timeout() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(500, serde_json::Value::Null);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let order = gated(
        "HIGHNY-24JAN01-T60",
        Side::Yes,
        Action::Buy,
        56,
        10,
        60,
        doc_coid(),
    );
    let err = block_on(venue.place(order)).unwrap_err();
    // A 5xx on a mutation does NOT mean "no effect": the order may exist.
    assert!(matches!(err, VenueError::Timeout { .. }), "got {err:?}");
}

#[test]
fn place_propagates_transport_rate_limit() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_err(VenueError::RateLimited);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let order = gated(
        "HIGHNY-24JAN01-T60",
        Side::Yes,
        Action::Buy,
        56,
        10,
        60,
        doc_coid(),
    );
    let err = block_on(venue.place(order)).unwrap_err();
    assert!(matches!(err, VenueError::RateLimited), "got {err:?}");
}

// ---- cancel(): DELETE + mandatory reconcile-after-cancel ----
//
// Research §6 (changelog May 21, 2026): V2 cancel responses "may not
// correspond to your request... only the response body is affected", so the
// adapter must confirm via GET and never trust the DELETE body.

#[test]
fn cancel_reconciles_via_get_and_ignores_the_buggy_delete_body() {
    let mock = Arc::new(MockKalshiTransport::new());
    let mut wrong_body = sample("cancel_order_v2_response.json");
    // Script the documented bug: the DELETE response describes a DIFFERENT
    // order.
    wrong_body["order_id"] = serde_json::json!("ffffffff-0000-4000-8000-00000000beef");
    wrong_body["client_order_id"] = serde_json::json!("someone-elses-order");
    mock.push_ok(200, wrong_body);
    let mut confirmed = sample("order_response.json");
    confirmed["order"]["status"] = serde_json::json!("canceled");
    mock.push_ok(200, confirmed);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let id = VenueOrderId::new("3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d").unwrap();
    block_on(venue.cancel(&id)).unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        (calls[0].method.as_str(), calls[0].path.as_str()),
        (
            "DELETE",
            "/portfolio/events/orders/3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d"
        )
    );
    assert_eq!(
        (calls[1].method.as_str(), calls[1].path.as_str()),
        (
            "GET",
            "/portfolio/orders/3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d"
        )
    );
}

#[test]
fn cancel_with_order_still_resting_is_timeout_semantics() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("cancel_order_v2_response.json"));
    // Reconcile GET says the order still rests: cancel effect is unknown.
    mock.push_ok(200, sample("order_response.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let id = VenueOrderId::new("3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d").unwrap();
    let err = block_on(venue.cancel(&id)).unwrap_err();
    assert!(matches!(err, VenueError::Timeout { .. }), "got {err:?}");
}

#[test]
fn cancel_of_fully_executed_order_is_rejected() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("cancel_order_v2_response.json"));
    let mut executed = sample("order_response.json");
    executed["order"]["status"] = serde_json::json!("executed");
    executed["order"]["remaining_count_fp"] = serde_json::json!("0.00");
    executed["order"]["fill_count_fp"] = serde_json::json!("10.00");
    mock.push_ok(200, executed);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let id = VenueOrderId::new("3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d").unwrap();
    let err = block_on(venue.cancel(&id)).unwrap_err();
    // Cancel had no effect; the order filled. Fills arrive via fills_since.
    assert!(matches!(err, VenueError::Rejected { .. }), "got {err:?}");
}

#[test]
fn cancel_unknown_order_is_not_found_without_reconcile() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(404, sample("error_envelope.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let id = VenueOrderId::new("does-not-exist").unwrap();
    let err = block_on(venue.cancel(&id)).unwrap_err();
    assert!(matches!(err, VenueError::NotFound { .. }), "got {err:?}");
    assert_eq!(mock.calls().len(), 1, "no reconcile GET after a clean 404");
}

// ---- open_orders / positions / balance ----

#[test]
fn open_orders_maps_resting_orders_canonically() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("orders_response.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let orders = block_on(venue.open_orders()).unwrap();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].market.as_str(), "HIGHNY-24JAN01-T60");
    assert_eq!((orders[0].side, orders[0].action), (Side::Yes, Action::Buy));
    assert_eq!(orders[0].limit_price, Cents::new(56));
    assert_eq!(orders[0].remaining_qty.raw(), 10);
    assert_eq!(orders[0].client_order_id.as_str(), doc_coid());
    // outcome_side=no/book_side=ask order: NO-space price (0.3000 -> 30c).
    assert_eq!((orders[1].side, orders[1].action), (Side::No, Action::Buy));
    assert_eq!(orders[1].limit_price, Cents::new(30));
    assert_eq!(orders[1].remaining_qty.raw(), 4);

    let q = mock.calls()[0].query.clone().unwrap_or_default();
    assert!(q.contains("status=resting"), "query: {q}");
}

#[test]
fn positions_split_signed_position_fp_into_yes_no_lots() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("positions_response.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let positions = block_on(venue.positions()).unwrap();
    assert_eq!(positions.len(), 2);
    // position_fp "10.00" -> long 10 YES.
    assert_eq!(positions[0].market.as_str(), "HIGHNY-24JAN01-T60");
    assert_eq!((positions[0].yes, positions[0].no), (10, 0));
    assert_eq!(positions[0].cost, Cents::new(560));
    // position_fp "-4.00" -> long 4 NO ("Negative means NO contracts").
    assert_eq!((positions[1].yes, positions[1].no), (0, 4));
    assert_eq!(positions[1].cost, Cents::new(120));

    let q = mock.calls()[0].query.clone().unwrap_or_default();
    assert!(q.contains("count_filter=position"), "query: {q}");
}

#[test]
fn positions_with_fractional_contracts_are_a_hard_error() {
    let mut body = sample("positions_response.json");
    body["market_positions"][0]["position_fp"] = serde_json::json!("2.50");
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, body);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let err = block_on(venue.positions()).unwrap_err();
    assert!(matches!(err, VenueError::Invalid { .. }), "got {err:?}");
}

#[test]
fn balance_uses_the_integer_cent_field() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("balance_response.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    // balance=12345 cents; balance_dollars="123.4567" would NOT be exactly
    // representable — the truncating integer field is the conservative
    // choice for available cash (never overstates).
    assert_eq!(block_on(venue.balance()).unwrap(), Cents::new(12345));
    assert_eq!(mock.calls()[0].path, "/portfolio/balance");
}

// ---- fills_since ----

#[test]
fn fills_map_directions_prices_fees_and_resolve_client_order_id() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("fills_response.json"));
    // Both fills share one unknown order_id -> exactly ONE GET order to
    // resolve the client_order_id (then cached).
    let mut order = sample("order_response.json");
    order["order"]["order_id"] = serde_json::json!("ee587a1c-8b87-4dcf-b721-9f6f790619fa");
    order["order"]["client_order_id"] = serde_json::json!("fortuna-resolved-coid");
    mock.push_ok(200, order);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let page = block_on(venue.fills_since(Cursor::start())).unwrap();
    assert_eq!(page.fills.len(), 2);

    let f = &page.fills[0];
    assert_eq!(f.fill_id, "d91bc706-ee49-470d-82d8-11418bda6fed");
    assert_eq!(
        f.venue_order_id.as_str(),
        "ee587a1c-8b87-4dcf-b721-9f6f790619fa"
    );
    assert_eq!(f.client_order_id.as_str(), "fortuna-resolved-coid");
    assert_eq!((f.side, f.action), (Side::Yes, Action::Buy));
    assert_eq!(f.price, Cents::new(56));
    assert_eq!(f.qty.raw(), 10);
    // fee_cost "0.1800" -> 18 cents; is_maker = !is_taker.
    assert_eq!(f.fee, Cents::new(18));
    assert!(!f.is_maker);
    assert_eq!(
        f.at,
        UtcTimestamp::parse_iso8601("2024-01-01T12:00:00Z").unwrap()
    );

    let g = &page.fills[1];
    // outcome_side=no fill: NO-space price (0.4400 -> 44c).
    assert_eq!((g.side, g.action), (Side::No, Action::Buy));
    assert_eq!(g.price, Cents::new(44));
    assert_eq!(g.qty.raw(), 5);
    assert!(g.is_maker);
    assert_eq!(g.fee, Cents::ZERO);

    let calls = mock.calls();
    assert_eq!(calls.len(), 2, "one fills page + ONE order lookup (cached)");
    assert_eq!(
        calls[1].path,
        "/portfolio/orders/ee587a1c-8b87-4dcf-b721-9f6f790619fa"
    );
}

#[test]
fn fills_skip_order_lookup_when_coid_already_known_from_place() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(201, sample("create_order_v2_response.json"));
    let mut fills = sample("fills_response.json");
    fills["fills"][0]["order_id"] = serde_json::json!("3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d");
    fills["fills"].as_array_mut().unwrap().truncate(1);
    mock.push_ok(200, fills);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let order = gated(
        "HIGHNY-24JAN01-T60",
        Side::Yes,
        Action::Buy,
        56,
        10,
        60,
        doc_coid(),
    );
    block_on(venue.place(order)).unwrap();
    let page = block_on(venue.fills_since(Cursor::start())).unwrap();

    assert_eq!(page.fills[0].client_order_id.as_str(), doc_coid());
    let calls = mock.calls();
    assert_eq!(calls.len(), 2, "place + fills page; NO extra order lookup");
}

#[test]
fn fills_cursor_semantics_are_at_least_once() {
    // Terminal page (empty venue cursor): next_cursor stays at the polled
    // cursor so the next poll re-reads from the same point and can see new
    // fills. At-least-once redelivery is allowed; consumers dedup on
    // fill_id. (Venue cursor stability across inserts is fixture checklist
    // #17.)
    let mock = Arc::new(MockKalshiTransport::new());
    let mut terminal = sample("fills_response.json");
    terminal["fills"][0]["order_id"] = serde_json::json!("3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d");
    terminal["fills"][1]["order_id"] = serde_json::json!("3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d");
    let mut more = terminal.clone();
    more["cursor"] = serde_json::json!("F2");
    // Queue (FIFO): fills page 1 (terminal), the ONE order lookup it
    // triggers, then fills page 2 (the coid is cached by then).
    mock.push_ok(200, terminal);
    let mut order = sample("order_response.json");
    order["order"]["order_id"] = serde_json::json!("3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d");
    mock.push_ok(200, order);
    mock.push_ok(200, more);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let page1 = block_on(venue.fills_since(Cursor("ABC".into()))).unwrap();
    let page2 = block_on(venue.fills_since(Cursor("ABC".into()))).unwrap();

    let calls = mock.calls();
    let q1 = calls[0].query.clone().unwrap_or_default();
    assert!(q1.contains("cursor=ABC"), "polled cursor forwarded: {q1}");
    // Empty venue cursor -> stay on the polled cursor (page1), non-empty ->
    // advance to it (page2).
    assert_eq!(page1.next_cursor, Cursor("ABC".into()));
    assert_eq!(page2.next_cursor, Cursor("F2".into()));
}

#[test]
fn fills_with_fractional_count_are_a_hard_error() {
    let mut body = sample("fills_response.json");
    body["fills"][0]["count_fp"] = serde_json::json!("2.50");
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, body);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let err = block_on(venue.fills_since(Cursor::start())).unwrap_err();
    assert!(matches!(err, VenueError::Invalid { .. }), "got {err:?}");
}

// ---- fee model + reconciliation ----

#[test]
fn fee_model_is_the_researched_kalshi_schedule() {
    let mock = Arc::new(MockKalshiTransport::new());
    let venue = venue_with(&mock, &["KXHIGHNY"]);
    let fees = venue.fee_model();
    // Fee schedule PDF (effective 2026-02-05): taker ceil(0.07*C*P*(1-P)),
    // maker ceil(0.0175*C*P*(1-P)). Worked examples from the research doc.
    let at = t0();
    assert_eq!(
        fees.fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at
        )
        .unwrap(),
        Cents::new(175)
    );
    assert_eq!(
        fees.fee(
            FillRole::Maker,
            Cents::new(50),
            Contracts::new(100),
            None,
            at
        )
        .unwrap(),
        Cents::new(44)
    );
    assert_eq!(
        fees.fee(
            FillRole::Taker,
            Cents::new(25),
            Contracts::new(100),
            None,
            at
        )
        .unwrap(),
        Cents::new(132)
    );
    assert_eq!(
        fees.fee(
            FillRole::Taker,
            Cents::new(1),
            Contracts::new(100),
            None,
            at
        )
        .unwrap(),
        Cents::new(7)
    );
}

#[test]
fn fee_model_has_no_schedule_before_the_effective_date() {
    let mock = Arc::new(MockKalshiTransport::new());
    let venue = venue_with(&mock, &["KXHIGHNY"]);
    let before = UtcTimestamp::parse_iso8601("2026-02-04T00:00:00Z").unwrap();
    let err = venue
        .fee_model()
        .fee(
            FillRole::Taker,
            Cents::new(50),
            Contracts::new(100),
            None,
            before,
        )
        .unwrap_err();
    assert!(
        matches!(err, FeeError::NoEffectiveSchedule { .. }),
        "got {err:?}"
    );
}

#[test]
fn reconcile_fee_matches_on_quadratic_series() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("series_response.json"));
    mock.push_ok(200, sample("markets_response_page1.json"));
    mock.push_ok(200, sample("markets_response_page2.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);
    block_on(venue.markets(MarketFilter::default())).unwrap();

    // Taker 10 @ 56c on multiplier-1 quadratic: ceil(0.07*10*0.56*0.44) =
    // ceil(0.17248) = $0.18.
    let rec = venue
        .reconcile_fee(&test_fill("HIGHNY-24JAN01-T60", 56, 10, 18, false))
        .unwrap();
    assert_eq!(rec.modeled, Cents::new(18));
    assert_eq!(rec.charged, Cents::new(18));
    assert!(rec.matches);

    // Maker on a plain quadratic series pays ZERO (maker fees exist only on
    // quadratic_with_maker_fees series).
    let rec = venue
        .reconcile_fee(&test_fill("HIGHNY-24JAN01-T60", 56, 10, 0, true))
        .unwrap();
    assert_eq!(rec.modeled, Cents::ZERO);
    assert!(rec.matches);
}

#[test]
fn reconcile_fee_flags_overcharge_and_model_drift() {
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("series_response.json"));
    mock.push_ok(200, sample("markets_response_page1.json"));
    mock.push_ok(200, sample("markets_response_page2.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);
    block_on(venue.markets(MarketFilter::default())).unwrap();

    // Venue charged MORE than modeled -> mismatch.
    let rec = venue
        .reconcile_fee(&test_fill("HIGHNY-24JAN01-T60", 56, 10, 19, false))
        .unwrap();
    assert!(!rec.matches);
    // Venue charged a cent LESS (per-order rounding rebate, documented in
    // the fee_rounding page) -> still a match.
    let rec = venue
        .reconcile_fee(&test_fill("HIGHNY-24JAN01-T60", 56, 10, 17, false))
        .unwrap();
    assert!(rec.matches);
    // Way off -> mismatch.
    let rec = venue
        .reconcile_fee(&test_fill("HIGHNY-24JAN01-T60", 56, 10, 2, false))
        .unwrap();
    assert!(!rec.matches);
}

#[test]
fn reconcile_fee_unknown_market_is_an_error_not_a_guess() {
    let mock = Arc::new(MockKalshiTransport::new());
    let venue = venue_with(&mock, &["KXHIGHNY"]);
    let err = venue
        .reconcile_fee(&test_fill("NEVER-SEEN-MKT", 56, 10, 18, false))
        .unwrap_err();
    assert!(matches!(err, VenueError::Invalid { .. }), "got {err:?}");
}

#[test]
fn ensure_series_for_market_resolves_and_applies_the_multiplier() {
    let mock = Arc::new(MockKalshiTransport::new());
    // GET /markets/{ticker} -> market in event INXY-26.
    let mut market =
        serde_json::json!({ "market": sample("markets_response_page1.json")["markets"][0] });
    market["market"]["ticker"] = serde_json::json!("INXY-26-B5000");
    market["market"]["event_ticker"] = serde_json::json!("INXY-26");
    mock.push_ok(200, market);
    // GET /events/INXY-26 -> series KXINXY.
    let mut event = sample("event_response.json");
    event["event"]["event_ticker"] = serde_json::json!("INXY-26");
    event["event"]["series_ticker"] = serde_json::json!("KXINXY");
    mock.push_ok(200, event);
    // GET /series/KXINXY -> quadratic_with_maker_fees, multiplier 0.5.
    mock.push_ok(200, sample("series_response_maker_half.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let market_id = MarketId::new("INXY-26-B5000").unwrap();
    block_on(venue.ensure_series_for_market(&market_id)).unwrap();

    // Research worked example: maker 100 @ 50c at multiplier 0.5 ->
    // ceil(0.5 * 0.0175 * 100 * 0.25) = $0.22.
    let rec = venue
        .reconcile_fee(&test_fill("INXY-26-B5000", 50, 100, 22, true))
        .unwrap();
    assert_eq!(rec.modeled, Cents::new(22));
    assert!(rec.matches);
    // Taker at multiplier 0.5: ceil(0.035 * 100 * 0.25) = $0.88.
    let rec = venue
        .reconcile_fee(&test_fill("INXY-26-B5000", 50, 100, 88, false))
        .unwrap();
    assert_eq!(rec.modeled, Cents::new(88));
    assert!(rec.matches);

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].path, "/markets/INXY-26-B5000");
    assert_eq!(calls[1].path, "/events/INXY-26");
    assert_eq!(calls[2].path, "/series/KXINXY");
}

#[test]
fn venue_id_is_what_was_configured() {
    let mock = Arc::new(MockKalshiTransport::new());
    let venue = venue_with(&mock, &["KXHIGHNY"]);
    assert_eq!(venue.id().as_str(), "kalshi");
}

// ---- settlements_since (T1.4): the 5.13 reconciliation input ----

#[test]
fn settlements_map_market_result_and_hold_cursor_on_terminal_page() {
    use fortuna_venues::SettlementOutcome;
    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, sample("settlements_response.json"));
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let page = block_on(venue.settlements_since(Cursor::start())).unwrap();
    assert_eq!(page.notices.len(), 1);
    let n = &page.notices[0];
    assert_eq!(n.market.as_str(), "HIGHNY-24JAN02-T55");
    assert_eq!(
        n.outcome,
        SettlementOutcome::Winner(fortuna_core::market::Side::Yes)
    );
    assert_eq!(
        n.at,
        UtcTimestamp::parse_iso8601("2024-01-03T00:30:00Z").unwrap()
    );
    // The raw venue record rides along for audit + payout reconciliation.
    assert_eq!(n.detail["revenue"], 1000);
    // Doc sample has cursor "": terminal page holds the polled cursor.
    assert_eq!(page.next_cursor, Cursor::start());

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path, "/portfolio/settlements");
}

#[test]
fn settlement_with_undocumented_market_result_is_a_hard_error() {
    let mock = Arc::new(MockKalshiTransport::new());
    let mut body = sample("settlements_response.json");
    body["settlements"][0]["market_result"] = serde_json::json!("voided_maybe");
    mock.push_ok(200, body);
    let venue = venue_with(&mock, &["KXHIGHNY"]);

    let err = block_on(venue.settlements_since(Cursor::start())).unwrap_err();
    assert!(
        matches!(err, VenueError::Invalid { .. }),
        "undocumented market_result must fail loud (void representation \
         is a fixture-confirmation item), got {err:?}"
    );
}
