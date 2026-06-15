//! T5.B8 kill-switch perp FLATTEN, behavioral (spec 5.15). MockKalshiTransport
//! scripts the venue (no network). The transport is FIFO; the flatten makes its
//! calls in a fixed order — list_orders → (cancels) → balance → positions →
//! per-position (orderbook → create_order) — so the response script mirrors that.
//!
//! Pins: a LONG closes with a SELL IOC reduce-only whose limit is strictly below
//! the bid touch; a SHORT closes with a BUY mirror above the ask; an empty book
//! side / orderbook error SKIPS (no create_order); a place error is counted and
//! the sweep continues; cancel-all sweeps every order (NotFound == cancelled,
//! one retry); a gate rejection (slippage > price band) is counted not fatal; and
//! load_gate_config is FAIL-CLOSED. Every placed close is a sealed GatedPerpOrder
//! (the gate is the only constructor — I1).

use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::market::VenueId;
use fortuna_gates::{GateConfig, GatePipeline};
use fortuna_killswitch::{freeze_cancel_perp_and_flatten, load_gate_config};
use fortuna_venues::kalshi::client::MockKalshiTransport;
use fortuna_venues::kinetics::adapter::KineticsAdapter;
use fortuna_venues::kinetics::client::KineticsClient;
use fortuna_venues::kinetics::dto::parse_perp_price;
use fortuna_venues::VenueError;
use serde_json::json;
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T08:00:00.000Z").unwrap()
}

const GATE_CONFIG_TOML: &str = r#"
[global]
max_total_exposure_cents = 100000000
max_daily_loss_cents = 50000
min_order_contracts = 1
max_order_contracts = 1000
price_band_cents = 49
max_cross_cents = 98
per_market_exposure_cents = 100000000
per_event_exposure_cents = 100000000
require_event_mapping = false

[per_strategy.killswitch_flatten]
max_exposure_cents = 100000000
max_order_notional_cents = 1000000
min_net_edge_bps = 0

[rate.kinetics]
burst = 100000
sustained_per_min = 100000
market_burst = 100000
market_sustained_per_min = 100000

[perp.venues.kinetics]
max_total_notional_cents = 10000000
min_order_contracts = 1
max_order_contracts = 10000
price_band_bps = 2000
assumed_fee_bps = 12
funding_drag_bps_per_window = 4
min_liquidation_distance_bps = 100
mm_safety_multiplier_pct = 130

[perp.assets.KXBTCPERP]
max_leverage_x10 = 50
max_notional_cents = 5000000
mm_curve = [[1000000, 500], [5000000, 800]]
"#;

fn pipeline() -> GatePipeline {
    GatePipeline::new(toml::from_str::<GateConfig>(GATE_CONFIG_TOML).unwrap()).unwrap()
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

// ---- JSON fixtures (built inline so each scenario is controlled) ----

fn orders_resp(orders: Vec<serde_json::Value>) -> serde_json::Value {
    json!({ "cursor": "", "orders": orders })
}
fn order_json(id: &str) -> serde_json::Value {
    json!({
        "order_id": id, "client_order_id": format!("c-{id}"), "user_id": "u",
        "ticker": "KXBTCPERP", "side": "bid", "price": "6.0000",
        "fill_count": "0.00", "remaining_count": "5.00", "order_source": "user",
        "self_trade_prevention_type": "taker_at_cross",
        "created_time": "2026-06-12T08:00:00Z", "last_update_time": "2026-06-12T08:00:00Z",
        "last_update_reason": "none"
    })
}
fn cancel_resp() -> serde_json::Value {
    json!({ "order_id": "o", "reduced_by": "5.00" })
}
fn balance_resp() -> serde_json::Value {
    json!({ "settled_funds": "1000.0000", "subaccount_balances": [] })
}
/// `signed_qty`: positive = long, negative = short.
fn position_json(signed_qty: &str) -> serde_json::Value {
    json!({
        "market_ticker": "KXBTCPERP", "position": signed_qty, "entry_price": "6.0000",
        "fees": "0.0000", "margin_used": "1.0000", "unrealized_pnl": "0.0000",
        "roe": 0.0, "subaccount": 0
    })
}
fn positions_resp(positions: Vec<serde_json::Value>) -> serde_json::Value {
    json!({ "positions": positions })
}
fn orderbook_resp(bids: Vec<[&str; 2]>, asks: Vec<[&str; 2]>) -> serde_json::Value {
    json!({ "orderbook": { "bids": bids, "asks": asks } })
}
fn create_resp(order_id: &str) -> serde_json::Value {
    json!({
        "order_id": order_id, "client_order_id": "c", "fill_count": "5.00", "remaining_count": "0.00"
    })
}

enum Scripted {
    Ok(serde_json::Value),
    OkStatus(u16, serde_json::Value),
    Err(VenueError),
}

fn adapter_with(script: Vec<Scripted>) -> (KineticsAdapter, Arc<MockKalshiTransport>) {
    let transport = Arc::new(MockKalshiTransport::new());
    for s in script {
        match s {
            Scripted::Ok(body) => transport.push_ok(200, body),
            Scripted::OkStatus(status, body) => transport.push_ok(status, body),
            Scripted::Err(e) => transport.push_err(e),
        }
    }
    (
        KineticsAdapter::new(KineticsClient::new(transport.clone())),
        transport,
    )
}

fn journal_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("ks-flatten-{tag}-{}.jsonl", std::process::id()))
}

