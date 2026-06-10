//! T0.5 property tests: gate pipeline over arbitrary order sequences.
//! PROMPT doctrine: "property tests for logic ('for all order sequences...')".

use fortuna_core::book::{FeeError, FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_gates::{
    CandidateOrder, GateCheck, GateConfig, GateInputs, GatePipeline, HaltScope, Verdict,
};
use proptest::prelude::*;
use std::collections::BTreeSet;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

/// Flat 0 fee model: properties target gate logic, not fee math (fee math
/// has its own suite in fortuna-venues).
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

fn config() -> GateConfig {
    toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 50000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 100
        price_band_cents = 49
        max_cross_cents = 98
        per_market_exposure_cents = 50000
        per_event_exposure_cents = 50000
        require_event_mapping = false

        [per_strategy.s]
        max_exposure_cents = 50000
        max_order_notional_cents = 50000
        min_net_edge_bps = 0

        [rate.v]
        burst = 1000
        sustained_per_min = 60000
        market_burst = 1000
        market_sustained_per_min = 60000
        "#,
    )
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

#[derive(Debug, Clone)]
struct ArbOrder {
    side: bool,
    action: bool,
    price: i64,
    qty: i64,
    fair: i64,
    exposure: i64,
}

fn arb_order() -> impl Strategy<Value = ArbOrder> {
    (
        any::<bool>(),
        any::<bool>(),
        -5i64..110,
        -5i64..150,
        -5i64..110,
        0i64..60_000,
    )
        .prop_map(|(side, action, price, qty, fair, exposure)| ArbOrder {
            side,
            action,
            price,
            qty,
            fair,
            exposure,
        })
}

fn to_candidate(o: &ArbOrder, n: usize) -> CandidateOrder {
    let mut g = IdGen::new(n as u64 + 1);
    CandidateOrder {
        intent_id: IntentId::new(g.next(t0()).unwrap()),
        strategy: StrategyId::new("s").unwrap(),
        venue: VenueId::new("v").unwrap(),
        market: MarketId::new("M").unwrap(),
        side: if o.side { Side::Yes } else { Side::No },
        action: if o.action { Action::Buy } else { Action::Sell },
        limit_price: Cents::new(o.price),
        qty: Contracts::new(o.qty),
        fair_value: Cents::new(o.fair),
        client_order_id: ClientOrderId::new(format!("p-{n}")).unwrap(),
    }
}

proptest! {
    /// For all order sequences: audit records are complete, ordered, and
    /// consistent with the verdict; approved orders satisfy the binding
    /// limits; nothing panics.
    #[test]
    fn prop_audit_and_limits_hold_for_all_sequences(orders in proptest::collection::vec(arb_order(), 1..40)) {
        let mut p = GatePipeline::new(config()).unwrap();
        let market = MarketId::new("M").unwrap();
        let book = book(&market);
        let fees = ZeroFees;
        let recent = BTreeSet::new();

        for (n, o) in orders.iter().enumerate() {
            let inputs = GateInputs {
                now: t0(),
                open_exposure_cents: Cents::new(o.exposure),
                market_exposure_cents: Cents::new(o.exposure.min(50_000)),
                strategy_exposure_cents: Cents::new(o.exposure.min(50_000)),
                event_exposure_cents: Cents::ZERO,
                event_id: None,
                book: Some(&book),
                last_trade_price: None,
                fee_model: &fees,
                category: None,
                own_resting: &[],
                recent_client_order_ids: &recent,
            };
            let candidate = to_candidate(o, n);
            let out = p.evaluate(&candidate, &inputs);

            // Records: non-empty, in pipeline order, Pass everywhere except
            // a final Reject exactly when the verdict is Err.
            prop_assert!(!out.records.is_empty());
            for (i, r) in out.records.iter().enumerate() {
                prop_assert_eq!(r.check.index(), i + 1);
                let is_last = i == out.records.len() - 1;
                match (&out.gated, is_last) {
                    (Err(rej), true) => {
                        prop_assert_eq!(r.verdict, Verdict::Reject);
                        prop_assert_eq!(r.check, rej.check);
                    }
                    _ => prop_assert_eq!(r.verdict, Verdict::Pass),
                }
            }

            if let Ok(g) = &out.gated {
                // Approved orders respect the hard numeric limits.
                prop_assert!(g.qty().raw() >= 1 && g.qty().raw() <= 100);
                prop_assert!((1..=99).contains(&g.limit_price().raw()));
                // Capital bound applies to buys; sells are close-only and
                // add no worst-case exposure (ASSUMPTIONS.md, T0.5).
                if g.action() == Action::Buy {
                    let notional = g.limit_price().checked_mul(g.qty().raw()).unwrap();
                    prop_assert!(notional.checked_add(Cents::new(o.exposure)).unwrap()
                        <= Cents::new(50_000));
                }
                // The gated order is the candidate, unmodified.
                prop_assert_eq!(g.limit_price(), candidate.limit_price);
                prop_assert_eq!(g.qty(), candidate.qty);
                prop_assert_eq!(g.side(), candidate.side);
                prop_assert_eq!(g.action(), candidate.action);
            }
        }
    }

    /// For all sequences: once a halt is set, nothing passes until re-armed.
    #[test]
    fn prop_halted_pipeline_passes_nothing(orders in proptest::collection::vec(arb_order(), 1..20)) {
        let mut p = GatePipeline::new(config()).unwrap();
        p.set_halt(HaltScope::Global, "prop halt");
        let market = MarketId::new("M").unwrap();
        let book = book(&market);
        let fees = ZeroFees;
        let recent = BTreeSet::new();
        for (n, o) in orders.iter().enumerate() {
            let inputs = GateInputs {
                now: t0(),
                open_exposure_cents: Cents::ZERO,
                market_exposure_cents: Cents::ZERO,
                strategy_exposure_cents: Cents::ZERO,
                event_exposure_cents: Cents::ZERO,
                event_id: None,
                book: Some(&book),
                last_trade_price: None,
                fee_model: &fees,
                category: None,
                own_resting: &[],
                recent_client_order_ids: &recent,
            };
            let out = p.evaluate(&to_candidate(o, n), &inputs);
            prop_assert!(out.gated.is_err());
            prop_assert_eq!(out.gated.unwrap_err().check, GateCheck::Halts);
        }
    }

    /// Rate limiting: at a frozen instant, at most `burst` orders pass the
    /// rate check per venue, and a breach halts everything after it.
    #[test]
    fn prop_burst_bound_holds(n_orders in 1usize..30, burst in 1u32..8) {
        let cfg_src = format!(
            r#"
            [global]
            max_total_exposure_cents = 1000000
            max_daily_loss_cents = 50000
            min_order_contracts = 1
            max_order_contracts = 100
            price_band_cents = 49
            max_cross_cents = 98
            per_market_exposure_cents = 1000000
            per_event_exposure_cents = 1000000
            require_event_mapping = false

            [per_strategy.s]
            max_exposure_cents = 1000000
            max_order_notional_cents = 1000000
            min_net_edge_bps = 0

            [rate.v]
            burst = {burst}
            sustained_per_min = 60
            market_burst = {burst}
            market_sustained_per_min = 60
            "#
        );
        let cfg: GateConfig = toml::from_str(&cfg_src).unwrap();
        let mut p = GatePipeline::new(cfg).unwrap();
        let market = MarketId::new("M").unwrap();
        let book = book(&market);
        let fees = ZeroFees;
        let recent = BTreeSet::new();
        let mut approved = 0usize;
        for n in 0..n_orders {
            let o = ArbOrder { side: true, action: true, price: 50, qty: 1, fair: 60, exposure: 0 };
            let inputs = GateInputs {
                now: t0(), // frozen: no refill
                open_exposure_cents: Cents::ZERO,
                market_exposure_cents: Cents::ZERO,
                strategy_exposure_cents: Cents::ZERO,
                event_exposure_cents: Cents::ZERO,
                event_id: None,
                book: Some(&book),
                last_trade_price: None,
                fee_model: &fees,
                category: None,
                own_resting: &[],
                recent_client_order_ids: &recent,
            };
            if p.evaluate(&to_candidate(&o, n), &inputs).gated.is_ok() {
                approved += 1;
            }
        }
        prop_assert!(approved <= burst as usize);
        if n_orders > burst as usize {
            // The breach halted the venue.
            prop_assert!(p.halts().venue_halted("v").is_some());
        }
    }
}
