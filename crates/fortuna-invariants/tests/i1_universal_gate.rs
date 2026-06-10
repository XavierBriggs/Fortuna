//! I1: universal gate
//!
//! Property to encode: no code path constructs a venue-acceptable order except the gate pipeline; property-test that arbitrary order sequences reaching Venue::place all carry gate verdict audit rows; GatedOrder is unconstructible outside fortuna-gates (compile-fail test)
//!
//! Implemented in T0.5 per tests/README.md (stubs are implemented, never
//! weakened, by their owning BUILD_PLAN task):
//! - The compile-fail half lives as `compile_fail` doc-tests on
//!   fortuna-invariants/src/lib.rs (GatedOrder: private fields, no public
//!   constructor, and no Deserialize impl).
//! - The runtime half below: `Venue::place` accepts only `GatedOrder` (the
//!   place call type-checks ONLY with a pipeline-produced order), every
//!   order that reaches a venue carries a complete 10-check pass audit
//!   trail, and every rejection carries a trail ending in the failing check.

use fortuna_core::book::{FeeError, FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline, Verdict};
use fortuna_venues::sim::{FaultConfig, SimVenue};
use fortuna_venues::{Market, MarketStatus, SettlementMeta, Venue};
use proptest::prelude::*;
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

        [per_strategy.i1]
        max_exposure_cents = 1000000
        max_order_notional_cents = 1000000
        min_net_edge_bps = 0

        [rate.sim]
        burst = 100000
        sustained_per_min = 100000
        market_burst = 100000
        market_sustained_per_min = 100000
        "#,
    )
    .unwrap()
}

fn sim_venue() -> SimVenue {
    let clock = Arc::new(SimClock::new(t0()));
    let s: fortuna_venues::fees::FeeSchedule = toml::from_str(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
        "#,
    )
    .unwrap();
    let fees = fortuna_venues::fees::ScheduleFeeModel::new(vec![s]).unwrap();
    let v = SimVenue::new(
        VenueId::new("sim").unwrap(),
        clock,
        fees,
        FaultConfig::none(1),
        Cents::new(10_000_000),
    );
    v.add_market(Market {
        id: MarketId::new("I1-MKT").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        title: "I1 market".into(),
        category: "test".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 0,
        },
        payout_per_contract: Cents::new(100),
    });
    v
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

fn candidate(n: u64, price: i64, qty: i64, fair: i64) -> CandidateOrder {
    let mut g = IdGen::new(n + 1);
    CandidateOrder {
        intent_id: IntentId::new(g.next(t0()).unwrap()),
        strategy: StrategyId::new("i1").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        market: MarketId::new("I1-MKT").unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(price),
        qty: Contracts::new(qty),
        fair_value: Cents::new(fair),
        client_order_id: ClientOrderId::new(format!("i1-{n}")).unwrap(),
    }
}

fn inputs<'a>(
    book: &'a OrderBook,
    fees: &'a ZeroFees,
    recent: &'a BTreeSet<String>,
) -> GateInputs<'a> {
    GateInputs {
        now: t0(),
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

/// The original stub, implemented: a deterministic sweep proving that the
/// only orders a venue ever sees were produced by the pipeline WITH their
/// full verdict trail, and that rejected candidates yield no placeable
/// artifact (there is nothing of type GatedOrder to place).
#[test]
fn i1_universal_gate() {
    let mut pipeline = GatePipeline::new(permissive_config()).unwrap();
    let venue = sim_venue();
    let market = MarketId::new("I1-MKT").unwrap();
    let book = book(&market);
    let fees = ZeroFees;
    let recent = BTreeSet::new();

    let cases: Vec<(i64, i64, i64)> = vec![
        (50, 10, 60),  // clean pass
        (50, 0, 60),   // rejected: size
        (120, 10, 60), // rejected: price insanity
        (50, 10, 40),  // rejected: negative edge
        (45, 5, 55),   // clean pass
    ];
    for (n, (price, qty, fair)) in cases.into_iter().enumerate() {
        let i = inputs(&book, &fees, &recent);
        let out = pipeline.evaluate(&candidate(n as u64, price, qty, fair), &i);
        match out.gated {
            Ok(gated) => {
                // Venue-acceptable orders exist ONLY with a complete pass
                // trail: all ten checks, in order, every verdict Pass.
                assert_eq!(out.records.len(), 10);
                assert!(out.records.iter().all(|r| r.verdict == Verdict::Pass));
                for (idx, r) in out.records.iter().enumerate() {
                    assert_eq!(r.check.index(), idx + 1);
                }
                // `Venue::place` accepts exactly this type and nothing else
                // (type-level I1: this call is only writable with a
                // pipeline-produced order).
                futures::executor::block_on(venue.place(gated)).unwrap();
            }
            Err(rejection) => {
                // A rejection ends the trail at the failing check and
                // produces NO placeable artifact.
                let last = out.records.last().unwrap();
                assert_eq!(last.verdict, Verdict::Reject);
                assert_eq!(last.check, rejection.check);
            }
        }
    }
}

proptest! {
    /// For ALL candidate orders: a venue-acceptable order implies a complete
    /// 10-pass verdict trail; a rejection implies a trail ending at the
    /// failing check. (Added alongside the implemented stub; spec I1.)
    #[test]
    fn i1_prop_all_orders_carry_gate_verdicts(
        price in -10i64..120,
        qty in -10i64..50,
        fair in -10i64..120,
        seed in 0u64..1000,
    ) {
        let mut pipeline = GatePipeline::new(permissive_config()).unwrap();
        let market = MarketId::new("I1-MKT").unwrap();
        let book = book(&market);
        let fees = ZeroFees;
        let recent = BTreeSet::new();
        let i = inputs(&book, &fees, &recent);
        let out = pipeline.evaluate(&candidate(seed, price, qty, fair), &i);
        match &out.gated {
            Ok(_) => {
                prop_assert_eq!(out.records.len(), 10);
                prop_assert!(out.records.iter().all(|r| r.verdict == Verdict::Pass));
            }
            Err(rejection) => {
                let last = out.records.last().unwrap();
                prop_assert_eq!(last.verdict, Verdict::Reject);
                prop_assert_eq!(last.check, rejection.check);
            }
        }
    }
}
