//! T1.1 tests: Kalshi V2 DTOs and the Side/Action <-> book_side mapping,
//! written from docs/research/venue/kalshi-api-2026-06-10/research.md BEFORE
//! the implementation.
//!
//! Every JSON used here lives in tests/kalshi_doc_samples/ and is DOC-DERIVED
//! (see the README there): these tests pin the adapter to the documented
//! shapes, NOT to observed venue behavior. Fixture confirmation is required
//! before paper/live (research doc Uncertainties checklist, 27 items).

use fortuna_core::market::{Action, Side};
use fortuna_core::money::Cents;
use fortuna_venues::kalshi::dto::{
    error_reason, format_count, format_price_dollars, from_direction, parse_count_ceil,
    parse_count_integral, parse_dollars_to_cents_exact, parse_fee_dollars_ceil, to_book_side,
    CancelOrderV2Response, CreateOrderV2Request, CreateOrderV2Response, GetBalanceResponse,
    GetEventResponse, GetFillsResponse, GetMarketsResponse, GetOrderResponse, GetOrderbookResponse,
    GetOrdersResponse, GetPositionsResponse, GetSeriesResponse, GetSettlementsResponse,
    KalshiBookSide, KalshiFeeType, KalshiMarketStatus, KalshiOrderStatus, KalshiOutcomeSide,
    KalshiStp, KalshiTimeInForce,
};
use fortuna_venues::VenueError;

