//! Paper-on-live deterministic replay: live Kalshi reads, local paper fills.
//!
//! The property pinned here is the safety/realism pair for the new mode:
//! a touch print never fills a resting paper order, a through print does, and
//! replaying the same seed yields byte-identical paper fills with only GET
//! calls against the Kalshi read transport.

use fortuna_core::book::Cursor;
use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline};
use fortuna_paper::{PaperConfig, PaperLiveVenue};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::kalshi::client::{KalshiTransport, MockKalshiTransport};
use fortuna_venues::kalshi::KalshiReadClient;
use fortuna_venues::{MarketFilter, MarketStatus, Venue};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FillView {
    id: String,
    price: i64,
    qty: i64,
    maker: bool,
}

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-16T12:00:00.000Z").unwrap()
}

fn market_id() -> MarketId {
    MarketId::new("KXTEST-26JUN16-T50").unwrap()
}

fn seed_from_corpus() -> u64 {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fortuna-core/dst-corpus/paper-live-through-not-touch.seed");
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .expect("seed file has one seed line")
        .parse()
        .expect("seed parses as u64")
}

fn fee_model() -> ScheduleFeeModel {
    let schedule: FeeSchedule = toml::from_str(
        r#"
        formula = "quadratic"
        effective_date = "2026-01-01"
        taker_coeff = "0.07"
        maker_coeff = "0"
        "#,
    )
    .unwrap();
    ScheduleFeeModel::new(vec![schedule]).unwrap()
}

fn gate_config() -> GateConfig {
    toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 10000000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 10000
        price_band_cents = 49
        max_cross_cents = 98
        per_market_exposure_cents = 10000000
        per_event_exposure_cents = 10000000
        require_event_mapping = false

        [per_strategy.paper_live_dst]
        max_exposure_cents = 10000000
        max_order_notional_cents = 10000000
        min_net_edge_bps = 0

        [rate."paper-live"]
        burst = 100000
        sustained_per_min = 100000
        market_burst = 100000
        market_sustained_per_min = 100000
        "#,
    )
    .unwrap()
}

