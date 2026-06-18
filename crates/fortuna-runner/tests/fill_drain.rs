//! TDD (A2): drain_applied_fills buffers applied fills with their strategy,
//! and a second drain returns empty (mem::take semantics).
//!
//! Written RED first against the not-yet-implemented `pending_fills` field +
//! `drain_applied_fills` method; will fail until the implementation lands.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_gates::GateConfig;
use fortuna_runner::mech_structural::{MechStructural, MechStructuralConfig};
use fortuna_runner::{MemoryAuditSink, RunnerConfig, SimRunner};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, PriceLevel, SettlementMeta};
use std::collections::BTreeMap;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-18T12:00:00.000Z").unwrap()
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        "formula = \"quadratic\"\neffective_date = \"2026-01-01\"\ntaker_coeff = \"0.07\"\nmaker_coeff = \"0.0175\"\n",
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

fn bracket_market(id: &str) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("sim").unwrap(),
        title: format!("bracket {id}"),
        category: "weather".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "nws".into(),
            resolution_source: "nws".into(),
            expected_lag_hours: 2,
        },
        volume_contracts: None,
        payout_per_contract: Cents::new(100),
    }
}

fn gate_config() -> GateConfig {
    toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 800000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 45
        max_cross_cents = 10
        per_market_exposure_cents = 100000
        per_event_exposure_cents = 150000
        require_event_mapping = false

        [per_strategy.mech_structural]
        max_exposure_cents = 200000
        max_order_notional_cents = 10000
        min_net_edge_bps = 100

        [rate.sim]
        burst = 100
        sustained_per_min = 600
        market_burst = 50
        market_sustained_per_min = 300
        "#,
    )
    .unwrap()
}

fn runner_config() -> RunnerConfig {
    RunnerConfig {
        seed: 42,
        gate_config: gate_config(),
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("mech_structural".to_string(), Cents::new(300_000))]),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![
            bracket_market("FD-LO"),
            bracket_market("FD-MID"),
            bracket_market("FD-HI"),
        ],
        starting_cash: Cents::new(1_000_000),
        faults: Some(FaultConfig::none(42)),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 50,
        kelly_fraction: 0.25,
        veto_mind: None,
        veto_strategies: Vec::new(),
    }
}

fn strategy() -> Box<dyn fortuna_runner::Strategy> {
    Box::new(
        MechStructural::new(MechStructuralConfig {
            bracket_sets: vec![vec![mkt("FD-LO"), mkt("FD-MID"), mkt("FD-HI")]],
            min_edge_cents_per_set: 2,
            max_unhedged_notional: Cents::new(5_000),
            max_leg_open_ms: 60_000,
            min_completion_edge_bps: 100,
        })
        .unwrap(),
    )
}

fn set_arb_books(runner: &SimRunner) {
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: fortuna_core::market::Contracts::new(q),
    };
    runner
        .venue()
        .set_book(&mkt("FD-LO"), vec![lvl(20, 80)], vec![lvl(25, 80)])
        .unwrap();
    runner
        .venue()
        .set_book(&mkt("FD-MID"), vec![lvl(23, 80)], vec![lvl(28, 80)])
        .unwrap();
    runner
        .venue()
        .set_book(&mkt("FD-HI"), vec![lvl(25, 80)], vec![lvl(30, 80)])
        .unwrap();
}

/// After a fill-generating tick, `drain_applied_fills()` returns at least one
/// `(Fill, Some(strategy_id))` pair where the strategy is "mech_structural";
/// a second drain returns empty (mem::take semantics).
#[test]
fn drain_applied_fills_buffers_fills_with_strategy_and_empties_on_second_drain() {
    let mut r = SimRunner::new(
        runner_config(),
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();

    set_arb_books(&r);

    let report = futures::executor::block_on(r.tick()).unwrap();
    // mech_structural crosses the arb: 3 legs → 3 fills.
    assert!(report.fills_applied >= 1, "at least one fill must apply");

    // First drain: fills with correct strategy id.
    let fills = r.drain_applied_fills();
    assert!(
        !fills.is_empty(),
        "drain_applied_fills returns the applied fills"
    );
    for (fill, strat) in &fills {
        assert!(!fill.fill_id.is_empty(), "fill has a non-empty fill_id");
        assert_eq!(
            strat.as_ref().map(|s| s.as_str()),
            Some("mech_structural"),
            "each fill carries the mech_structural strategy id"
        );
    }

    // Second drain: empty (mem::take guarantees one-shot delivery).
    let fills2 = r.drain_applied_fills();
    assert!(fills2.is_empty(), "second drain must return empty");
}