/// Find the single create_order call (POST /margin/orders) wire body.
fn create_body(transport: &MockKalshiTransport) -> serde_json::Value {
    transport
        .calls()
        .into_iter()
        .find(|c| c.method == "POST" && c.path == "/margin/orders")
        .and_then(|c| c.body)
        .expect("a create_order POST was issued")
}

// ---- long → SELL IOC reduce-only below the bid touch ----

#[test]
fn long_position_closes_with_a_sell_ioc_reduce_only_below_the_bid() {
    let (adapter, transport) = adapter_with(vec![
        Scripted::Ok(orders_resp(vec![])), // list_orders: nothing to cancel
        Scripted::Ok(balance_resp()),      // balance
        Scripted::Ok(positions_resp(vec![position_json("5.00")])), // one LONG +5
        Scripted::Ok(orderbook_resp(
            vec![["6.2500", "10.00"]],
            vec![["6.2700", "10.00"]],
        )),
        Scripted::Ok(create_resp("vo-long")), // create_order succeeds
    ]);
    let mut gates = pipeline();
    let clock = SimClock::new(t0());
    let venue = VenueId::new("kinetics").unwrap();
    let journal = journal_path("long");
    let report = block_on(freeze_cancel_perp_and_flatten(
        &adapter, &mut gates, &venue, 50, &clock, &journal,
    ))
    .unwrap();

    assert_eq!(report.positions_seen, 1);
    assert_eq!(report.flatten_orders_placed, 1, "the long was closed");
    assert_eq!(report.flatten_orders_skipped_no_price, 0);
    assert_eq!(report.flatten_orders_rejected_by_gate, 0);

    let body = create_body(&transport);
    assert_eq!(body["side"], "ask", "closing a LONG is a SELL = ask leg");
    assert_eq!(body["reduce_only"], true);
    assert_eq!(body["time_in_force"], "immediate_or_cancel");
    // The limit crossed the bid AGAINST us: strictly below the 6.2500 touch.
    let limit = parse_perp_price(body["price"].as_str().unwrap()).unwrap();
    assert!(
        limit.raw() < parse_perp_price("6.2500").unwrap().raw(),
        "sell limit {} must be below the bid touch 62500",
        limit.raw()
    );
    let _ = std::fs::remove_file(&journal);
}

// ---- short → BUY IOC reduce-only above the ask touch ----

#[test]
fn short_position_closes_with_a_buy_ioc_reduce_only_above_the_ask() {
    let (adapter, transport) = adapter_with(vec![
        Scripted::Ok(orders_resp(vec![])),
        Scripted::Ok(balance_resp()),
        Scripted::Ok(positions_resp(vec![position_json("-5.00")])), // one SHORT -5
        Scripted::Ok(orderbook_resp(
            vec![["6.2500", "10.00"]],
            vec![["6.2700", "10.00"]],
        )),
        Scripted::Ok(create_resp("vo-short")),
    ]);
    let mut gates = pipeline();
    let clock = SimClock::new(t0());
    let venue = VenueId::new("kinetics").unwrap();
    let journal = journal_path("short");
    let report = block_on(freeze_cancel_perp_and_flatten(
        &adapter, &mut gates, &venue, 50, &clock, &journal,
    ))
    .unwrap();

    assert_eq!(report.flatten_orders_placed, 1);
    let body = create_body(&transport);
    assert_eq!(body["side"], "bid", "closing a SHORT is a BUY = bid leg");
    assert_eq!(body["reduce_only"], true);
    assert_eq!(body["time_in_force"], "immediate_or_cancel");
    let limit = parse_perp_price(body["price"].as_str().unwrap()).unwrap();
    assert!(
        limit.raw() > parse_perp_price("6.2700").unwrap().raw(),
        "buy limit {} must be above the ask touch 62700",
        limit.raw()
    );
    let _ = std::fs::remove_file(&journal);
}

