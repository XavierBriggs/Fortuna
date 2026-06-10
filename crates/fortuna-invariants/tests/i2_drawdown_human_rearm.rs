//! I2: drawdown halts with human re-arm
//!
//! Property to encode: for all DST sequences breaching a drawdown threshold, a halt flag is set, no further orders pass gates, and no code path clears the flag without the CLI re-arm action
//!
//! Implemented in T0.7 per tests/README.md (stubs are implemented, never
//! weakened, by their owning BUILD_PLAN task). The DrawdownMonitor computes
//! breaches; the gate pipeline's halt flag is the lock. Encoded here:
//! breach => global halt => no order passes any gate; neither time passing,
//! equity recovering, day rolling, nor a config reload clears it; the ONLY
//! clear path is the operator re-arm. Randomized equity paths (property)
//! confirm: whenever a breach verdict fires, the halted pipeline passes
//! nothing until re-armed.

use fortuna_core::book::{FeeError, FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_gates::{CandidateOrder, GateCheck, GateConfig, GateInputs, GatePipeline, HaltScope};
use fortuna_state::{DrawdownMonitor, DrawdownVerdict};
use proptest::prelude::*;
use std::collections::BTreeSet;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T00:00:00.000Z").unwrap()
}

fn at_ms(offset: i64) -> UtcTimestamp {
    t0().checked_add_millis(offset).unwrap()
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

fn pipeline() -> GatePipeline {
    let cfg: GateConfig = toml::from_str(
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

        [per_strategy.i2]
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
    .unwrap();
    GatePipeline::new(cfg).unwrap()
}

fn book(market: &MarketId, as_of: UtcTimestamp) -> OrderBook {
    OrderBook {
        market: market.clone(),
        as_of,
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

fn candidate(n: u64) -> CandidateOrder {
    let mut g = IdGen::new(n + 1);
    CandidateOrder {
        intent_id: IntentId::new(g.next(t0()).unwrap()),
        strategy: StrategyId::new("i2").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        market: MarketId::new("M").unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(50),
        qty: Contracts::new(1),
        fair_value: Cents::new(60),
        client_order_id: ClientOrderId::new(format!("i2-{n}")).unwrap(),
    }
}

fn try_order(p: &mut GatePipeline, n: u64, now: UtcTimestamp) -> Result<(), GateCheck> {
    let market = MarketId::new("M").unwrap();
    let b = book(&market, now);
    let fees = ZeroFees;
    let recent = BTreeSet::new();
    let inputs = GateInputs {
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
    match p.evaluate(&candidate(n), &inputs).gated {
        Ok(_) => Ok(()),
        Err(rej) => Err(rej.check),
    }
}

/// The original stub, implemented: the full breach -> halt -> no-clear ->
/// re-arm lifecycle.
#[test]
fn i2_drawdown_human_rearm() {
    let max_daily_loss = Cents::new(50_000);
    let mut monitor = DrawdownMonitor::new(max_daily_loss);
    let mut gates = pipeline();

    // Healthy morning: equity at baseline, orders flow.
    assert_eq!(
        monitor.check(at_ms(0), Cents::new(1_000_000)).unwrap(),
        DrawdownVerdict::Ok
    );
    assert!(try_order(&mut gates, 1, at_ms(0)).is_ok());

    // The day goes badly: equity drops by exactly the limit. Breach.
    let verdict = monitor
        .check(at_ms(3_600_000), Cents::new(950_000))
        .unwrap();
    let DrawdownVerdict::Breach { loss, limit } = verdict else {
        panic!("expected breach at exact limit, got {verdict:?}");
    };
    assert_eq!(loss, max_daily_loss);
    assert_eq!(limit, max_daily_loss);

    // The breach sets the halt flag (this wiring IS the invariant: breach
    // verdicts must reach the gates as a halt).
    gates.set_halt(HaltScope::Global, format!("drawdown breach: {loss}"));

    // No further orders pass any gate.
    assert_eq!(
        try_order(&mut gates, 2, at_ms(3_700_000)),
        Err(GateCheck::Halts)
    );

    // Equity recovering does NOT clear it (monitor stays sticky AND the
    // flag stays set).
    assert!(matches!(
        monitor
            .check(at_ms(4_000_000), Cents::new(1_100_000))
            .unwrap(),
        DrawdownVerdict::Breach { .. }
    ));
    assert_eq!(
        try_order(&mut gates, 3, at_ms(4_000_000)),
        Err(GateCheck::Halts)
    );

    // Time passing — even into the NEXT UTC DAY — does not clear the flag:
    // the monitor's baseline resets, but the halt is not the monitor's to
    // clear. No automatic resumption (I2).
    let next_day = at_ms(86_400_000 + 1);
    assert!(matches!(
        monitor.check(next_day, Cents::new(1_100_000)).unwrap(),
        DrawdownVerdict::Ok
    ));
    assert_eq!(try_order(&mut gates, 4, next_day), Err(GateCheck::Halts));

    // A config reload does not clear it either (pinned in fortuna-gates
    // tests too; re-asserted here because it is an I2 bypass risk).
    let cfg: GateConfig = toml::from_str(
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

        [per_strategy.i2]
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
    .unwrap();
    gates.reload_config(cfg).unwrap();
    assert_eq!(try_order(&mut gates, 5, next_day), Err(GateCheck::Halts));

    // THE ONLY CLEAR PATH: the operator re-arm (CLI-wired in T0.9).
    gates.rearm(HaltScope::Global).unwrap();
    assert!(try_order(&mut gates, 6, next_day).is_ok());
}

proptest! {
    /// For all same-day equity paths: the first observation at or past the
    /// loss limit yields Breach, every breach keeps the halted pipeline
    /// passing nothing, and only re-arm restores flow.
    #[test]
    fn i2_prop_breach_always_locks_until_rearm(
        start_equity in 100_000i64..2_000_000,
        deltas in proptest::collection::vec(-60_000i64..30_000, 1..30),
    ) {
        let limit = Cents::new(50_000);
        let mut monitor = DrawdownMonitor::new(limit);
        let mut gates = pipeline();
        let mut equity = start_equity;
        let mut halted = false;
        monitor.check(at_ms(0), Cents::new(equity)).unwrap();

        for (i, d) in deltas.iter().enumerate() {
            equity += d;
            let now = at_ms(1_000 * (i as i64 + 1));
            let verdict = monitor.check(now, Cents::new(equity)).unwrap();
            let loss = start_equity - equity;
            match verdict {
                DrawdownVerdict::Breach { .. } => {
                    if !halted {
                        gates.set_halt(HaltScope::Global, "drawdown breach");
                        halted = true;
                    }
                }
                DrawdownVerdict::Ok => {
                    // The monitor may only say Ok if the loss never reached
                    // the limit so far today (stickiness).
                    prop_assert!(!halted, "monitor said Ok after a breach today");
                    prop_assert!(loss < 50_000, "loss {loss} reached limit without Breach");
                }
            }
            let outcome = try_order(&mut gates, 1_000 + i as u64, now);
            if halted {
                prop_assert_eq!(outcome, Err(GateCheck::Halts));
            } else {
                prop_assert!(outcome.is_ok());
            }
        }
        if halted {
            gates.rearm(HaltScope::Global).unwrap();
            prop_assert!(try_order(&mut gates, 9_999, at_ms(999_000_000)).is_ok());
        }
    }
}
