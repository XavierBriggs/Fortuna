//! T5.B4 slice-3 tests: the kinetics adapter. I1 is structural in this
//! suite: every placed order is a REAL `GatedPerpOrder` produced by the
//! perp gate arm (there is no other way to construct one). Wire shapes
//! replay the operator recordings; venue rules (reduce_only => IOC/FOK)
//! are enforced before the wire; system fills classify as the distinct
//! liquidation class; fees reconcile against the posted tiers — and the
//! RECORDED demo fill mismatches them (promo-$0 charged vs posted
//! 0.0012 taker), which is the fee-trap reality the discrepancy must
//! surface.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{MarginAccountView, PerpPosition, PerpPrice};
use fortuna_gates::perp::{GatedPerpOrder, PerpCandidateOrder, PerpGateInputs};
use fortuna_gates::{GateConfig, GatePipeline};
use fortuna_venues::kalshi::client::MockKalshiTransport;
use fortuna_venues::kinetics::adapter::{KineticsAdapter, PerpFillClass};
use fortuna_venues::kinetics::client::{KineticsClient, TimeInForce};
use fortuna_venues::kinetics::dto;
use fortuna_venues::VenueError;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/kinetics-perps")
}

fn fixture_json(name: &str) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(fixtures_dir().join(format!("{name}.json"))).unwrap())
        .unwrap()
}

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T08:00:00.000Z").unwrap()
}

fn gate_config() -> GateConfig {
    toml::from_str(
        r#"
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

        [per_strategy.b4]
        max_exposure_cents = 100000000
        max_order_notional_cents = 100000000
        min_net_edge_bps = 0

        [rate.kinetics]
        burst = 1000
        sustained_per_min = 1000
        market_burst = 1000
        market_sustained_per_min = 1000

        [perp.venues.kinetics]
        max_total_notional_cents = 100000000
        min_order_contracts = 1
        max_order_contracts = 10000
        price_band_bps = 2000
        assumed_fee_bps = 12
        funding_drag_bps_per_window = 4
        min_liquidation_distance_bps = 100
        mm_safety_multiplier_pct = 130

        [perp.assets.KXBTCPERP1]
        max_leverage_x10 = 100
        max_notional_cents = 100000000
        mm_curve = [[100000000, 1696]]
        "#,
    )
    .unwrap()
}

/// Gate a real candidate into a sealed order — the ONLY way to obtain a
/// `GatedPerpOrder` (I1).
fn gate_order(
    n: u64,
    client_order_id: &str,
    price: i64,
    qty: i64,
    action: Action,
    reduce_only: bool,
    position: Option<PerpPosition>,
) -> GatedPerpOrder {
    let mut pipeline = GatePipeline::new(gate_config()).unwrap();
    let mut idgen = IdGen::new(n);
    let candidate = PerpCandidateOrder {
        intent_id: IntentId::new(idgen.next(t0()).unwrap()),
        strategy: StrategyId::new("b4").unwrap(),
        venue: VenueId::new("kinetics").unwrap(),
        market: MarketId::new("KXBTCPERP1").unwrap(),
        action,
        reduce_only,
        limit_price: PerpPrice::new(price),
        qty: Contracts::new(qty),
        fair_value: PerpPrice::new(match action {
            Action::Buy => price + 500,
            Action::Sell => price - 500,
        }),
        holding_windows: 1,
        client_order_id: ClientOrderId::new(client_order_id).unwrap(),
    };
    let account = MarginAccountView::compute(Cents::new(10_000_000), &[], Cents::ZERO).unwrap();
    let recent = BTreeSet::new();
    let inputs = PerpGateInputs {
        now: t0(),
        account: &account,
        position: position.as_ref(),
        conservative_mark: PerpPrice::new(price),
        venue_open_notional_cents: Cents::ZERO,
        own_resting: &[],
        recent_client_order_ids: &recent,
    };
    pipeline
        .evaluate_perp(&candidate, &inputs)
        .gated
        .expect("test candidate must gate cleanly")
}

