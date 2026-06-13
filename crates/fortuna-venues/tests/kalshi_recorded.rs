//! T4.2 item 2(iii) — Kalshi adapter paper-clearance, CLUSTER 1: assert the
//! adapter's DTO / error-body / unit parsing against the OPERATOR-RECORDED
//! fixtures in fixtures/kalshi/ (demo env, 2026-06-11). Until now ZERO tests
//! loaded the recorded fixtures — every adapter test used doc-derived samples
//! (tests/kalshi_doc_samples/). These tests make the clearance record's
//! "the adapter handles the wire the venue ACTUALLY sent" claim executably true.
//!
//! Scope (Cluster 1): parsing / error-shape / units / status-vocabulary — load
//! a recorded body, parse it through the adapter's public DTO + parsing fns,
//! assert per the README "Load-bearing wire findings". Transport round-trips
//! (place/cancel/fills flows via MockKalshiTransport) are Cluster 2; auth-skew +
//! WS-frame replay are Cluster 3. Clearance record:
//! docs/design/track-a-kalshi-paper-clearance.md.
//!
//! Honesty rules honored here:
//! - Assertions on `error_reason` use CONTAINS, never equality on the raw body,
//!   so they stay true whether the code surfaces the venue code via raw-JSON
//!   fallback (today) or structured extraction (after the ledgered DTO fix) —
//!   the tests do not enshrine the current gap.
//! - The wire SHAPE is pinned at the serde_json::Value level (ground truth that
//!   never drifts), independent of how the adapter renders it.

use fortuna_core::money::Cents;
use fortuna_venues::kalshi::dto::{
    error_reason, parse_count_integral, parse_dollars_to_cents_exact, parse_fee_dollars_ceil,
    GetBalanceResponse, GetFillsResponse, GetMarketsResponse, GetOrderbookResponse,
    GetPositionsResponse, GetSettlementsResponse, KalshiBookSide, KalshiMarketStatus,
    KalshiOutcomeSide,
};
use serde::Deserialize;

const RECORDED_TICKER: &str = "KXWTACHALLENGERMATCH-26JUN11JIMLEP-LEP";

/// Load an OPERATOR-RECORDED fixture body (verbatim venue response).
fn recorded(name: &str) -> serde_json::Value {
    let path = format!(
        "{}/../../fixtures/kalshi/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse fixture {path}: {e}"))
}

fn parse<T: for<'de> Deserialize<'de>>(name: &str) -> T {
    serde_json::from_value(recorded(name)).unwrap_or_else(|e| {
        panic!(
            "deserialize {name} into {}: {e}",
            std::any::type_name::<T>()
        )
    })
}

// ===========================================================================
// Units / types (checklist #19) — balance, fills, positions
// ===========================================================================

#[test]
fn recorded_balance_is_integer_cents_plus_a_separate_dollars_string() {
    // auth__balance_ok.json: {"balance":9801,...,"balance_dollars":"98.0186",...}
    // The adapter uses the TRUNCATING integer-cent field (never overstates cash);
    // balance_dollars carries sub-cent precision (98.0186 != 98.01) — exactly the
    // documented split (dto.rs GetBalanceResponse).
    let b: GetBalanceResponse = parse("auth__balance_ok.json");
    assert_eq!(b.balance, 9801, "integer cents, truncated");
    assert_eq!(b.balance_dollars.as_str(), "98.0186", "4dp dollars string");
    assert_eq!(b.portfolio_value, 198);
    assert_eq!(b.updated_ts, 1781159348);
    // The adapter's balance() returns Cents::new(balance):
    assert_eq!(Cents::new(b.balance).raw(), 9801);
}

#[test]
fn recorded_taker_fill_has_6dp_dollar_fee_and_yes_bid_direction() {
    // fills__after_taker.json: 3 taker fills; cursor "" (terminal page).
    let f: GetFillsResponse = parse("fills__after_taker.json");
    assert_eq!(f.fills.len(), 3);
    assert!(
        f.cursor.is_empty(),
        "wire finding 11: last page cursor is empty string"
    );
    let first = &f.fills[0];
    assert_eq!(first.fee_cost.as_str(), "0.017500", "6dp dollars string");
    assert!(first.is_taker);
    assert_eq!(first.outcome_side, KalshiOutcomeSide::Yes);
    assert_eq!(first.book_side, KalshiBookSide::Bid);
    assert_eq!(first.yes_price_dollars.as_str(), "0.5200");
    assert_eq!(first.count_fp.as_str(), "1.00");
    assert_eq!(first.ticker.as_str(), RECORDED_TICKER);
}

