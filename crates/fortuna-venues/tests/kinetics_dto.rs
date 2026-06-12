//! T5.B4 slice-1 tests: kinetics DTOs vs the OPERATOR-RECORDED fixtures
//! (fixtures/kinetics-perps/). FIXTURES-GATED with FULL coverage: every
//! *.json body in the directory is explicitly classified and must parse
//! into its typed DTO (or the error envelope, per the recorded HTTP
//! status); an unclassified file FAILS the suite, so new captures must be
//! classified before they count as covered. WS .jsonl streams parse
//! line-by-line with zero unknown frames.

use fortuna_core::market::Contracts;
use fortuna_core::perp::PerpPrice;
use fortuna_venues::kinetics::dto::{self, KineticsApiError, WsFrame};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/kinetics-perps")
}

fn body(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join(format!("{name}.json")))
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn meta_status(name: &str) -> Option<i64> {
    let raw = fs::read_to_string(fixtures_dir().join(format!("{name}.meta.json"))).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("status").and_then(|s| s.as_i64())
}

/// What each recorded body must parse as.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Market,
    Markets,
    Orderbook,
    Order,
    Orders,
    CreateOrder,
    OrderId,
    AmendDecrease,
    Decrease,
    Cancel,
    Fills,
    Positions,
    Balance,
    RiskAccount,
    RiskParams,
    NotionalLimit,
    AccountLimits,
    FundingEstimate,
    FundingHistorical,
    FundingHistory,
    FeeTiers,
    ExchangeStatus,
    Enabled,
    GroupCreate,
    GroupGet,
    GroupsList,
    SubaccountCreate,
    Transfer,
    Empty,
    Err,
}

fn classification() -> BTreeMap<&'static str, Kind> {
    use Kind::*;
    BTreeMap::from([
        ("account__limits_perps", AccountLimits),
        ("auth__bad_signature", Err),
        ("auth__margin_balance", Balance),
        ("auth__margin_enabled_ok", Enabled),
        ("auth__skew_minus30s", Err),
        ("auth__skew_minus5min", Err),
        ("auth__skew_minus5s", Balance),
        ("auth__skew_plus30s", Balance),
        ("auth__skew_plus5min", Err),
        ("auth__skew_plus5s", Balance),
        ("balance__compute_available", Balance),
        ("cleanup__leftover_0", Err),
        ("exchange__status", ExchangeStatus),
        ("fees__tiers", FeeTiers),
        ("fills__after_open", Fills),
        ("funding__history_baseline", FundingHistory),
        ("funding__history_no_params", Err),
        ("funding__rates_estimate", FundingEstimate),
        ("funding__rates_historical", FundingHistorical),
        ("groups__create", GroupCreate),
        ("groups__delete", Empty),
        ("groups__get", GroupGet),
        ("groups__get_after_reset", GroupGet),
        ("groups__list", GroupsList),
        ("groups__reset", Empty),
        ("groups__trigger", Empty),
        ("groups__update_limit", Empty),
        ("markets__list", Markets),
        ("markets__single", Market),
        ("orderbook__agg_010", Orderbook),
        ("orderbook__depth0", Orderbook),
        ("orderbook__depth5", Orderbook),
        ("orders__amend_decrease", AmendDecrease),
        ("orders__amend_price", OrderId),
        ("orders__cancel", Cancel),
        ("orders__cancel_after_amend", Cancel),
        ("orders__cancel_after_decrease", Cancel),
        ("orders__create_for_decrease", CreateOrder),
        ("orders__create_gtc", CreateOrder),
        ("orders__create_gtc_blocked", Err),
        ("orders__create_in_group", CreateOrder),
        ("orders__create_post_only", CreateOrder),
        ("orders__decrease_reduce_by", Decrease),
        ("orders__duplicate_client_order_id", Err),
        ("orders__filter_canceled", Orders),
        ("orders__filter_executed", Orders),
        ("orders__filter_garbage", Orders),
        ("orders__filter_open", Orders),
        ("orders__filter_resting", Orders),
        ("orders__final_resting", Orders),
        ("orders__funding_position_ioc", CreateOrder),
        ("orders__get_after_amend", Order),
        ("orders__get_after_cancel", Order),
        ("orders__get_after_create", Order),
        ("orders__get_after_decrease", Order),
        ("orders__get_after_group_trigger", Order),
        ("orders__insufficient_margin", Err),
        ("orders__list_all", Orders),
        ("orders__off_tick_price", Err),
        ("orders__price_band_violation", Err),
        ("orders__reduce_only_gtc", Err),
        ("orders__reuse_canceled_client_id", Err),
        ("positions__blocked", Positions),
        ("positions__final", Positions),
        ("positions__open", Positions),
        ("risk__account", RiskAccount),
        ("risk__notional_limit", NotionalLimit),
        ("risk__parameters", RiskParams),
        ("subaccounts__create", SubaccountCreate),
        ("subaccounts__create_nobody", Err),
        ("subaccounts__transfer_back", Empty),
        ("subaccounts__transfer_duplicate", Err),
        ("subaccounts__transfer_first", Empty),
        ("transfer__intra_exchange", Transfer),
    ])
}

