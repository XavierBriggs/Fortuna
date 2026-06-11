//! T0.10 tests: mech_structural through the full composed Sim loop.
//! Phase 0 exit criteria made executable: the arb is found, sized from the
//! envelope, gated, grouped, filled, settled at pair value, and the whole
//! run is byte-deterministic under its seed.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_gates::{GateConfig, HaltScope};
use fortuna_runner::mech_structural::{MechStructural, MechStructuralConfig};
use fortuna_runner::{
    MemoryAuditSink, RunnerConfig, SimRunner, Stage, Strategy, StrategyKind, StrategyMetrics,
};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, PriceLevel, SettlementMeta};
use std::collections::BTreeMap;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
            maker_coeff = "0.0175"
        "#,
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

fn runner_config(seed: u64) -> RunnerConfig {
    RunnerConfig {
        seed,
        gate_config: gate_config(),
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("mech_structural".to_string(), Cents::new(300_000))]),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![
            bracket_market("BKT-LO"),
            bracket_market("BKT-MID"),
            bracket_market("BKT-HI"),
        ],
        starting_cash: Cents::new(1_000_000),
        faults: FaultConfig::none(seed),
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

fn strategy() -> Box<dyn Strategy> {
    Box::new(
        MechStructural::new(MechStructuralConfig {
            bracket_sets: vec![vec![mkt("BKT-LO"), mkt("BKT-MID"), mkt("BKT-HI")]],
            min_edge_cents_per_set: 2,
            max_unhedged_notional: Cents::new(5_000),
            max_leg_open_ms: 60_000,
            min_completion_edge_bps: 100,
        })
        .unwrap(),
    )
}

/// Books where the YES basket costs 25+28+30 = 83c + ~5c fees: ~12c locked
/// per set, wide enough that each leg clears the 100bps gate floor at the
/// sized quantity.
fn set_arb_books(runner: &SimRunner) {
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: fortuna_core::market::Contracts::new(q),
    };
    runner
        .venue()
        .set_book(&mkt("BKT-LO"), vec![lvl(20, 80)], vec![lvl(25, 80)])
        .unwrap();
    runner
        .venue()
        .set_book(&mkt("BKT-MID"), vec![lvl(23, 80)], vec![lvl(28, 80)])
        .unwrap();
    runner
        .venue()
        .set_book(&mkt("BKT-HI"), vec![lvl(25, 80)], vec![lvl(30, 80)])
        .unwrap();
}

/// Books that sum above 100: no arb anywhere.
fn set_fair_books(runner: &SimRunner) {
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: fortuna_core::market::Contracts::new(q),
    };
    for (m, bid, ask) in [("BKT-LO", 30, 35), ("BKT-MID", 30, 35), ("BKT-HI", 30, 35)] {
        runner
            .venue()
            .set_book(&mkt(m), vec![lvl(bid, 80)], vec![lvl(ask, 80)])
            .unwrap();
    }
}

// ---- the full loop ----

