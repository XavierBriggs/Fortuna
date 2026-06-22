//! W4: `fortuna start paper-demo` — paper-live safety wall + pointer-write.
//!
//! # TDD — written BEFORE implementation; both tests must fail first.
//!
//! ## Test 1 — `paper_demo_transport_wall`
//! Proves the paper-live transport wall is load-bearing by driving a REAL
//! `PaperLiveVenue` backed by a `GuardedKalshiTransport`:
//! - `place()` and `cancel()` route to the in-memory `PaperVenue`, never to
//!   Kalshi order endpoints.
//! - Every call on the underlying Kalshi transport is a GET (market-data read);
//!   zero POST/DELETE/PATCH calls reach the transport — the wall holds.
//! - This mirrors the construction in `i_paper_live_no_real_order`; the
//!   production order wall is owned by that invariant (run at the boundary
//!   gate), and this test independently confirms the same invariant at the
//!   integration-test level.
//!
//! ## Test 2 — `pointer_write_lands_live_url`
//! After `fortuna_live::boot::write_demo_db_pointer(runtime_dir, url)` the
//! file `current-demo-db-url` under `runtime_dir` contains exactly `url`.
//! The write is atomic (temp + rename) so `runtime_dir` is the canonical path.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use async_trait::async_trait;
use fortuna_core::book::{Cursor, FeeError, FeeModel, FillRole};
use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline};
use fortuna_live::boot::write_demo_db_pointer;
use fortuna_paper::{PaperConfig, PaperLiveVenue};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::kalshi::client::{KalshiTransport, RecordedCall};
use fortuna_venues::kalshi::KalshiReadClient;
use fortuna_venues::{MarketFilter, MarketStatus, Venue, VenueError};
use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

// ---------------------------------------------------------------------------
// Helpers shared by both tests
// ---------------------------------------------------------------------------

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-16T12:00:00.000Z").unwrap()
}

fn market_id() -> MarketId {
    MarketId::new("KXTEST-26JUN16-T50").unwrap()
}

// ---------------------------------------------------------------------------
// Guarded transport — panics on any non-GET or any order endpoint call
// (mirrors i_paper_live_no_real_order's GuardedKalshiTransport pattern,
// targeting the REAL production transport wall).
// ---------------------------------------------------------------------------

#[derive(Default)]
struct GuardedTransport {
    script: Mutex<VecDeque<Result<(u16, serde_json::Value), VenueError>>>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl GuardedTransport {
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
impl KalshiTransport for GuardedTransport {
    async fn request(
        &self,
        method: &str,
        path: &str,
        query: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> Result<(u16, serde_json::Value), VenueError> {
        // WALL: any non-GET or any /portfolio/order call is a real-order attempt.
        if method != "GET" || path.contains("/portfolio/order") {
            panic!(
                "paper-live transport wall violated: real execution endpoint attempted: \
                 {method} {path}"
            );
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
                    reason: format!("unscripted call: {method} {path}"),
                })
            })
    }
}