fn parse_as(kind: Kind, raw: &str) -> Result<(), String> {
    fn p<T: serde::de::DeserializeOwned>(raw: &str) -> Result<(), String> {
        serde_json::from_str::<T>(raw)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    use fortuna_venues::kinetics::dto as d;
    match kind {
        Kind::Market => p::<d::MarketResponse>(raw),
        Kind::Markets => p::<d::MarketsResponse>(raw),
        Kind::Orderbook => p::<d::OrderbookResponse>(raw),
        Kind::Order => p::<d::OrderResponse>(raw),
        Kind::Orders => p::<d::OrdersResponse>(raw),
        Kind::CreateOrder => p::<d::CreateOrderResponse>(raw),
        Kind::OrderId => p::<d::OrderIdResponse>(raw),
        Kind::AmendDecrease => p::<d::AmendDecreaseResponse>(raw),
        Kind::Decrease => p::<d::DecreaseOrderResponse>(raw),
        Kind::Cancel => p::<d::CancelOrderResponse>(raw),
        Kind::Fills => p::<d::FillsResponse>(raw),
        Kind::Positions => p::<d::PositionsResponse>(raw),
        Kind::Balance => p::<d::BalanceResponse>(raw),
        Kind::RiskAccount => p::<d::RiskAccountResponse>(raw),
        Kind::RiskParams => p::<d::RiskParametersResponse>(raw),
        Kind::NotionalLimit => p::<d::NotionalRiskLimitResponse>(raw),
        Kind::AccountLimits => p::<d::AccountLimitsResponse>(raw),
        Kind::FundingEstimate => p::<d::FundingEstimateResponse>(raw),
        Kind::FundingHistorical => p::<d::FundingRatesHistoricalResponse>(raw),
        Kind::FundingHistory => p::<d::FundingHistoryResponse>(raw),
        Kind::FeeTiers => p::<d::FeeTiersResponse>(raw),
        Kind::ExchangeStatus => p::<d::ExchangeStatusResponse>(raw),
        Kind::Enabled => p::<d::MarginEnabledResponse>(raw),
        Kind::GroupCreate => p::<d::GroupCreateResponse>(raw),
        Kind::GroupGet => p::<d::GroupGetResponse>(raw),
        Kind::GroupsList => p::<d::GroupsListResponse>(raw),
        Kind::SubaccountCreate => p::<d::SubaccountCreateResponse>(raw),
        Kind::Transfer => p::<d::TransferResponse>(raw),
        Kind::Empty => p::<d::EmptyResponse>(raw),
        Kind::Err => p::<KineticsApiError>(raw),
    }
}

/// Every fixture body parses as its classified DTO; every file is
/// classified; the classification agrees with the recorded HTTP status.
#[test]
fn every_fixture_parses_into_its_typed_dto() {
    let table = classification();
    let mut seen = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for entry in fs::read_dir(fixtures_dir()).expect("fixtures dir") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with(".json") || name.ends_with(".meta.json") || name.ends_with(".jsonl") {
            continue;
        }
        let stem = name.trim_end_matches(".json");
        seen += 1;
        let Some(kind) = table.get(stem) else {
            failures.push(format!("{stem}: UNCLASSIFIED — classify new fixtures"));
            continue;
        };
        // The table must agree with the recorded status (errors <=> 4xx/5xx).
        if let Some(status) = meta_status(stem) {
            let is_err = *kind == Kind::Err;
            if is_err != (status >= 400) {
                failures.push(format!(
                    "{stem}: classified {kind:?} but recorded status is {status}"
                ));
            }
        }
        if let Err(e) = parse_as(*kind, &body(stem)) {
            failures.push(format!("{stem} as {kind:?}: {e}"));
        }
    }
    assert!(
        failures.is_empty(),
        "fixture parse failures:\n{}",
        failures.join("\n")
    );
    assert_eq!(
        seen,
        table.len(),
        "fixture count {seen} != classification table {} (stale table entry?)",
        table.len()
    );
}