fn gated_buy(pipeline: &mut GatePipeline, seed: u64, n: u64) -> fortuna_gates::GatedOrder {
    let mut ids = IdGen::new(seed.wrapping_add(n));
    let intent = IntentId::new(ids.next(t0()).unwrap());
    let candidate = CandidateOrder {
        intent_id: intent,
        strategy: StrategyId::new("paper_live_dst").unwrap(),
        venue: VenueId::new("paper-live").unwrap(),
        market: market_id(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(40),
        qty: Contracts::new(1),
        fair_value: Cents::new(50),
        client_order_id: ClientOrderId::from_intent(intent),
    };
    let fees = fee_model();
    let recent = BTreeSet::new();
    let inputs = GateInputs {
        now: t0(),
        open_exposure_cents: Cents::ZERO,
        market_exposure_cents: Cents::ZERO,
        strategy_exposure_cents: Cents::ZERO,
        event_exposure_cents: Cents::ZERO,
        event_id: None,
        book: None,
        last_trade_price: Some(Cents::new(50)),
        fee_model: &fees,
        category: None,
        own_resting: &[],
        recent_client_order_ids: &recent,
    };
    pipeline.evaluate(&candidate, &inputs).gated.unwrap()
}

fn series_body() -> serde_json::Value {
    serde_json::json!({
        "series": {
            "ticker": "KXTEST",
            "title": "Test series",
            "category": "weather",
            "fee_type": "quadratic",
            "fee_multiplier": 1.0,
            "settlement_sources": [{"name": "test", "url": "https://example.test/rules"}]
        }
    })
}

fn markets_body() -> serde_json::Value {
    serde_json::json!({
        "markets": [{
            "ticker": "KXTEST-26JUN16-T50",
            "event_ticker": "KXTEST-26JUN16",
            "market_type": "binary",
            "title": "Will the test settle yes?",
            "yes_sub_title": "Yes",
            "no_sub_title": "No",
            "status": "active",
            "strike_type": "greater",
            "floor_strike": 50,
            "close_time": "2026-06-16T17:00:00Z",
            "settlement_timer_seconds": 3600,
            "notional_value_dollars": "1.0000",
            "price_level_structure": "linear_cent",
            "volume_fp": "100.00"
        }],
        "cursor": ""
    })
}

fn book_body() -> serde_json::Value {
    serde_json::json!({
        "orderbook_fp": {
            "yes_dollars": [["0.3000", "100.00"]],
            "no_dollars": [["0.4000", "100.00"]]
        }
    })
}

fn trades_body(trade_id: &str, yes_cents: i64, created_time: &str) -> serde_json::Value {
    serde_json::json!({
        "trades": [{
            "trade_id": trade_id,
            "ticker": "KXTEST-26JUN16-T50",
            "yes_price_dollars": format!("0.{yes_cents:02}00"),
            "no_price_dollars": format!("0.{:02}00", 100 - yes_cents),
            "count_fp": "20.00",
            "taker_outcome_side": "yes",
            "taker_book_side": "bid",
            "created_time": created_time
        }],
        "cursor": ""
    })
}

fn script_transport(mock: &MockKalshiTransport) {
    mock.push_ok(200, series_body());
    mock.push_ok(200, markets_body());
    mock.push_ok(200, book_body());
    mock.push_ok(200, serde_json::json!({ "trades": [], "cursor": "" }));
    mock.push_ok(200, markets_body());
    mock.push_ok(200, book_body());
    mock.push_ok(200, trades_body("touch-40", 40, "2026-06-16T12:00:05.000Z"));
    mock.push_ok(200, markets_body());
    mock.push_ok(200, book_body());
    mock.push_ok(
        200,
        trades_body("through-39", 39, "2026-06-16T12:00:10.000Z"),
    );
}

fn refresh(venue: &PaperLiveVenue) {
    futures::executor::block_on(venue.refresh_market_data_for(MarketFilter {
        category: None,
        status: Some(MarketStatus::Trading),
    }))
    .unwrap();
}

fn run(seed: u64) -> (Vec<FillView>, Vec<String>) {
    let mock = Arc::new(MockKalshiTransport::new());
    script_transport(&mock);
    let clock: Arc<dyn Clock> = Arc::new(SimClock::new(t0()));
    let read = KalshiReadClient::new(
        mock.clone() as Arc<dyn KalshiTransport>,
        clock.clone(),
        vec!["KXTEST".to_string()],
    )
    .unwrap();
    let venue = PaperLiveVenue::new(
        read,
        clock,
        fee_model(),
        PaperConfig {
            maker_haircut_pct: 100,
        },
        Cents::new(1_000_000),
    )
    .unwrap();

    refresh(&venue);
    let mut pipeline = GatePipeline::new(gate_config()).unwrap();
    for n in 0..5 {
        futures::executor::block_on(venue.place(gated_buy(&mut pipeline, seed, n))).unwrap();
    }

    refresh(&venue);
    assert!(
        futures::executor::block_on(venue.fills_since(Cursor::start()))
            .unwrap()
            .fills
            .is_empty(),
        "touch print at 40 must not fill resting bids at 40"
    );

    refresh(&venue);
    let fills = futures::executor::block_on(venue.fills_since(Cursor::start()))
        .unwrap()
        .fills
        .into_iter()
        .map(|fill| FillView {
            id: fill.fill_id,
            price: fill.price.raw(),
            qty: fill.qty.raw(),
            maker: fill.is_maker,
        })
        .collect::<Vec<_>>();
    let calls = mock
        .calls()
        .into_iter()
        .map(|call| format!("{} {}", call.method, call.path))
        .collect();
    (fills, calls)
}

#[test]
fn paper_live_through_not_touch_replay_is_deterministic() {
    let seed = seed_from_corpus();
    let (fills_a, calls_a) = run(seed);
    let (fills_b, calls_b) = run(seed);

    assert_eq!(fills_a, fills_b, "paper-live replay must be byte-identical");
    assert_eq!(calls_a, calls_b, "read call sequence must be deterministic");
    assert!(
        !fills_a.is_empty(),
        "through print must produce paper maker fills"
    );
    assert!(fills_a.iter().all(|fill| fill.price == 40 && fill.maker));
    assert!(
        calls_a.iter().all(|call| {
            call.starts_with("GET ")
                && !call.contains("/portfolio/order")
                && !call.contains("/portfolio/orders")
        }),
        "paper-live DST must issue only read calls: {calls_a:?}"
    );
}
