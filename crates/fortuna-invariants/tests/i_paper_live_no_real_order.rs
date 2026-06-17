//! Paper-on-live must never place or cancel a real Kalshi order.
//!
//! This is an additions-only protected invariant. The read side is allowed to
//! hit live Kalshi market-data endpoints; execution must stay entirely inside
//! `PaperVenue`, even when callers drive gated `place` and `cancel` calls
//! through the composite `Venue` trait.

use async_trait::async_trait;
use fortuna_core::book::{Cursor, FeeError, FeeModel, FillRole};
use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline};
use fortuna_paper::{PaperConfig, PaperLiveVenue};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::kalshi::client::{KalshiTransport, RecordedCall};
use fortuna_venues::kalshi::KalshiReadClient;
use fortuna_venues::{MarketFilter, MarketStatus, Venue, VenueError};
use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-16T12:00:00.000Z").unwrap()
}

fn market_id() -> MarketId {
    MarketId::new("KXTEST-26JUN16-T50").unwrap()
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

        [per_strategy.paper_live_invariant]
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

fn gated_buy(pipeline: &mut GatePipeline, n: u64) -> fortuna_gates::GatedOrder {
    let mut ids = IdGen::new(10_000 + n);
    let intent = IntentId::new(ids.next(t0()).unwrap());
    let candidate = CandidateOrder {
        intent_id: intent,
        strategy: StrategyId::new("paper_live_invariant").unwrap(),
        venue: VenueId::new("paper-live").unwrap(),
        market: market_id(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(40),
        qty: Contracts::new(1),
        fair_value: Cents::new(50),
        client_order_id: ClientOrderId::new(format!("paper-live-invariant-{n}")).unwrap(),
    };
    let fees = ZeroFees;
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

fn trades_body(trade_id: &str, yes_cents: i64, created_time: &str, qty: i64) -> serde_json::Value {
    serde_json::json!({
        "trades": [{
            "trade_id": trade_id,
            "ticker": "KXTEST-26JUN16-T50",
            "yes_price_dollars": format!("0.{yes_cents:02}00"),
            "no_price_dollars": format!("0.{:02}00", 100 - yes_cents),
            "count_fp": format!("{qty}.00"),
            "taker_outcome_side": "yes",
            "taker_book_side": "bid",
            "created_time": created_time
        }],
        "cursor": ""
    })
}

#[derive(Default)]
struct GuardedKalshiTransport {
    script: Mutex<VecDeque<Result<(u16, serde_json::Value), VenueError>>>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl GuardedKalshiTransport {
    fn push_ok(&self, status: u16, body: serde_json::Value) {
        self.script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(Ok((status, body)));
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl KalshiTransport for GuardedKalshiTransport {
    async fn request(
        &self,
        method: &str,
        path: &str,
        query: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> Result<(u16, serde_json::Value), VenueError> {
        if method != "GET" || path.contains("/portfolio/order") {
            panic!("paper-on-live attempted a real execution endpoint: {method} {path}");
        }
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(RecordedCall {
                method: method.to_string(),
                path: path.to_string(),
                query: query.map(str::to_string),
                body,
            });
        self.script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| {
                Err(VenueError::Invalid {
                    reason: format!("unscripted transport call: {method} {path}"),
                })
            })
    }
}

fn refresh(venue: &PaperLiveVenue) {
    futures::executor::block_on(venue.refresh_market_data_for(MarketFilter {
        category: None,
        status: Some(MarketStatus::Trading),
    }))
    .unwrap();
}

#[test]
fn paper_on_live_cannot_place_or_cancel_real_orders() {
    let guarded = Arc::new(GuardedKalshiTransport::default());
    guarded.push_ok(200, series_body());
    guarded.push_ok(200, markets_body());
    guarded.push_ok(200, book_body());
    guarded.push_ok(
        200,
        serde_json::json!({
            "trades": [],
            "cursor": ""
        }),
    );
    guarded.push_ok(200, markets_body());
    guarded.push_ok(200, book_body());
    guarded.push_ok(
        200,
        trades_body("touch-40", 40, "2026-06-16T12:00:05.000Z", 100),
    );
    guarded.push_ok(200, markets_body());
    guarded.push_ok(200, book_body());
    guarded.push_ok(
        200,
        trades_body("through-39", 39, "2026-06-16T12:00:10.000Z", 100),
    );

    let clock: Arc<dyn Clock> = Arc::new(SimClock::new(t0()));
    let read = KalshiReadClient::new(
        guarded.clone() as Arc<dyn KalshiTransport>,
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
    let mut order_ids = Vec::new();
    for n in 0..50 {
        let id = futures::executor::block_on(venue.place(gated_buy(&mut pipeline, n))).unwrap();
        order_ids.push(id);
    }

    for id in order_ids.iter().step_by(10) {
        futures::executor::block_on(venue.cancel(id)).unwrap();
    }

    refresh(&venue);
    assert!(
        futures::executor::block_on(venue.fills_since(Cursor::start()))
            .unwrap()
            .fills
            .is_empty(),
        "a public print at the resting limit is a touch and must not paper-fill"
    );

    refresh(&venue);
    let fills = futures::executor::block_on(venue.fills_since(Cursor::start()))
        .unwrap()
        .fills;
    assert!(
        !fills.is_empty(),
        "the through print must produce paper fills after touch prints produced none"
    );
    assert!(
        fills.len() <= 45,
        "paper fills cannot exceed the 45 uncancelled resting orders"
    );
    for fill in fills {
        assert!(fill.fill_id.starts_with("p-"));
        assert_eq!(fill.market, market_id());
        assert_eq!(fill.price, Cents::new(40));
        assert_eq!(fill.qty.raw(), 1);
        assert!(
            fill.is_maker,
            "paper-on-live fills must be maker fills from public trade-throughs"
        );
    }

    let calls = guarded.calls();
    assert!(calls.iter().all(|call| call.method == "GET"));
    assert!(
        calls
            .iter()
            .all(|call| !call.path.contains("/portfolio/order")),
        "paper-on-live must never call Kalshi order endpoints: {calls:?}"
    );
}