// ---- semantic spot checks (values, not just shapes) ----

#[test]
fn market_single_parses_load_bearing_values() {
    let m: dto::MarketResponse = serde_json::from_str(&body("markets__single")).unwrap();
    let m = m.market;
    assert_eq!(m.ticker, "KXBTCPERP1");
    assert_eq!(
        dto::parse_perp_price(m.ask.as_deref().unwrap()).unwrap(),
        PerpPrice::new(63_416)
    );
    assert_eq!(
        dto::parse_perp_price(m.bid.as_deref().unwrap()).unwrap(),
        PerpPrice::new(63_329)
    );
    // Demo BTC tick_size is the EMPTY STRING (finding 12) — tolerated.
    assert_eq!(dto::parse_perp_price_opt(&m.tick_size).unwrap(), None);
    // The recorded risk curve the gate/sim mm_curve derives from.
    assert_eq!(m.leverage_estimates.get("1000000").copied(), Some(5.8143));
    assert_eq!(m.leverage_estimate, Some(5.899));
    assert!(!m.fractional_trading_enabled);
    assert_eq!(
        dto::parse_perp_price(&m.settlement_mark_price.unwrap().price).unwrap(),
        PerpPrice::new(63_805)
    );
}

#[test]
fn order_get_parses_sides_counts_and_source() {
    let o: dto::OrderResponse = serde_json::from_str(&body("orders__get_after_create")).unwrap();
    let o = o.order;
    assert_eq!(o.side, "bid");
    assert_eq!(o.order_source.as_deref(), Some("user"));
    assert_eq!(
        dto::parse_perp_price(&o.price).unwrap(),
        PerpPrice::new(53_829)
    );
    assert_eq!(
        dto::parse_whole_count(&o.remaining_count).unwrap(),
        Contracts::new(1)
    );
    assert_eq!(
        dto::parse_whole_count(&o.fill_count).unwrap(),
        Contracts::new(0)
    );
}

#[test]
fn position_parses_signed_count_and_dollar_strings() {
    let p: dto::PositionsResponse = serde_json::from_str(&body("positions__open")).unwrap();
    let p = &p.positions[0];
    assert_eq!(
        dto::parse_whole_count(&p.position).unwrap(),
        Contracts::new(1)
    );
    assert_eq!(
        dto::parse_perp_price(&p.entry_price).unwrap(),
        PerpPrice::new(63_587)
    );
    // unrealized_pnl is a signed dollar string with 4dp.
    assert!(dto::parse_dollars(&p.unrealized_pnl).unwrap() < rust_decimal::Decimal::ZERO);
}

#[test]
fn funding_rates_are_floats_never_strings() {
    let r: dto::FundingRatesHistoricalResponse =
        serde_json::from_str(&body("funding__rates_historical")).unwrap();
    assert_eq!(r.funding_rates[0].funding_rate, -0.0009593552948286);
    let e: dto::FundingEstimateResponse =
        serde_json::from_str(&body("funding__rates_estimate")).unwrap();
    assert_eq!(e.funding_rate, 0.0);
    assert_eq!(e.next_funding_time, "2026-06-12T12:00:00Z");
}

#[test]
fn risk_parameters_carry_per_market_multipliers() {
    let r: dto::RiskParametersResponse = serde_json::from_str(&body("risk__parameters")).unwrap();
    assert_eq!(
        r.initial_margin_multiplier.get("KXBTCPERP1").copied(),
        Some(1.3)
    );
    assert_eq!(
        r.initial_margin_multiplier.get("KXHYPEPERP1").copied(),
        Some(1.1)
    );
    assert_eq!(r.liquidation_margin_ratio_threshold, 1.0);
    assert_eq!(r.queue_entry_margin_ratio_threshold, 0.8);
}

#[test]
fn error_codes_prefix_match_dynamic_families() {
    // finding 8: dynamic code with embedded uuid.
    let e: KineticsApiError = serde_json::from_str(&body("orders__create_gtc_blocked")).unwrap();
    assert!(e.code_matches("user_not_found"));
    assert!(!e.code_matches("user_not"));
    // Static nested code.
    let e: KineticsApiError = serde_json::from_str(&body("auth__bad_signature")).unwrap();
    assert!(e.code_matches("authentication_error"));
    // Flat shape.
    let e: KineticsApiError = serde_json::from_str(&body("subaccounts__create_nobody")).unwrap();
    assert!(e.code_matches("invalid_content_type"));
    // Bare-msg shape.
    let e: KineticsApiError = serde_json::from_str(&body("funding__history_no_params")).unwrap();
    assert!(matches!(e, KineticsApiError::Bare { .. }));
}