fn sample(name: &str) -> serde_json::Value {
    let path = format!(
        "{}/tests/kalshi_doc_samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

// ---- fixed-point parsing (HARD RULE: Decimal::from_str then
// Cents::from_dollars_exact; sub-cent on linear_cent = hard error) ----

#[test]
fn dollars_parse_exact_whole_cents() {
    assert_eq!(
        parse_dollars_to_cents_exact("0.5600").unwrap(),
        Cents::new(56)
    );
    assert_eq!(
        parse_dollars_to_cents_exact("1.0000").unwrap(),
        Cents::new(100)
    );
    assert_eq!(parse_dollars_to_cents_exact("0.01").unwrap(), Cents::new(1));
    assert_eq!(parse_dollars_to_cents_exact("0").unwrap(), Cents::ZERO);
}

#[test]
fn dollars_parse_rejects_sub_cent_remainder() {
    // 0.5550 is a deci_cent price; integer-cent core => hard error.
    let err = parse_dollars_to_cents_exact("0.5550").unwrap_err();
    assert!(matches!(err, VenueError::Invalid { .. }), "got {err:?}");
    let msg = err.to_string();
    assert!(
        msg.contains("0.5550"),
        "error must name the offending value: {msg}"
    );
}

#[test]
fn dollars_parse_rejects_garbage() {
    assert!(parse_dollars_to_cents_exact("").is_err());
    assert!(parse_dollars_to_cents_exact("abc").is_err());
    assert!(parse_dollars_to_cents_exact("0.56.0").is_err());
}

#[test]
fn count_parse_requires_integral_contracts() {
    assert_eq!(parse_count_integral("10.00").unwrap().raw(), 10);
    assert_eq!(parse_count_integral("10").unwrap().raw(), 10);
    assert_eq!(parse_count_integral("0.00").unwrap().raw(), 0);
    // Fractional contracts cannot be represented by the integer core.
    let err = parse_count_integral("2.50").unwrap_err();
    assert!(matches!(err, VenueError::Invalid { .. }), "got {err:?}");
}

#[test]
fn fee_parse_rounds_up_against_us() {
    assert_eq!(parse_fee_dollars_ceil("0.1800").unwrap(), Cents::new(18));
    // Centi-cent per-fill fee (docs fee_rounding page): ceil to whole cents.
    assert_eq!(parse_fee_dollars_ceil("0.0175").unwrap(), Cents::new(2));
    assert_eq!(parse_fee_dollars_ceil("0.0000").unwrap(), Cents::ZERO);
}

#[test]
fn outbound_formatting_matches_doc_examples() {
    // Doc example uses "0.5600" / "10.00".
    assert_eq!(format_price_dollars(Cents::new(56)).unwrap(), "0.5600");
    assert_eq!(format_price_dollars(Cents::new(1)).unwrap(), "0.0100");
    assert_eq!(format_price_dollars(Cents::new(99)).unwrap(), "0.9900");
    assert_eq!(
        format_count(fortuna_core::market::Contracts::new(10)).unwrap(),
        "10.00"
    );
}

// ---- Side/Action <-> Kalshi V2 direction mapping ----
//
// Research §5a (S1 BookSide, doc-verbatim): "bid means buy YES, ask means
// sell YES. (Selling YES is economically equivalent to buying NO at
// 1 - price, but this endpoint quotes everything from the YES side.)"
// Research §5 / order-direction page equivalence table:
//   buy+yes -> yes/bid, sell+no -> yes/bid, buy+no -> no/ask, sell+yes -> no/ask.

#[test]
fn to_book_side_buy_yes_is_bid_at_own_price() {
    let (side, price) = to_book_side(Side::Yes, Action::Buy, Cents::new(56)).unwrap();
    assert_eq!(side, KalshiBookSide::Bid);
    assert_eq!(price, Cents::new(56));
}

#[test]
fn to_book_side_sell_yes_is_ask_at_own_price() {
    let (side, price) = to_book_side(Side::Yes, Action::Sell, Cents::new(56)).unwrap();
    assert_eq!(side, KalshiBookSide::Ask);
    assert_eq!(price, Cents::new(56));
}

#[test]
fn to_book_side_buy_no_is_ask_at_mirrored_price() {
    // Buying NO at 44c == selling YES at 56c; V2 quotes the YES leg.
    let (side, price) = to_book_side(Side::No, Action::Buy, Cents::new(44)).unwrap();
    assert_eq!(side, KalshiBookSide::Ask);
    assert_eq!(price, Cents::new(56));
}

#[test]
fn to_book_side_sell_no_is_bid_at_mirrored_price() {
    // Selling NO at 44c == buying YES at 56c.
    let (side, price) = to_book_side(Side::No, Action::Sell, Cents::new(44)).unwrap();
    assert_eq!(side, KalshiBookSide::Bid);
    assert_eq!(price, Cents::new(56));
}

#[test]
fn from_direction_canonicalizes_to_buy_of_outcome_side() {
    // Kalshi's V2 model collapses buy-yes/sell-no into outcome_side=yes
    // (and buy-no/sell-yes into outcome_side=no); the original action is
    // NOT recoverable from a V2 fill/order. The adapter canonicalizes to
    // Action::Buy of the outcome side, which is venue-truthful on Kalshi's
    // net-position model.
    assert_eq!(
        from_direction(KalshiOutcomeSide::Yes, KalshiBookSide::Bid).unwrap(),
        (Side::Yes, Action::Buy)
    );
    assert_eq!(
        from_direction(KalshiOutcomeSide::No, KalshiBookSide::Ask).unwrap(),
        (Side::No, Action::Buy)
    );
}

#[test]
fn from_direction_rejects_inconsistent_pairs() {
    // Order-direction page: "bid == yes, ask == no, ALWAYS". A disagreeing
    // pair means we are misreading the venue; hard error, never a guess.
    assert!(from_direction(KalshiOutcomeSide::Yes, KalshiBookSide::Ask).is_err());
    assert!(from_direction(KalshiOutcomeSide::No, KalshiBookSide::Bid).is_err());
}

// ---- doc-sample serde round trips ----

#[test]
fn create_order_v2_request_round_trips_doc_example() {
    let raw = sample("create_order_v2_request.json");
    let req: CreateOrderV2Request = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(req.ticker, "HIGHNY-24JAN01-T60");
    assert_eq!(req.side, KalshiBookSide::Bid);
    assert_eq!(req.count, "10.00");
    assert_eq!(req.price, "0.5600");
    assert_eq!(req.time_in_force, KalshiTimeInForce::GoodTillCanceled);
    assert_eq!(req.self_trade_prevention_type, KalshiStp::TakerAtCross);
    // Serializing our struct reproduces the doc example EXACTLY.
    assert_eq!(serde_json::to_value(&req).unwrap(), raw);
}

#[test]
fn create_order_v2_response_parses_doc_example() {
    let resp: CreateOrderV2Response =
        serde_json::from_value(sample("create_order_v2_response.json")).unwrap();
    assert_eq!(resp.order_id, "3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d");
    assert_eq!(
        resp.client_order_id.as_deref(),
        Some("8c35ecb3-328f-4f52-8c7c-0f4b9862f8d1")
    );
    assert_eq!(resp.fill_count, "0.00");
    assert_eq!(resp.remaining_count, "10.00");
    assert_eq!(resp.ts_ms, 1715793600123);
    assert!(resp.average_fill_price.is_none());
}

#[test]
fn create_order_v2_response_filled_carries_optional_averages() {
    let resp: CreateOrderV2Response =
        serde_json::from_value(sample("create_order_v2_response_filled.json")).unwrap();
    assert_eq!(resp.average_fill_price.as_deref(), Some("0.5600"));
    assert_eq!(resp.average_fee_paid.as_deref(), Some("0.0180"));
}

#[test]
fn cancel_order_v2_response_parses_doc_example() {
    let resp: CancelOrderV2Response =
        serde_json::from_value(sample("cancel_order_v2_response.json")).unwrap();
    assert_eq!(resp.order_id, "3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d");
    assert_eq!(resp.reduced_by, "10.00");
    assert_eq!(resp.ts_ms, 1715793660456);
}

#[test]
fn markets_response_parses_and_carries_structures() {
    let resp: GetMarketsResponse =
        serde_json::from_value(sample("markets_response_page1.json")).unwrap();
    assert_eq!(resp.cursor, "CUR-PAGE-2");
    assert_eq!(resp.markets.len(), 2);
    let m = &resp.markets[0];
    assert_eq!(m.ticker, "HIGHNY-24JAN01-T60");
    assert_eq!(m.event_ticker, "HIGHNY-24JAN01");
    assert_eq!(m.market_type, "binary");
    assert_eq!(m.status, KalshiMarketStatus::Active);
    assert_eq!(m.price_level_structure, "linear_cent");
    assert_eq!(m.notional_value_dollars, "1.0000");
    assert_eq!(m.settlement_timer_seconds, 3600);
    assert_eq!(resp.markets[1].price_level_structure, "tapered_deci_cent");
}

#[test]
fn unknown_market_status_maps_to_unknown_not_a_parse_failure() {
    // Lifecycle vocabulary is fixture-needed (checklist #21); an unlisted
    // status must degrade conservatively, not break the catalog sync.
    let mut raw = sample("markets_response_page1.json");
    raw["markets"][0]["status"] = serde_json::json!("some_future_status");
    let resp: GetMarketsResponse = serde_json::from_value(raw).unwrap();
    assert_eq!(resp.markets[0].status, KalshiMarketStatus::Unknown);
}

#[test]
fn orderbook_response_parses_doc_shape() {
    let resp: GetOrderbookResponse =
        serde_json::from_value(sample("orderbook_response.json")).unwrap();
    assert_eq!(resp.orderbook_fp.yes_dollars.len(), 2);
    assert_eq!(resp.orderbook_fp.no_dollars.len(), 2);
    // PriceLevelDollarsCountFp: [dollars_string, count_fp] — "the second
    // element is the contract quantity (not price)".
    assert_eq!(resp.orderbook_fp.yes_dollars[0][0], "0.5500");
    assert_eq!(resp.orderbook_fp.yes_dollars[0][1], "300.00");
}

#[test]
fn fills_response_parses_doc_shape() {
    let resp: GetFillsResponse = serde_json::from_value(sample("fills_response.json")).unwrap();
    assert_eq!(resp.cursor, "");
    assert_eq!(resp.fills.len(), 2);
    let f = &resp.fills[0];
    assert_eq!(f.fill_id, "d91bc706-ee49-470d-82d8-11418bda6fed");
    assert_eq!(f.order_id, "ee587a1c-8b87-4dcf-b721-9f6f790619fa");
    assert_eq!(f.ticker, "HIGHNY-24JAN01-T60");
    assert_eq!(f.outcome_side, KalshiOutcomeSide::Yes);
    assert_eq!(f.book_side, KalshiBookSide::Bid);
    assert_eq!(f.count_fp, "10.00");
    assert!(f.is_taker);
    // fee_cost is a fixed-point DOLLARS STRING (research §7), not cents.
    assert_eq!(f.fee_cost, "0.1800");
    assert!(!resp.fills[1].is_taker);
}

#[test]
fn balance_response_parses_cents_int_and_dollars_string() {
    let resp: GetBalanceResponse = serde_json::from_value(sample("balance_response.json")).unwrap();
    // `balance` is an int64 in CENTS; `balance_dollars` may carry sub-cent
    // precision for direct members (research §9).
    assert_eq!(resp.balance, 12345);
    assert_eq!(resp.balance_dollars, "123.4567");
    assert_eq!(resp.portfolio_value, 56789);
}

#[test]
fn positions_response_parses_signed_position_fp() {
    let resp: GetPositionsResponse =
        serde_json::from_value(sample("positions_response.json")).unwrap();
    assert_eq!(resp.market_positions.len(), 2);
    // "Negative means NO contracts and positive means YES contracts".
    assert_eq!(resp.market_positions[0].position_fp, "10.00");
    assert_eq!(resp.market_positions[1].position_fp, "-4.00");
}

#[test]
fn settlements_response_parses_mixed_units() {
    let resp: GetSettlementsResponse =
        serde_json::from_value(sample("settlements_response.json")).unwrap();
    let s = &resp.settlements[0];
    // Units mix is real and documented: revenue/value in INTEGER CENTS,
    // fee_cost a DOLLARS STRING (research §10; fixture checklist #19).
    assert_eq!(s.revenue, 1000);
    assert_eq!(s.value, Some(100));
    assert_eq!(s.fee_cost, "0.3400");
    assert_eq!(s.market_result, "yes");
}

#[test]
fn order_and_orders_responses_parse() {
    let one: GetOrderResponse = serde_json::from_value(sample("order_response.json")).unwrap();
    assert_eq!(one.order.order_id, "3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d");
    assert_eq!(one.order.status, KalshiOrderStatus::Resting);
    assert_eq!(one.order.outcome_side, KalshiOutcomeSide::Yes);
    assert_eq!(one.order.book_side, KalshiBookSide::Bid);
    assert_eq!(
        one.order.client_order_id,
        "8c35ecb3-328f-4f52-8c7c-0f4b9862f8d1"
    );

    let many: GetOrdersResponse = serde_json::from_value(sample("orders_response.json")).unwrap();
    assert_eq!(many.orders.len(), 2);
    assert_eq!(many.orders[1].book_side, KalshiBookSide::Ask);
    assert_eq!(many.orders[1].no_price_dollars, "0.3000");
    assert_eq!(many.cursor, "");
}

#[test]
fn series_response_parses_fee_fields() {
    let resp: GetSeriesResponse = serde_json::from_value(sample("series_response.json")).unwrap();
    assert_eq!(resp.series.ticker, "KXHIGHNY");
    assert_eq!(resp.series.category, "Climate and Weather");
    assert_eq!(resp.series.fee_type, KalshiFeeType::Quadratic);
    let maker: GetSeriesResponse =
        serde_json::from_value(sample("series_response_maker_half.json")).unwrap();
    assert_eq!(maker.series.fee_type, KalshiFeeType::QuadraticWithMakerFees);
}

#[test]
fn event_response_parses_series_ticker() {
    let resp: GetEventResponse = serde_json::from_value(sample("event_response.json")).unwrap();
    assert_eq!(resp.event.event_ticker, "HIGHNY-24JAN01");
    assert_eq!(resp.event.series_ticker, "KXHIGHNY");
}

// ---- error envelopes: BOTH documented shapes must be tolerated ----

#[test]
fn error_reason_reads_the_openapi_error_envelope() {
    let reason = error_reason(&sample("error_envelope.json"));
    assert!(reason.contains("insufficient_balance_PLACEHOLDER_FIXTURE_NEEDED"));
    assert!(reason.contains("ErrorResponse SHAPE"));
}

#[test]
fn error_reason_reads_the_429_body_shape() {
    // Rate-limit page (doc-verbatim): {"error": "too many requests"} —
    // explicitly a DIFFERENT shape from ErrorResponse.
    let reason = error_reason(&sample("rate_limit_429.json"));
    assert!(reason.contains("too many requests"));
}

#[test]
fn error_reason_survives_arbitrary_bodies() {
    let reason = error_reason(&serde_json::json!(["unexpected"]));
    assert!(!reason.is_empty());
    let reason = error_reason(&serde_json::Value::Null);
    assert!(!reason.is_empty());
}

#[test]
fn error_reason_extracts_the_nested_error_object() {
    // The most common recorded 4xx shape (17/19): {"error":{"code","message",
    // "details"}} — an OBJECT, not the 429 string. error_reason must pull the
    // nested code/message/details into the same diagnostic form as the flat shape.
    let body = serde_json::json!({
        "error": {
            "code": "order_already_exists",
            "message": "order already exists",
            "service": "exchange"
        }
    });
    let reason = error_reason(&body);
    assert!(
        reason.contains("code=order_already_exists"),
        "structured nested code: {reason}"
    );
    assert!(reason.contains("order already exists"));
}

// ---- volume parsing (T1.3: mech_extremes sub-volume filter input) ----

#[test]
fn volume_parse_ceils_fractional_contracts() {
    // Fractional volume is legitimate (0.01-contract granularity); ceil
    // OVER-states it so sub-volume market filters stay conservative.
    assert_eq!(parse_count_ceil("33896.50").unwrap(), 33_897);
    assert_eq!(parse_count_ceil("33896.00").unwrap(), 33_896);
    assert_eq!(parse_count_ceil("0.01").unwrap(), 1);
    assert_eq!(parse_count_ceil("0").unwrap(), 0);
}

#[test]
fn volume_parse_rejects_negative_and_garbage() {
    assert!(parse_count_ceil("-1.00").is_err());
    assert!(parse_count_ceil("not a number").is_err());
}