fn adapter_with(
    responses: Vec<(u16, serde_json::Value)>,
) -> (KineticsAdapter, Arc<MockKalshiTransport>) {
    let transport = Arc::new(MockKalshiTransport::new());
    for (status, body) in responses {
        transport.push_ok(status, body);
    }
    (
        KineticsAdapter::new(KineticsClient::new(transport.clone())),
        transport,
    )
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

// ---- place: gated order -> recorded wire shape ----

#[test]
fn place_maps_gated_order_to_the_recorded_create_request() {
    // RE-RECORDING-PROOF (perps-merge revert lesson): the candidate is
    // built FROM the recorded request body (client id, price, count) and
    // the placement assertions compare against the recorded response's
    // own fields. Nothing capture-specific is hardcoded.
    let meta: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(fixtures_dir().join("orders__create_gtc.meta.json")).unwrap(),
    )
    .unwrap();
    let req = &meta["request_body"];
    let price =
        fortuna_venues::kinetics::dto::parse_perp_price(req["price"].as_str().unwrap()).unwrap();
    let response = fixture_json("orders__create_gtc");
    let order = gate_order(
        1,
        req["client_order_id"].as_str().unwrap(),
        price.raw(),
        req["count"].as_str().unwrap().parse().unwrap(),
        Action::Buy,
        false,
        None,
    );
    let (adapter, transport) = adapter_with(vec![(201, response.clone())]);
    let placement =
        block_on(adapter.place(&order, TimeInForce::GoodTillCanceled, Some(false))).unwrap();
    assert_eq!(
        placement.venue_order_id.as_str(),
        response["order_id"].as_str().unwrap()
    );

    // The wire body equals the recording.
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "POST");
    assert_eq!(calls[0].path, "/margin/orders");
    assert_eq!(calls[0].body.as_ref(), Some(req));
}

#[test]
fn reduce_only_gtc_is_refused_before_the_wire() {
    let position = PerpPosition {
        market: MarketId::new("KXBTCPERP1").unwrap(),
        qty: Contracts::new(-5),
        avg_entry: PerpPrice::new(63_816),
    };
    let order = gate_order(
        2,
        "61f14161-f3df-4a20-88f5-c41645f0b480",
        63_816,
        1,
        Action::Buy,
        true,
        Some(position),
    );
    let (adapter, transport) = adapter_with(vec![]);
    let err = block_on(adapter.place(&order, TimeInForce::GoodTillCanceled, None)).unwrap_err();
    assert!(matches!(err, VenueError::Invalid { .. }), "{err:?}");
    // ZERO wire traffic: the refusal is local.
    assert!(transport.calls().is_empty());

    // The same order with IOC goes to the wire (reduce_only: true rides).
    let (adapter, transport) =
        adapter_with(vec![(201, fixture_json("orders__funding_position_ioc"))]);
    block_on(adapter.place(&order, TimeInForce::ImmediateOrCancel, None)).unwrap();
    let calls = transport.calls();
    assert_eq!(calls[0].body.as_ref().unwrap()["reduce_only"], true);
    assert_eq!(
        calls[0].body.as_ref().unwrap()["time_in_force"],
        "immediate_or_cancel"
    );
}

#[test]
fn duplicate_client_id_resolves_to_already_exists_via_list_scan() {
    // Derived: the duplicate's client id and the expected venue order id
    // come from the FIRST entry of the recorded list itself.
    let listing = fixture_json("orders__list_all");
    let first = &listing["orders"][0];
    let client_id = first["client_order_id"].as_str().unwrap();
    let expected_order_id = first["order_id"].as_str().unwrap();
    let order = gate_order(3, client_id, 53_829, 1, Action::Buy, false, None);
    let (adapter, _) = adapter_with(vec![
        (409, fixture_json("orders__duplicate_client_order_id")),
        (200, listing.clone()),
    ]);
    let err =
        block_on(adapter.place(&order, TimeInForce::GoodTillCanceled, Some(false))).unwrap_err();
    let VenueError::AlreadyExists { existing } = &err else {
        panic!("expected AlreadyExists, got {err:?}");
    };
    assert_eq!(existing.as_str(), expected_order_id);
}

#[test]
fn duplicate_not_on_first_page_stays_rejected() {
    let order = gate_order(
        4,
        "ffffffff-0000-0000-0000-000000000000",
        53_829,
        1,
        Action::Buy,
        false,
        None,
    );
    let (adapter, _) = adapter_with(vec![
        (409, fixture_json("orders__duplicate_client_order_id")),
        (200, fixture_json("orders__list_all")),
    ]);
    let err =
        block_on(adapter.place(&order, TimeInForce::GoodTillCanceled, Some(false))).unwrap_err();
    assert!(matches!(err, VenueError::Rejected { .. }), "{err:?}");
}

// ---- fills: classification + fee reconciliation ----