// ---- empty book side → skip, no create_order ----

#[test]
fn empty_book_side_skips_and_places_nothing() {
    let (adapter, transport) = adapter_with(vec![
        Scripted::Ok(orders_resp(vec![])),
        Scripted::Ok(balance_resp()),
        Scripted::Ok(positions_resp(vec![position_json("5.00")])),
        Scripted::Ok(orderbook_resp(vec![], vec![["6.2700", "10.00"]])), // NO bids
    ]);
    let mut gates = pipeline();
    let clock = SimClock::new(t0());
    let venue = VenueId::new("kinetics").unwrap();
    let journal = journal_path("emptybook");
    let report = block_on(freeze_cancel_perp_and_flatten(
        &adapter, &mut gates, &venue, 50, &clock, &journal,
    ))
    .unwrap();
    assert_eq!(report.flatten_orders_skipped_no_price, 1);
    assert_eq!(report.flatten_orders_placed, 0);
    assert!(
        !transport
            .calls()
            .iter()
            .any(|c| c.method == "POST" && c.path == "/margin/orders"),
        "no create_order on an un-priceable position"
    );
    let _ = std::fs::remove_file(&journal);
}

// ---- orderbook error → skip + continue ----

#[test]
fn orderbook_error_skips_and_continues() {
    let (adapter, _t) = adapter_with(vec![
        Scripted::Ok(orders_resp(vec![])),
        Scripted::Ok(balance_resp()),
        Scripted::Ok(positions_resp(vec![position_json("5.00")])),
        Scripted::Err(VenueError::Outage {
            venue: "kinetics".into(),
            reason: "book unavailable".into(),
        }),
    ]);
    let mut gates = pipeline();
    let clock = SimClock::new(t0());
    let venue = VenueId::new("kinetics").unwrap();
    let journal = journal_path("bookerr");
    let report = block_on(freeze_cancel_perp_and_flatten(
        &adapter, &mut gates, &venue, 50, &clock, &journal,
    ))
    .unwrap(); // best-effort: the function still returns Ok
    assert_eq!(report.flatten_orders_skipped_no_price, 1);
    assert_eq!(report.flatten_orders_placed, 0);
    let _ = std::fs::remove_file(&journal);
}

// ---- place error → counted, sweep continues (Ok) ----

#[test]
fn place_error_is_counted_and_the_sweep_continues() {
    let (adapter, _t) = adapter_with(vec![
        Scripted::Ok(orders_resp(vec![])),
        Scripted::Ok(balance_resp()),
        Scripted::Ok(positions_resp(vec![position_json("5.00")])),
        Scripted::Ok(orderbook_resp(
            vec![["6.2500", "10.00"]],
            vec![["6.2700", "10.00"]],
        )),
        Scripted::Err(VenueError::Rejected {
            reason: "insufficient liquidity".into(),
        }),
    ]);
    let mut gates = pipeline();
    let clock = SimClock::new(t0());
    let venue = VenueId::new("kinetics").unwrap();
    let journal = journal_path("placeerr");
    let report = block_on(freeze_cancel_perp_and_flatten(
        &adapter, &mut gates, &venue, 50, &clock, &journal,
    ))
    .unwrap();
    assert_eq!(report.flatten_orders_failed, 1);
    assert_eq!(report.flatten_orders_placed, 0);
    let _ = std::fs::remove_file(&journal);
}

// ---- cancel-all sweeps every order (NotFound==cancelled, one retry) ----