// ---------------------------------------------------------------------------
// Fee model for the paper venue
// ---------------------------------------------------------------------------

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

        [per_strategy.paper_live_wall]
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
    let mut ids = IdGen::new(20_000 + n);
    let intent = IntentId::new(ids.next(t0()).unwrap());
    let candidate = CandidateOrder {
        intent_id: intent,
        strategy: StrategyId::new("paper_live_wall").unwrap(),
        venue: VenueId::new("paper-live").unwrap(),
        market: market_id(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(40),
        qty: Contracts::new(1),
        fair_value: Cents::new(50),
        client_order_id: ClientOrderId::new(format!("paper-live-wall-{n}")).unwrap(),
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

// Fixture bodies matching the invariant test (same series/market/book shapes).
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

fn refresh(venue: &PaperLiveVenue) {
    futures::executor::block_on(venue.refresh_market_data_for(MarketFilter {
        category: None,
        status: Some(MarketStatus::Trading),
    }))
    .unwrap();
}

// ---------------------------------------------------------------------------
// Test 1 — paper-demo transport wall via REAL PaperLiveVenue
// ---------------------------------------------------------------------------

/// Drives a REAL `PaperLiveVenue` backed by a `GuardedKalshiTransport` that
/// panics on any non-GET or order-endpoint call. Proves the paper-live
/// transport wall is load-bearing:
///
/// 1. `place()` and `cancel()` route to the in-memory PaperVenue — zero calls
///    to real Kalshi order endpoints.
/// 2. Every call on the underlying transport is a GET (market-data read).
/// 3. Paper fills arrive after a through-print, confirming the paper-fill path
///    is live and not vacuous.
///
/// The production order wall is owned by `i_paper_live_no_real_order`
/// (run at the boundary gate). This test independently confirms the same
/// invariant at the integration-test level, using the real venue code-path —
/// NOT a locally-defined copy of the guard.
#[test]
fn paper_demo_transport_wall() {
    let guarded = Arc::new(GuardedTransport::default());
    // Prime the transport with the same fixture sequence as the invariant test.
    guarded.push_ok(200, series_body());
    guarded.push_ok(200, markets_body());
    guarded.push_ok(200, book_body());
    guarded.push_ok(200, serde_json::json!({"trades": [], "cursor": ""}));
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

    // Refresh market data (GET calls only — the wall must not fire here).
    refresh(&venue);

    // Place orders via the real PaperLiveVenue — must route to paper, not Kalshi.
    let mut pipeline = GatePipeline::new(gate_config()).unwrap();
    let mut order_ids = Vec::new();
    for n in 0..10 {
        let id = futures::executor::block_on(venue.place(gated_buy(&mut pipeline, n))).unwrap();
        order_ids.push(id);
    }

    // Cancel some — again, must route to paper.
    for id in order_ids.iter().step_by(5) {
        futures::executor::block_on(venue.cancel(id)).unwrap();
    }

    // Touch print: no fills yet.
    refresh(&venue);
    assert!(
        futures::executor::block_on(venue.fills_since(Cursor::start()))
            .unwrap()
            .fills
            .is_empty(),
        "a touch print must not produce paper fills"
    );

    // Through print: paper fills should arrive.
    refresh(&venue);
    let fills = futures::executor::block_on(venue.fills_since(Cursor::start()))
        .unwrap()
        .fills;
    assert!(!fills.is_empty(), "through print must produce paper fills");
    for fill in &fills {
        assert!(fill.fill_id.starts_with("p-"), "paper fill ids start with p-");
        assert_eq!(fill.market, market_id());
        assert!(fill.is_maker, "paper-on-live fills must be maker fills");
    }

    // THE LOAD-BEARING ASSERTION: every Kalshi transport call is a GET,
    // and none hit /portfolio/order.  If place()/cancel() had called the
    // real Kalshi order endpoints, GuardedTransport::request() would have
    // panicked above — reaching here proves they did not.
    let calls = guarded.calls();
    assert!(!calls.is_empty(), "must have made at least some GET calls");
    assert!(
        calls.iter().all(|c| c.method == "GET"),
        "all calls must be GET (market-data reads): {calls:?}"
    );
    assert!(
        calls.iter().all(|c| !c.path.contains("/portfolio/order")),
        "paper-live must never call real Kalshi order endpoints: {calls:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — pointer-write lands the live DATABASE_URL
// ---------------------------------------------------------------------------

/// After `write_demo_db_pointer(runtime_dir, url)` the file
/// `current-demo-db-url` under `runtime_dir` contains exactly `url`.
///
/// Uses a temp directory keyed with `{pid}-{test-name}`.
#[test]
fn pointer_write_lands_live_url() {
    let base = std::env::temp_dir().join(format!(
        "fortuna-paper-demo-ptr-{}-pointer-write",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    let runtime_dir = base.join("data").join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();

    let db_url = "postgres://fortuna_app@localhost/fortuna_demo_test";

    write_demo_db_pointer(&runtime_dir, db_url)
        .expect("write_demo_db_pointer must not fail on a writable directory");

    let pointer_path = runtime_dir.join("current-demo-db-url");
    assert!(
        pointer_path.exists(),
        "current-demo-db-url must exist after write_demo_db_pointer"
    );
    let contents = std::fs::read_to_string(&pointer_path)
        .expect("should be able to read current-demo-db-url");
    assert_eq!(
        contents.trim(),
        db_url,
        "current-demo-db-url must contain exactly the DATABASE_URL written"
    );

    // Idempotent: overwrite with a different URL and re-check.
    let db_url2 = "postgres://fortuna_app@localhost/fortuna_demo_test_2";
    write_demo_db_pointer(&runtime_dir, db_url2)
        .expect("second write_demo_db_pointer must also succeed");
    let contents2 = std::fs::read_to_string(&pointer_path).unwrap();
    assert_eq!(
        contents2.trim(),
        db_url2,
        "second write must overwrite atomically"
    );

    let _ = std::fs::remove_dir_all(&base);
}