#[test]
fn recorded_taker_fee_matches_quadratic_007_and_ceils_against_us() {
    // Wire finding 9: a real fill at YES 0.52 charged fee 0.017500.
    // Quadratic 0.07*p*(1-p) = 0.07*0.52*0.48 = 0.017472; the wire rounds the
    // CHARGED fee to 0.017500 (4dp granularity). The integer-cent view ceils:
    // 0.017500 dollars = 1.75c -> 2c (rounding against us).
    assert_eq!(parse_fee_dollars_ceil("0.017500").unwrap(), Cents::new(2));
    // And the price parses exact:
    assert_eq!(
        parse_dollars_to_cents_exact("0.5200").unwrap(),
        Cents::new(52)
    );
    // The cheaper fills (YES 0.99) charged 0.000700 -> 1c ceil:
    assert_eq!(parse_fee_dollars_ceil("0.000700").unwrap(), Cents::new(1));
}

#[test]
fn recorded_positions_carry_a_signed_yes_position_extra_fields_ignored() {
    // portfolio__positions.json carries event_positions + last_updated_ts +
    // resting_orders_count (NOT in the adapter DTO) — serde must ignore them.
    let p: GetPositionsResponse = parse("portfolio__positions.json");
    assert_eq!(p.cursor.as_deref(), Some(""));
    assert_eq!(p.market_positions.len(), 1);
    let mp = &p.market_positions[0];
    assert_eq!(mp.ticker.as_str(), RECORDED_TICKER);
    assert_eq!(mp.position_fp.as_str(), "3.00", "positive = YES contracts");
    assert_eq!(mp.market_exposure_dollars.as_str(), "2.500000");
    // position_fp parses to whole contracts:
    assert_eq!(parse_count_integral("3.00").unwrap().raw(), 3);
}

// ===========================================================================
// Market status vocabulary (checklist #21) — response enum vs query filter
// ===========================================================================

#[test]
fn recorded_active_market_parses_on_a_terminal_page() {
    // markets__single_filter_lastpage.json: one active binary linear_cent market,
    // cursor "" (terminal).
    let m: GetMarketsResponse = parse("markets__single_filter_lastpage.json");
    assert!(m.cursor.is_empty(), "terminal page cursor is empty string");
    assert_eq!(m.markets.len(), 1);
    let mk = &m.markets[0];
    assert_eq!(mk.ticker.as_str(), RECORDED_TICKER);
    assert_eq!(mk.status, KalshiMarketStatus::Active);
    assert_eq!(mk.market_type.as_str(), "binary");
    assert_eq!(mk.price_level_structure.as_str(), "linear_cent");
}

#[test]
fn recorded_closed_filter_returns_determined_not_a_closed_status() {
    // Wire/vocab finding: querying ?status=closed returns markets whose RESPONSE
    // status is the lifecycle value `determined` — the query vocabulary and the
    // response vocabulary differ (checklist #21).
    let m: GetMarketsResponse = parse("markets__status_closed.json");
    assert!(!m.markets.is_empty());
    assert!(
        m.markets
            .iter()
            .all(|mk| mk.status != KalshiMarketStatus::Closed),
        "response uses lifecycle vocab, never the query token `closed`"
    );
    assert!(m
        .markets
        .iter()
        .any(|mk| mk.status == KalshiMarketStatus::Determined));
}

#[test]
fn recorded_settled_filter_returns_finalized_status() {
    let m: GetMarketsResponse = parse("markets__status_settled.json");
    assert!(m
        .markets
        .iter()
        .any(|mk| mk.status == KalshiMarketStatus::Finalized));
}

// ===========================================================================
// REST orderbook no-leg pricing (checklist #20)
// ===========================================================================

