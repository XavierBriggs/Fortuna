//! I3: runaway detection halts, never throttles
//!
//! Property to encode: exceeding burst or sustained token buckets per venue/market sets a halt (not a delay); duplicate client order ids are rejected exactly-once under duplicate delivery faults
//!
//! Implemented in T0.5 per tests/README.md (subset: gate-side rate breach
//! semantics and duplicate-coid exactly-once placement; full
//! duplicate-delivery fault coverage extends through DST at T0.6).

use fortuna_core::book::{FeeError, FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_gates::{CandidateOrder, GateCheck, GateConfig, GateInputs, GatePipeline, HaltScope};
use fortuna_venues::sim::{FaultConfig, PlaceOrder, SimVenue};
use fortuna_venues::{Market, MarketStatus, SettlementMeta, VenueError};
use std::collections::BTreeSet;
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
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

fn config(burst: u32, market_burst: u32) -> GateConfig {
    toml::from_str(&format!(
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

        [per_strategy.i3]
        max_exposure_cents = 1000000
        max_order_notional_cents = 1000000
        min_net_edge_bps = 0

        [rate.sim]
        burst = {burst}
        sustained_per_min = 60
        market_burst = {market_burst}
        market_sustained_per_min = 60
        "#
    ))
    .unwrap()
}

fn book(market: &MarketId) -> OrderBook {
    OrderBook {
        market: market.clone(),
        as_of: t0(),
        yes_bids: vec![PriceLevel {
            price: Cents::new(49),
            qty: Contracts::new(1000),
        }],
        yes_asks: vec![PriceLevel {
            price: Cents::new(51),
            qty: Contracts::new(1000),
        }],
    }
}

fn candidate(n: u64, market: &str) -> CandidateOrder {
    let mut g = IdGen::new(n + 1);
    CandidateOrder {
        intent_id: IntentId::new(g.next(t0()).unwrap()),
        strategy: StrategyId::new("i3").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        market: MarketId::new(market).unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(50),
        qty: Contracts::new(1),
        fair_value: Cents::new(60),
        client_order_id: ClientOrderId::new(format!("i3-{n}")).unwrap(),
    }
}

/// The original stub, implemented: breaching either bucket is a HALT that
/// time does not clear, and duplicate client order ids never double-place.
#[test]
fn i3_runaway_halt() {
    // --- venue-bucket breach (burst 2, ample market buckets) ---
    let mut p = GatePipeline::new(config(2, 100)).unwrap();
    let fees = ZeroFees;
    let recent = BTreeSet::new();
    let market = MarketId::new("M1").unwrap();
    let b = book(&market);
    let mk_inputs = |now: UtcTimestamp| GateInputs {
        now,
        open_exposure_cents: Cents::ZERO,
        market_exposure_cents: Cents::ZERO,
        strategy_exposure_cents: Cents::ZERO,
        event_exposure_cents: Cents::ZERO,
        event_id: None,
        book: Some(&b),
        last_trade_price: None,
        fee_model: &fees,
        category: None,
        own_resting: &[],
        recent_client_order_ids: &recent,
    };

    assert!(p
        .evaluate(&candidate(0, "M1"), &mk_inputs(t0()))
        .gated
        .is_ok());
    assert!(p
        .evaluate(&candidate(1, "M1"), &mk_inputs(t0()))
        .gated
        .is_ok());
    // Third submission breaches the venue bucket: rejected at RateLimits...
    let out = p.evaluate(&candidate(2, "M1"), &mk_inputs(t0()));
    assert_eq!(out.gated.unwrap_err().check, GateCheck::RateLimits);
    // ...and the venue is HALTED.
    assert!(p.halts().venue_halted("sim").is_some());

    // A halt, not a throttle: a full hour of token refill changes nothing.
    let later = t0().checked_add_millis(3_600_000).unwrap();
    let out = p.evaluate(&candidate(3, "M1"), &mk_inputs(later));
    assert_eq!(out.gated.unwrap_err().check, GateCheck::Halts);

    // Only the operator re-arm path clears it (I2/I3).
    p.rearm(HaltScope::Venue("sim".into())).unwrap();
    assert!(p
        .evaluate(&candidate(4, "M1"), &mk_inputs(later))
        .gated
        .is_ok());

    // --- market-bucket breach (ample venue bucket, market burst 1) ---
    let mut p = GatePipeline::new(config(100, 1)).unwrap();
    let b2 = book(&MarketId::new("M2").unwrap());
    let b3 = book(&MarketId::new("M3").unwrap());
    fn inputs_for<'a>(
        book: &'a OrderBook,
        fees: &'a ZeroFees,
        recent: &'a BTreeSet<String>,
    ) -> GateInputs<'a> {
        GateInputs {
            now: UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap(),
            open_exposure_cents: Cents::ZERO,
            market_exposure_cents: Cents::ZERO,
            strategy_exposure_cents: Cents::ZERO,
            event_exposure_cents: Cents::ZERO,
            event_id: None,
            book: Some(book),
            last_trade_price: None,
            fee_model: fees,
            category: None,
            own_resting: &[],
            recent_client_order_ids: recent,
        }
    }
    assert!(p
        .evaluate(&candidate(10, "M2"), &inputs_for(&b2, &fees, &recent))
        .gated
        .is_ok());
    let out = p.evaluate(&candidate(11, "M2"), &inputs_for(&b2, &fees, &recent));
    assert_eq!(out.gated.unwrap_err().check, GateCheck::RateLimits);
    assert!(p.halts().venue_halted("sim").is_some());
    let out = p.evaluate(&candidate(12, "M3"), &inputs_for(&b3, &fees, &recent));
    assert_eq!(out.gated.unwrap_err().check, GateCheck::Halts);

    // --- duplicate client order ids: rejected, never double-placed ---
    // Gate side: a coid the journal already knows is rejected at check 8.
    let mut p = GatePipeline::new(config(100, 100)).unwrap();
    let mut seen = BTreeSet::new();
    seen.insert("i3-dup".to_string());
    let dup_inputs = GateInputs {
        now: t0(),
        open_exposure_cents: Cents::ZERO,
        market_exposure_cents: Cents::ZERO,
        strategy_exposure_cents: Cents::ZERO,
        event_exposure_cents: Cents::ZERO,
        event_id: None,
        book: Some(&b),
        last_trade_price: None,
        fee_model: &fees,
        category: None,
        own_resting: &[],
        recent_client_order_ids: &seen,
    };
    let mut c = candidate(20, "M1");
    c.client_order_id = ClientOrderId::new("i3-dup").unwrap();
    let out = p.evaluate(&c, &dup_inputs);
    assert_eq!(out.gated.unwrap_err().check, GateCheck::Idempotency);

    // Venue side: resubmitting the same coid N times yields exactly one
    // live order, with every duplicate refused naming the original.
    let clock = Arc::new(SimClock::new(t0()));
    let s: fortuna_venues::fees::FeeSchedule = toml::from_str(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
        "#,
    )
    .unwrap();
    let venue = SimVenue::new(
        VenueId::new("sim").unwrap(),
        clock,
        fortuna_venues::fees::ScheduleFeeModel::new(vec![s]).unwrap(),
        FaultConfig::none(1),
        Cents::new(100_000),
    );
    venue.add_market(Market {
        id: MarketId::new("M1").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        title: "I3 market".into(),
        category: "test".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 0,
        },
        volume_contracts: None,
        payout_per_contract: Cents::new(100),
    });
    let req = PlaceOrder {
        market: MarketId::new("M1").unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(50),
        qty: Contracts::new(1),
        client_order_id: ClientOrderId::new("i3-venue-dup").unwrap(),
    };
    let original = venue.place_raw(req.clone()).unwrap();
    for _ in 0..5 {
        match venue.place_raw(req.clone()) {
            Err(VenueError::AlreadyExists { existing }) => assert_eq!(existing, original),
            other => panic!("duplicate must be refused with the original id, got {other:?}"),
        }
    }
    assert_eq!(venue.resting_orders().len(), 1);
}