#[test]
fn recorded_fill_classifies_user_and_surfaces_the_promo_fee_discrepancy() {
    let tiers: dto::FeeTiersResponse = serde_json::from_value(fixture_json("fees__tiers")).unwrap();
    let (adapter, _) = adapter_with(vec![(200, fixture_json("fills__after_open"))]);
    let (fills, discrepancies) =
        block_on(adapter.fills_reconciled(Some("KXBTCPERP1"), Some(10), &tiers)).unwrap();
    assert_eq!(fills.len(), 1);
    let PerpFillClass::User(fill) = &fills[0] else {
        panic!("recorded fill is order_source=user");
    };
    assert_eq!(fill.price, PerpPrice::new(63_587));
    assert_eq!(fill.count, Contracts::new(1));
    assert_eq!(fill.fee, Cents::ZERO);
    assert!(fill.is_taker);
    // The fee-trap reality: charged $0 (promo) vs posted taker 0.0012 ->
    // modeled ceil(6.3587 x 0.0012) = 1c. The discrepancy MUST surface.
    assert_eq!(discrepancies.len(), 1);
    assert_eq!(discrepancies[0].modeled, Cents::new(1));
    assert_eq!(discrepancies[0].charged, Cents::ZERO);
}

#[test]
fn system_fill_classifies_as_liquidation_never_user() {
    // order_source = "system" is the venue's liquidation execution class
    // (research §6 / OpenAPI MarginFill.order_source). The recorded demo
    // session produced no liquidation; this body is the RECORDED fill
    // with only order_source flipped to the research-grounded value.
    let mut body = fixture_json("fills__after_open");
    body["fills"][0]["order_source"] = serde_json::Value::String("system".into());
    let tiers: dto::FeeTiersResponse = serde_json::from_value(fixture_json("fees__tiers")).unwrap();
    let (adapter, _) = adapter_with(vec![(200, body)]);
    let (fills, _) =
        block_on(adapter.fills_reconciled(Some("KXBTCPERP1"), Some(10), &tiers)).unwrap();
    assert_eq!(fills.len(), 1);
    assert!(
        matches!(&fills[0], PerpFillClass::Liquidation(_)),
        "system fills must never classify as user fills"
    );
}

#[test]
fn matching_fee_produces_no_discrepancy() {
    // Same recorded fill with the charged fee set to the modeled value
    // (1c = $0.0100): reconciliation is quiet when the venue charges
    // what the tiers say.
    let mut body = fixture_json("fills__after_open");
    body["fills"][0]["fees"] = serde_json::Value::String("0.0100".into());
    let tiers: dto::FeeTiersResponse = serde_json::from_value(fixture_json("fees__tiers")).unwrap();
    let (adapter, _) = adapter_with(vec![(200, body)]);
    let (_, discrepancies) =
        block_on(adapter.fills_reconciled(Some("KXBTCPERP1"), Some(10), &tiers)).unwrap();
    assert!(discrepancies.is_empty());
}

#[test]
fn unknown_ticker_in_tiers_fails_closed() {
    let mut tiers: dto::FeeTiersResponse =
        serde_json::from_value(fixture_json("fees__tiers")).unwrap();
    tiers.taker_fee_rates.remove("KXBTCPERP1");
    let (adapter, _) = adapter_with(vec![(200, fixture_json("fills__after_open"))]);
    let err = block_on(adapter.fills_reconciled(Some("KXBTCPERP1"), Some(10), &tiers)).unwrap_err();
    assert!(matches!(err, VenueError::Invalid { .. }), "{err:?}");
}

// ---- positions / cancel ----

#[test]
fn positions_read_types_the_recorded_position() {
    let (adapter, _) = adapter_with(vec![(200, fixture_json("positions__open"))]);
    let positions = block_on(adapter.positions()).unwrap();
    assert_eq!(positions.len(), 1);
    let p = &positions[0];
    assert_eq!(p.position.qty, Contracts::new(1));
    assert_eq!(p.position.avg_entry, PerpPrice::new(63_587));
    // margin_used 0.8259 -> CEIL -> 83c; unrealized -0.0258 -> FLOOR -> -3c.
    assert_eq!(p.margin_used, Cents::new(83));
    assert_eq!(p.unrealized_pnl, Cents::new(-3));
    assert_eq!(p.fees_paid, Cents::ZERO);
}

#[test]
fn cancel_returns_reduced_by_and_maps_not_found() {
    let (adapter, _) = adapter_with(vec![(200, fixture_json("orders__cancel"))]);
    let reduced = block_on(adapter.cancel("c445aeac-f95b-4c96-8086-faacebfd300d")).unwrap();
    assert_eq!(reduced, Contracts::new(1));

    let (adapter, _) = adapter_with(vec![(404, fixture_json("cleanup__leftover_0"))]);
    let err = block_on(adapter.cancel("82c52513-39ed-4008-9aa0-73546b956f7a")).unwrap_err();
    assert!(matches!(err, VenueError::NotFound { .. }), "{err:?}");
}