#[test]
fn recorded_orderbook_no_dollars_are_no_leg_priced_and_mirror_to_yes() {
    // orderbook__base.json: yes_dollars [["0.4700","3.00"]], no_dollars [["0.4800","3.00"]].
    // The second element is the contract QUANTITY, not a price. The no_dollars
    // level is in NO-leg pricing: a NO bid at 48c <=> a YES ask at 100-48 = 52c.
    let ob: GetOrderbookResponse = parse("orderbook__base.json");
    assert_eq!(ob.orderbook_fp.yes_dollars.len(), 1);
    assert_eq!(ob.orderbook_fp.no_dollars.len(), 1);
    assert_eq!(ob.orderbook_fp.yes_dollars[0][0].as_str(), "0.4700");
    assert_eq!(
        ob.orderbook_fp.yes_dollars[0][1].as_str(),
        "3.00",
        "qty, not price"
    );
    assert_eq!(ob.orderbook_fp.no_dollars[0][0].as_str(), "0.4800");

    let yes_bid = parse_dollars_to_cents_exact("0.4700").unwrap();
    let no_bid = parse_dollars_to_cents_exact("0.4800").unwrap();
    assert_eq!(yes_bid, Cents::new(47));
    assert_eq!(no_bid, Cents::new(48));
    // NO bid 48c mirrors to a YES ask at 52c => best YES bid 47 < best YES ask 52
    // (consistent with the recorded WS book for the same market).
    assert_eq!(100 - no_bid.raw(), 52);
}

// ===========================================================================
// Settlements (checklist #19, partial) — empty page parse + terminal cursor
// ===========================================================================

#[test]
fn recorded_settlements_page_is_empty_with_a_terminal_cursor() {
    // The session seeded a position but the market had not settled by session
    // end, so the settlements page is empty. This confirms the empty-page parse
    // + terminal-cursor handling; settlement UNIT types stay PENDING (no rows).
    let s: GetSettlementsResponse = parse("settlements__page.json");
    assert!(s.settlements.is_empty());
    assert_eq!(s.cursor.as_deref(), Some(""));
}

// ===========================================================================
// Error-body shapes (wire finding 1: THREE shapes the adapter must tolerate)
// ===========================================================================

#[test]
fn recorded_flat_error_body_is_structured_extracted() {
    // orders__numeric_field_types.json (checklist #13/#18): the FLAT OpenAPI
    // shape {"code","message","details"}. error_reason extracts all three.
    let reason = error_reason(&recorded("orders__numeric_field_types.json"));
    assert!(
        reason.starts_with("code=bad_request"),
        "flat shape is structured-extracted (code= prefix): {reason}"
    );
    assert!(reason.contains("bad request"));
    assert!(
        reason.contains("cannot unmarshal"),
        "the details string surfaces"
    );
}

#[test]
fn recorded_bare_msg_error_body_surfaces_the_message() {
    // markets__limit_over_max.json (checklist #18): the BARE {"msg":"..."} shape.
    // `msg` is not a KalshiErrorBody field, so error_reason surfaces it via the
    // raw-JSON fallback. (CONTAINS, not equality — stays true if a future fix
    // teaches the parser to extract `msg`.)
    let reason = error_reason(&recorded("markets__limit_over_max.json"));
    assert!(
        reason.contains("Parameter validation failed"),
        "the venue message must surface in diagnostics: {reason}"
    );
}

#[test]
fn recorded_nested_4xx_error_bodies_surface_their_venue_code() {
    // Wire finding 1a: the NESTED {"error":{"code","message","service"}} shape —
    // 17 of 19 recorded 4xx bodies. Pin the wire shape at the Value level (ground
    // truth) AND assert error_reason surfaces the code string. NOTE (ledgered
    // gap, GAPS.md): KalshiErrorBody.error is Option<String> but the wire sends
    // an OBJECT, so the struct-parse fails and error_reason surfaces the code via
    // the RAW-JSON fallback, not structured extraction. The HTTP-status routing
    // (400->Rejected, 404->NotFound) is independent and is covered in Cluster 2.
    for (file, code) in [
        (
            "orders__duplicate_client_order_id.json",
            "order_already_exists",
        ),
        ("orders__insufficient_balance.json", "insufficient_balance"),
        ("orders__post_only_cross.json", "invalid_order"),
        ("orders__invalid_price_structure.json", "invalid_price"),
        ("orders__cancel_already_canceled.json", "not_found"),
    ] {
        let body = recorded(file);
        // Ground truth: the wire nests the code under `error`.
        assert_eq!(
            body["error"]["code"].as_str(),
            Some(code),
            "{file}: recorded venue code"
        );
        // The adapter surfaces it in diagnostics (raw-JSON fallback today).
        let reason = error_reason(&body);
        assert!(
            reason.contains(code),
            "{file}: error_reason must surface {code}: {reason}"
        );
    }
}