#[test]
fn mech_structural_captures_a_bracket_arb_end_to_end() {
    let mut r = SimRunner::new(
        runner_config(42),
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_arb_books(&r);

    let report = futures::executor::block_on(r.tick()).unwrap();
    assert!(report.proposals >= 1, "the scan must fire");
    assert_eq!(report.orders_submitted, 3, "three legs submitted");
    assert!(report.fills_applied >= 3, "crossing asks fill immediately");

    // The position book holds the YES basket.
    for m in ["BKT-LO", "BKT-MID", "BKT-HI"] {
        let p = r.positions().position(&mkt(m)).unwrap();
        assert!(p.yes.qty > 0, "{m} leg held");
        assert_eq!(
            p.yes.qty,
            r.positions().position(&mkt("BKT-LO")).unwrap().yes.qty
        );
    }
    let sets = r.positions().position(&mkt("BKT-LO")).unwrap().yes.qty;

    // Settle: exactly one bracket pays (that is what a bracket IS).
    r.venue().settle_market(&mkt("BKT-MID"), Side::Yes).unwrap();
    r.venue().settle_market(&mkt("BKT-LO"), Side::No).unwrap();
    r.venue().settle_market(&mkt("BKT-HI"), Side::No).unwrap();
    r.apply_settlement(&mkt("BKT-MID"), Side::Yes, Cents::new(100))
        .unwrap();
    r.apply_settlement(&mkt("BKT-LO"), Side::No, Cents::ZERO)
        .unwrap();
    r.apply_settlement(&mkt("BKT-HI"), Side::No, Cents::ZERO)
        .unwrap();

    // The arb realizes: 100c/set minus ~93c cost minus fees > 0, and the
    // venue cash agrees with our books.
    let report = r.report().unwrap();
    let pnl_net_of_fees = report.realized_pnl.checked_sub(report.fees_paid).unwrap();
    assert!(
        pnl_net_of_fees > Cents::ZERO,
        "arb must profit net of fees: pnl {} fees {}",
        report.realized_pnl,
        report.fees_paid
    );
    // Venue truth: final cash = start - cost - fees + 100/set.
    let expected_min = Cents::new(1_000_000)
        .checked_add(Cents::new(sets)) // at least +1c/set after fees (edge >= 2c, fees pre-counted)
        .unwrap();
    assert!(
        report.final_cash >= expected_min,
        "final cash {} must exceed start by the locked edge",
        report.final_cash
    );
}

#[test]
fn no_arb_means_no_orders() {
    let mut r = SimRunner::new(
        runner_config(42),
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_fair_books(&r);
    let report = futures::executor::block_on(r.tick()).unwrap();
    assert_eq!(report.proposals, 0);
    assert_eq!(report.orders_submitted, 0);
    assert!(r.venue().resting_orders().is_empty());
}

// ---- determinism: the Phase 0 exit's replay criterion ----

fn run_scripted(seed: u64) -> (String, Cents) {
    let mut r = SimRunner::new(
        runner_config(seed),
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_arb_books(&r);
    futures::executor::block_on(r.tick()).unwrap();
    r.clock.advance_millis(1_000).unwrap();
    set_fair_books(&r);
    futures::executor::block_on(r.tick()).unwrap();
    r.venue().settle_market(&mkt("BKT-MID"), Side::Yes).unwrap();
    r.apply_settlement(&mkt("BKT-MID"), Side::Yes, Cents::new(100))
        .unwrap();
    let report = r.report().unwrap();
    (report.recording_jsonl, report.final_cash)
}

#[test]
fn same_seed_same_script_byte_identical_recording() {
    let (rec_a, cash_a) = run_scripted(7);
    let (rec_b, cash_b) = run_scripted(7);
    assert_eq!(rec_a, rec_b, "recordings must be byte-identical");
    assert_eq!(cash_a, cash_b);
    assert!(!rec_a.is_empty());
}

// ---- halt mid-IntentGroup (doctrine scenario) ----

#[test]
fn global_halt_blocks_new_proposals_but_existing_groups_freeze_cleanly() {
    let mut r = SimRunner::new(
        runner_config(42),
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_arb_books(&r);
    let report = futures::executor::block_on(r.tick()).unwrap();
    assert_eq!(report.orders_submitted, 3);

    // Halt lands mid-life (operator or breach).
    r.gates_mut()
        .set_halt(HaltScope::Global, "mid-group halt test");

    // Books re-arb; the scan fires but the gates refuse everything.
    set_arb_books(&r);
    r.clock.advance_millis(500).unwrap();
    let report = futures::executor::block_on(r.tick()).unwrap();
    assert_eq!(report.orders_submitted, 0);
    assert!(report.halted);

    // Re-arm restores flow (I2 path).
    r.gates_mut().rearm(HaltScope::Global).unwrap();
    r.clock.advance_millis(500).unwrap();
    set_fair_books(&r); // no arb: nothing new, but ticks run clean
    let report = futures::executor::block_on(r.tick()).unwrap();
    assert!(!report.halted);
}

// ---- audit failure halts trading (I5 doctrine scenario) ----

#[test]
fn audit_sink_failure_halts_trading() {
    let sink = MemoryAuditSink {
        fail_after: Some(0), // dies on the very first record
        ..Default::default()
    };
    let mut r = SimRunner::new(runner_config(42), vec![strategy()], Box::new(sink), t0()).unwrap();
    set_arb_books(&r);
    let report = futures::executor::block_on(r.tick()).unwrap();
    // The first gate verdict's audit write fails -> global halt -> the
    // remaining legs never submit.
    assert!(report.halted, "audit failure must halt trading");
    assert!(
        report.orders_submitted <= 1,
        "at most the in-flight order survives the halt"
    );
    let halted_reason = r.gates().halts().global_halted().unwrap_or("").to_string();
    assert!(halted_reason.contains("audit"));
    // And the failure is on the bus for replay.
    let jsonl = r.recording().to_jsonl().unwrap();
    assert!(jsonl.contains("audit_failure_halt"));
}

// ---- I7 sliver: stage guard ----

struct LiveStrategy;

#[async_trait::async_trait]
impl Strategy for LiveStrategy {
    fn id(&self) -> fortuna_core::market::StrategyId {
        fortuna_core::market::StrategyId::new("rogue").unwrap()
    }
    fn kind(&self) -> StrategyKind {
        StrategyKind::Mechanical
    }
    fn stage(&self) -> Stage {
        Stage::LiveMin
    }
    async fn on_event(
        &mut self,
        _ev: &fortuna_core::bus::BusEvent,
        _core: &fortuna_runner::CoreHandle<'_>,
    ) -> Result<Vec<fortuna_runner::Proposal>, fortuna_runner::RunnerError> {
        Ok(Vec::new())
    }
    fn metrics(&self) -> StrategyMetrics {
        StrategyMetrics::default()
    }
}

#[test]
fn sim_runner_refuses_non_sim_staged_strategies() {
    let result = SimRunner::new(
        runner_config(42),
        vec![Box::new(LiveStrategy)],
        Box::new(MemoryAuditSink::default()),
        t0(),
    );
    assert!(matches!(
        result,
        Err(fortuna_runner::RunnerError::StageViolation { .. })
    ));
}

// ---- gate coverage smoke: the edge floor binds in the composed loop ----

#[test]
fn thin_edge_below_gate_floor_is_rejected_by_the_gates() {
    // Books sum to 99c: ~1c gross, negative after fees; even if the
    // strategy proposed it (min edge 0), the GATES would refuse. Belt and
    // braces: set strategy min edge 0 to prove the gates bind.
    let mut cfg = runner_config(42);
    cfg.gate_config = gate_config();
    let strat = Box::new(
        MechStructural::new(MechStructuralConfig {
            bracket_sets: vec![vec![mkt("BKT-LO"), mkt("BKT-MID"), mkt("BKT-HI")]],
            min_edge_cents_per_set: -10, // strategy floor OFF: gates must catch
            max_unhedged_notional: Cents::new(5_000),
            max_leg_open_ms: 60_000,
            min_completion_edge_bps: 100,
        })
        .unwrap(),
    );
    let mut r =
        SimRunner::new(cfg, vec![strat], Box::new(MemoryAuditSink::default()), t0()).unwrap();
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: fortuna_core::market::Contracts::new(q),
    };
    for (m, ask) in [("BKT-LO", 33), ("BKT-MID", 33), ("BKT-HI", 33)] {
        r.venue()
            .set_book(&mkt(m), vec![lvl(28, 80)], vec![lvl(ask, 80)])
            .unwrap();
    }
    let report = futures::executor::block_on(r.tick()).unwrap();
    // 99c + fees: each leg's net edge lands below the 100bps floor.
    assert_eq!(report.orders_submitted, 0);
    assert!(report.gate_rejections >= 1 || report.proposals == 0);
    let report2 = r.report().unwrap();
    assert_eq!(report2.realized_pnl, Cents::ZERO);
}

// ---- order/fill latency metrics (spec Section 8: "order/fill latency") ----

#[test]
fn fill_latency_is_measured_from_submit_to_execution() {
    // ack_delay on every order: placement is acknowledged but executes on
    // the NEXT tick — with 500ms between ticks, submit->fill latency is a
    // real, nonzero, deterministic number even in Sim.
    let mut cfg = runner_config(77);
    cfg.faults.ack_delay_pm = 1_000;
    let mut r = SimRunner::new(
        cfg,
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_arb_books(&r);

    let first = futures::executor::block_on(r.tick()).unwrap();
    assert!(first.orders_submitted >= 1, "orders placed");
    assert_eq!(first.fills_applied, 0, "delayed: nothing executes yet");

    r.clock.advance_millis(500).unwrap();
    r.venue().tick().unwrap(); // the venue's world advances: delayed orders execute
    let second = futures::executor::block_on(r.tick()).unwrap();
    assert!(second.fills_applied >= 1, "the delayed orders execute");

    let c = r.counters();
    assert!(c.fill_latency.count >= 1, "fill latency observed");
    assert!(
        c.fill_latency.max_ms >= 500,
        "submit->execution spanned the tick gap: {:?}",
        c.fill_latency
    );
    assert!(c.fill_latency.sum_ms >= c.fill_latency.max_ms);
    // Ack latency exists as a surface; under SimClock the await is
    // instantaneous (0ms is the TRUE value here — it becomes real wall
    // time in paper/live where the venue call actually waits).
    assert!(c.ack_latency.count >= 1);

    // The ops scrape carries it.
    let samples = r.metrics_export();
    let max = samples
        .iter()
        .find(|s| s.name == "fortuna_fill_latency_ms_max")
        .expect("fill latency exported");
    assert!(max.value >= 500, "exported max {}", max.value);
    assert!(samples
        .iter()
        .any(|s| s.name == "fortuna_fill_latency_ms_count"));
    assert!(samples
        .iter()
        .any(|s| s.name == "fortuna_order_ack_latency_ms_count"));

    // Deterministic: the same seed reproduces the same latency stats.
    let mut cfg2 = runner_config(77);
    cfg2.faults.ack_delay_pm = 1_000;
    let mut r2 = SimRunner::new(
        cfg2,
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_arb_books(&r2);
    futures::executor::block_on(r2.tick()).unwrap();
    r2.clock.advance_millis(500).unwrap();
    r2.venue().tick().unwrap();
    futures::executor::block_on(r2.tick()).unwrap();
    let c2 = r2.counters();
    assert_eq!(c.fill_latency.count, c2.fill_latency.count);
    assert_eq!(c.fill_latency.sum_ms, c2.fill_latency.sum_ms);
    assert_eq!(c.fill_latency.max_ms, c2.fill_latency.max_ms);
}