#[test]
fn cancel_all_sweeps_three_orders_including_not_found_and_a_retry() {
    let (adapter, _t) = adapter_with(vec![
        Scripted::Ok(orders_resp(vec![
            order_json("o1"),
            order_json("o2"),
            order_json("o3"),
        ])),
        Scripted::Ok(cancel_resp()), // o1: cancelled
        // o2: NotFound == cancelled. The client maps by the body error CODE
        // ("not_found"), never the HTTP status (recorded cleanup__leftover_0 shape).
        Scripted::OkStatus(
            404,
            json!({ "error": { "code": "not_found", "message": "not found", "service": "exchange" } }),
        ),
        Scripted::Err(VenueError::Invalid {
            reason: "transient".into(),
        }), // o3: first attempt fails
        Scripted::Ok(cancel_resp()), // o3: retry succeeds
        Scripted::Ok(balance_resp()),
        Scripted::Ok(positions_resp(vec![])), // flat: no flatten orders
    ]);
    let mut gates = pipeline();
    let clock = SimClock::new(t0());
    let venue = VenueId::new("kinetics").unwrap();
    let journal = journal_path("cancelsweep");
    let report = block_on(freeze_cancel_perp_and_flatten(
        &adapter, &mut gates, &venue, 50, &clock, &journal,
    ))
    .unwrap();
    assert_eq!(report.orders_seen, 3);
    assert_eq!(
        report.orders_cancelled, 3,
        "all 3 swept (NotFound + retry count)"
    );
    assert_eq!(report.orders_cancel_failed, 0);
    assert_eq!(report.positions_seen, 0);
    let _ = std::fs::remove_file(&journal);
}

// ---- gate rejection (slippage > price band) is counted, not fatal ----

#[test]
fn a_gate_rejection_is_counted_not_fatal() {
    // slippage 5000 bps crosses the 6.2500 bid by 50% → |limit − mark| far
    // exceeds the 2000 bps price band → PriceSanity self-rejects (the documented
    // slippage_bps <= price_band_bps requirement). Counted, never a wrong fill.
    let (adapter, transport) = adapter_with(vec![
        Scripted::Ok(orders_resp(vec![])),
        Scripted::Ok(balance_resp()),
        Scripted::Ok(positions_resp(vec![position_json("5.00")])),
        Scripted::Ok(orderbook_resp(
            vec![["6.2500", "10.00"]],
            vec![["6.2700", "10.00"]],
        )),
    ]);
    let mut gates = pipeline();
    let clock = SimClock::new(t0());
    let venue = VenueId::new("kinetics").unwrap();
    let journal = journal_path("gatereject");
    let report = block_on(freeze_cancel_perp_and_flatten(
        &adapter, &mut gates, &venue, 5000, &clock, &journal,
    ))
    .unwrap();
    assert_eq!(report.flatten_orders_rejected_by_gate, 1);
    assert_eq!(report.flatten_orders_placed, 0);
    assert!(
        !transport
            .calls()
            .iter()
            .any(|c| c.method == "POST" && c.path == "/margin/orders"),
        "a gate-rejected close never reaches the wire"
    );
    let _ = std::fs::remove_file(&journal);
}

// ---- load_gate_config is FAIL-CLOSED ----

#[test]
fn load_gate_config_is_fail_closed() {
    // (none) → refuse.
    assert!(load_gate_config(None).is_err());
    assert!(load_gate_config(Some("   ".to_string())).is_err());
    // (missing file) → refuse.
    assert!(load_gate_config(Some("/no/such/gate/config/path.toml".to_string())).is_err());
    // (unparseable) → refuse.
    let bad = journal_path("badcfg");
    std::fs::write(&bad, b"this is not = valid toml [[[").unwrap();
    assert!(load_gate_config(Some(bad.display().to_string())).is_err());
    let _ = std::fs::remove_file(&bad);
    // (parses but validate() fails: a zero max_order_contracts is invalid) → refuse.
    let invalid = journal_path("invalidcfg");
    std::fs::write(
        &invalid,
        GATE_CONFIG_TOML.replace("max_order_contracts = 1000", "max_order_contracts = 0"),
    )
    .unwrap();
    assert!(
        load_gate_config(Some(invalid.display().to_string())).is_err(),
        "a config that fails validate() must be refused"
    );
    let _ = std::fs::remove_file(&invalid);
    // (ok) → loads.
    let good = journal_path("goodcfg");
    std::fs::write(&good, GATE_CONFIG_TOML).unwrap();
    assert!(load_gate_config(Some(good.display().to_string())).is_ok());
    let _ = std::fs::remove_file(&good);
}