#[test]
fn orderbook_best_levels_sort_defensively() {
    // finding 1: recorded ordering is worst->best, best at array END —
    // best_bid/best_ask must not assume any ordering.
    let b: dto::OrderbookResponse = serde_json::from_str(&body("orderbook__depth5")).unwrap();
    let (best_bid, _) = b.orderbook.best_bid().unwrap().unwrap();
    let (best_ask, _) = b.orderbook.best_ask().unwrap().unwrap();
    assert_eq!(best_bid, PerpPrice::new(63_329));
    assert_eq!(best_ask, PerpPrice::new(63_416));
    assert!(best_bid < best_ask);
}

#[test]
fn parse_primitives_round_against_garbage() {
    assert_eq!(
        dto::parse_perp_price("6.3416").unwrap(),
        PerpPrice::new(63_416)
    );
    assert!(dto::parse_perp_price("6.34165").is_err()); // sub-tick
    assert!(dto::parse_perp_price("").is_err());
    assert_eq!(dto::parse_whole_count("-1.00").unwrap(), Contracts::new(-1));
    assert!(dto::parse_whole_count("0.50").is_err()); // fractional disabled
}

// ---- WS streams: every recorded frame parses, zero unknown ----

fn parse_stream(file: &str) -> BTreeMap<String, usize> {
    let raw = fs::read_to_string(fixtures_dir().join(file)).expect("ws fixture");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let frame = dto::parse_ws_frame(line)
            .unwrap_or_else(|e| panic!("{file}: frame failed: {e}\n{line}"));
        let key = match &frame {
            WsFrame::Subscribed { .. } => "subscribed",
            WsFrame::OrderbookSnapshot { .. } => "orderbook_snapshot",
            WsFrame::OrderbookDelta { .. } => "orderbook_delta",
            WsFrame::Ticker { .. } => "ticker",
            WsFrame::Trade { .. } => "trade",
            WsFrame::UserOrder { .. } => "user_order",
            WsFrame::OrderGroupUpdate { .. } => "order_group_updates",
            WsFrame::Unknown(v) => panic!("{file}: unknown frame type in a RECORDED stream: {v}"),
        };
        *counts.entry(key.to_string()).or_insert(0) += 1;
    }
    counts
}

#[test]
fn public_ws_stream_parses_completely() {
    let counts = parse_stream("ws__public_orderbook_ticker.jsonl");
    assert_eq!(counts.get("orderbook_snapshot").copied(), Some(1));
    assert!(counts.get("orderbook_delta").copied().unwrap_or(0) > 1000);
    assert!(counts.get("ticker").copied().unwrap_or(0) > 10);
    assert!(counts.get("trade").copied().unwrap_or(0) > 1000);
}

#[test]
fn private_ws_stream_parses_completely() {
    let counts = parse_stream("ws__private_lifecycle.jsonl");
    assert!(counts.get("user_order").copied().unwrap_or(0) > 0);
    assert!(counts.get("order_group_updates").copied().unwrap_or(0) > 0);
    assert_eq!(counts.get("subscribed").copied(), Some(3));
}

#[test]
fn ws_ticker_carries_funding_rate_and_all_three_marks() {
    let raw = fs::read_to_string(fixtures_dir().join("ws__public_orderbook_ticker.jsonl"))
        .expect("ws fixture");
    let ticker = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| match dto::parse_ws_frame(l) {
            Ok(WsFrame::Ticker { msg, .. }) => Some(msg),
            _ => None,
        })
        .next()
        .expect("at least one ticker frame");
    // finding 10/11: rate is a number; next funding time on the 8h grid.
    assert_eq!(
        ticker.funding_rate.next_funding_time_ms % 28_800_000,
        14_400_000
    );
    dto::parse_perp_price(&ticker.settlement_mark_price.price).unwrap();
    dto::parse_perp_price(&ticker.liquidation_mark_price.price).unwrap();
    dto::parse_perp_price(&ticker.reference_price.price).unwrap();
}

#[test]
fn unknown_ws_frame_type_degrades_to_unknown_not_error() {
    let frame = dto::parse_ws_frame(r#"{"type":"brand_new_thing","msg":{}}"#).unwrap();
    assert!(matches!(frame, WsFrame::Unknown(_)));
}
