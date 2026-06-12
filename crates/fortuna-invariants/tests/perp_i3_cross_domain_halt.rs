//! I3 extension (spec 5.15; T5.B3 ADDITION — this file only adds pins,
//! it weakens nothing): the perp arm shares the SAME dual token buckets
//! and halt flags as the event-contract arm.
//!
//! Property to encode: a rate breach on EITHER arm is a halt, not a
//! throttle — and the halt is venue-scoped across BOTH domains: a perp
//! breach blocks subsequent event-contract orders on that venue, and an
//! event-contract breach blocks subsequent perp orders. No per-arm
//! bucket split exists that an order could route around.

use fortuna_core::book::{FeeError, FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{MarginAccountView, PerpPrice};
use fortuna_gates::perp::{PerpCandidateOrder, PerpGateInputs};
use fortuna_gates::{CandidateOrder, GateCheck, GateConfig, GateInputs, GatePipeline};
use std::collections::BTreeSet;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T08:00:00.000Z").unwrap()
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

/// Venue burst of 2: the third order on the venue — from EITHER arm —
/// breaches and halts.
fn pipeline() -> GatePipeline {
    let cfg: GateConfig = toml::from_str(
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

        [per_strategy.perp_i3]
        max_exposure_cents = 100000000
        max_order_notional_cents = 1000000
        min_net_edge_bps = 0

        [rate.kinetics]
        burst = 2
        sustained_per_min = 1
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
        "#,
    )
    .unwrap();
    GatePipeline::new(cfg).unwrap()
}

fn perp_candidate(n: u64) -> PerpCandidateOrder {
    let mut g = IdGen::new(n + 1);
    PerpCandidateOrder {
        intent_id: IntentId::new(g.next(t0()).unwrap()),
        strategy: StrategyId::new("perp_i3").unwrap(),
        venue: VenueId::new("kinetics").unwrap(),
        market: MarketId::new("KXBTCPERP").unwrap(),
        action: Action::Buy,
        reduce_only: false,
        limit_price: PerpPrice::new(62_600),
        qty: Contracts::new(10),
        fair_value: PerpPrice::new(63_500),
        holding_windows: 1,
        client_order_id: ClientOrderId::new(format!("perp-i3-{n}")).unwrap(),
    }
}

fn try_perp(p: &mut GatePipeline, n: u64) -> Result<(), GateCheck> {
    let account = MarginAccountView::compute(Cents::new(1_000_000), &[], Cents::ZERO)
        .expect("balance-only view");
    let recent = BTreeSet::new();
    let inputs = PerpGateInputs {
        now: t0(),
        account: &account,
        position: None,
        conservative_mark: PerpPrice::new(62_600),
        venue_open_notional_cents: Cents::ZERO,
        own_resting: &[],
        recent_client_order_ids: &recent,
    };
    match p.evaluate_perp(&perp_candidate(n), &inputs).gated {
        Ok(_) => Ok(()),
        Err(rej) => Err(rej.check),
    }
}

/// An event-contract (binary) order on the SAME venue id ("kinetics") —
/// the bucket and the halt are venue-scoped, not instrument-scoped.
fn try_binary(p: &mut GatePipeline, n: u64) -> Result<(), GateCheck> {
    let market = MarketId::new("BINARY-M").unwrap();
    let b = OrderBook {
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
    };
    let fees = ZeroFees;
    let recent = BTreeSet::new();
    let mut g = IdGen::new(n + 1);
    let candidate = CandidateOrder {
        intent_id: IntentId::new(g.next(t0()).unwrap()),
        strategy: StrategyId::new("perp_i3").unwrap(),
        venue: VenueId::new("kinetics").unwrap(),
        market,
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(50),
        qty: Contracts::new(1),
        fair_value: Cents::new(60),
        client_order_id: ClientOrderId::new(format!("bin-i3-{n}")).unwrap(),
    };
    let inputs = GateInputs {
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
        recent_client_order_ids: &recent,
    };
    match p.evaluate(&candidate, &inputs).gated {
        Ok(_) => Ok(()),
        Err(rej) => Err(rej.check),
    }
}

#[test]
fn perp_i3_perp_breach_halts_event_orders_too() {
    let mut gates = pipeline();
    // Burst 2: two perp orders consume the venue bucket.
    assert!(try_perp(&mut gates, 1).is_ok());
    assert!(try_perp(&mut gates, 2).is_ok());
    // The third breaches: halt, not throttle.
    assert_eq!(try_perp(&mut gates, 3), Err(GateCheck::RateLimits));
    // The SAME venue is now halted for event-contract orders.
    assert_eq!(try_binary(&mut gates, 4), Err(GateCheck::Halts));
    // And further perp orders die at check 1, not at the bucket.
    assert_eq!(try_perp(&mut gates, 5), Err(GateCheck::Halts));
}

#[test]
fn perp_i3_event_breach_halts_perp_orders_too() {
    let mut gates = pipeline();
    assert!(try_binary(&mut gates, 1).is_ok());
    assert!(try_binary(&mut gates, 2).is_ok());
    assert_eq!(try_binary(&mut gates, 3), Err(GateCheck::RateLimits));
    // The perp arm sees the same halt.
    assert_eq!(try_perp(&mut gates, 4), Err(GateCheck::Halts));
}

#[test]
fn perp_i3_shared_bucket_no_per_arm_split() {
    let mut gates = pipeline();
    // One order from EACH arm consumes the SAME venue bucket (burst 2):
    // the third order breaches regardless of which arm it arrives on.
    assert!(try_perp(&mut gates, 1).is_ok());
    assert!(try_binary(&mut gates, 2).is_ok());
    assert_eq!(try_perp(&mut gates, 3), Err(GateCheck::RateLimits));
}