#[test]
fn recorded_duplicate_client_order_id_code_is_order_already_exists() {
    // Wire finding 3 (checklist #7), pinned exactly: the 409 dup code string.
    let body = recorded("orders__duplicate_client_order_id.json");
    assert_eq!(body["error"]["code"].as_str(), Some("order_already_exists"));
    assert_eq!(
        body["error"]["message"].as_str(),
        Some("order already exists")
    );
}

#[test]
fn recorded_post_only_cross_is_rejected_at_create_not_201_then_cancel() {
    // Wire finding 6 (checklist #10): demo REJECTS a crossing post_only at create
    // (400 invalid_order / details "post only cross"), diverging from the docs'
    // 201-then-PostOnlyCrossCancel description.
    let body = recorded("orders__post_only_cross.json");
    assert_eq!(body["error"]["code"].as_str(), Some("invalid_order"));
    assert_eq!(body["error"]["details"].as_str(), Some("post only cross"));
}

#[test]
fn recorded_cancel_terminal_states_all_return_not_found() {
    // Wire finding 5 (checklist #14): cancel of already-canceled / executed /
    // unknown all return 404 with the same nested not_found body (NOT 200-with-
    // zero reduced_by).
    for file in [
        "orders__cancel_already_canceled.json",
        "orders__cancel_executed.json",
        "orders__cancel_unknown_id.json",
    ] {
        let body = recorded(file);
        assert_eq!(body["error"]["code"].as_str(), Some("not_found"), "{file}");
    }
}

// ===========================================================================
// Endpoint token costs (checklist #16)
// ===========================================================================

#[test]
fn recorded_endpoint_costs_confirm_v2_vs_legacy_token_costs() {
    // account__endpoint_costs.json (checklist #16). "Legacy" here is research.md's
    // term (#12/#16) for the DEPRECATED `/portfolio/orders` order family (10x token
    // cost, deprecation announced) — distinct from the CURRENT `/portfolio/events/
    // orders` family; both share the `/trade-api/v2` URL prefix. The capture
    // confirms the changelog's cost 20 (page said 2) for the legacy family, and the
    // cheaper current event-orders DELETE (2).
    let body = recorded("account__endpoint_costs.json");
    assert_eq!(body["default_cost"].as_i64(), Some(10));
    let cost = |method: &str, path: &str| -> Option<i64> {
        body["endpoint_costs"].as_array().and_then(|arr| {
            arr.iter()
                .find(|e| e["method"].as_str() == Some(method) && e["path"].as_str() == Some(path))
                .and_then(|e| e["cost"].as_i64())
        })
    };
    assert_eq!(
        cost("DELETE", "/trade-api/v2/portfolio/events/orders/:order_id"),
        Some(2),
        "current event-orders DELETE"
    );
    assert_eq!(
        cost("POST", "/trade-api/v2/portfolio/orders"),
        Some(20),
        "legacy /portfolio/orders POST"
    );
    assert_eq!(
        cost("DELETE", "/trade-api/v2/portfolio/orders/:order_id"),
        Some(4),
        "legacy /portfolio/orders DELETE"
    );
}

// ===========================================================================
// Exchange status (checklist #27, normal-operation shape)
// ===========================================================================

#[test]
fn recorded_exchange_status_normal_operation_shape() {
    // exchange__status.json: {"exchange_active":true,"trading_active":true}.
    // NOTE (ledgered gap, GAPS.md): there is no KalshiExchangeStatus DTO and no
    // KalshiVenue::exchange_status() method yet — halt detection (I2/I3) cannot
    // consume this until that lands. A maintenance-window shape is UNCOVERABLE
    // from this quiet-session capture (README known gap).
    #[derive(Deserialize)]
    struct ExchangeStatus {
        exchange_active: bool,
        trading_active: bool,
    }
    let s: ExchangeStatus = parse("exchange__status.json");
    assert!(s.exchange_active);
    assert!(s.trading_active);
}

// ===========================================================================
// Series fee changes (checklist #22, partial)
// ===========================================================================

#[test]
fn recorded_series_fee_changes_is_an_empty_array_at_capture() {
    // series__fee_changes.json: {"series_fee_change_arr":[]}. Confirms the
    // endpoint shape; no fee changes were scheduled. The populated series fee
    // fields (fee_type/fee_multiplier) stay PENDING (series__base uncaptured —
    // README known gap); the fee MATH is confirmed by the taker fill above.
    let body = recorded("series__fee_changes.json");
    assert!(
        body["series_fee_change_arr"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false),
        "empty fee-change array at capture time"
    );
}
