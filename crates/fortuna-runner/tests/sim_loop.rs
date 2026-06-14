//! T0.10 tests: mech_structural through the full composed Sim loop.
//! Phase 0 exit criteria made executable: the arb is found, sized from the
//! envelope, gated, grouped, filled, settled at pair value, and the whole
//! run is byte-deterministic under its seed.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_gates::{GateConfig, HaltScope};
use fortuna_runner::mech_structural::{MechStructural, MechStructuralConfig};
use fortuna_runner::{
    CoreHandle, LatencyStat, MemoryAuditSink, RunnerConfig, SimRunner, Stage, Strategy,
    StrategyKind, StrategyMetrics,
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
        faults: Some(FaultConfig::none(seed)),
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
    let report = futures::executor::block_on(r.report()).unwrap();
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
    let report = futures::executor::block_on(r.report()).unwrap();
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

// ---- Section 8 telemetry surface ----

#[test]
fn section_8_metric_surface_is_present() {
    let mut r = SimRunner::new(
        runner_config(99),
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_arb_books(&r);
    futures::executor::block_on(r.tick()).unwrap();

    let names: std::collections::BTreeSet<&'static str> =
        r.metrics_export().iter().map(|s| s.name).collect();
    for required in [
        "fortuna_ticks_total",
        "fortuna_fill_latency_ms_count",
        "fortuna_fill_latency_ms_p99",
        "fortuna_order_ack_latency_ms_bucket",
        "fortuna_venue_api_errors_total",
        "fortuna_settlement_voids_total",
        "fortuna_settlement_reversals_total",
        "fortuna_capital_in_limbo_cents",
        "fortuna_envelope_utilization_bps",
        "fortuna_cognition_cost_cents_total",
        "fortuna_mind_spend_today_cents",
        "fortuna_budget_breaches_total",
        "fortuna_cognition_failures_total",
    ] {
        assert!(names.contains(required), "missing metric: {required}");
    }
    // Envelope utilization carries the strategy label.
    assert!(r
        .metrics_export()
        .iter()
        .any(|s| s.name == "fortuna_envelope_utilization_bps"
            && s.labels.iter().any(|(k, _)| k == "strategy")));

    // Section 8 "gate rejection counts by reason": provoke a rejection
    // (contract floor above the sized quantity) and find it attributed
    // to its check.
    let mut cfg = runner_config(98);
    cfg.gate_config = toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 800000
        max_daily_loss_cents = 50000
        min_order_contracts = 500
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
    .unwrap();
    let mut r2 = SimRunner::new(
        cfg,
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_arb_books(&r2);
    let report = futures::executor::block_on(r2.tick()).unwrap();
    assert!(report.gate_rejections >= 1, "the floor must reject");
    let by_check: Vec<_> = r2
        .metrics_export()
        .into_iter()
        .filter(|s| s.name == "fortuna_gate_rejections_by_check_total")
        .collect();
    assert!(
        by_check
            .iter()
            .any(|s| s.value >= 1 && s.labels.iter().any(|(k, _)| k == "check")),
        "rejections attributed by check: {by_check:?}"
    );
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
    let report2 = futures::executor::block_on(r.report()).unwrap();
    assert_eq!(report2.realized_pnl, Cents::ZERO);
}

// ---- order/fill latency metrics (spec Section 8: "order/fill latency") ----

#[test]
fn fill_latency_is_measured_from_submit_to_execution() {
    // ack_delay on every order: placement is acknowledged but executes on
    // the NEXT tick — with 500ms between ticks, submit->fill latency is a
    // real, nonzero, deterministic number even in Sim.
    let mut cfg = runner_config(77);
    cfg.faults = Some(FaultConfig {
        ack_delay_pm: 1_000,
        ..FaultConfig::none(77)
    });
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

    // Percentiles: conservative upper-edge estimates from fixed buckets.
    // Every fill here took ~500ms, so p50/p90/p99 all land on the 500ms
    // bucket edge — and the estimate must never UNDERSTATE latency.
    for q in [0.50, 0.90, 0.95, 0.99] {
        let est = c.fill_latency.quantile_ms(q).expect("quantile defined");
        assert!(
            est >= 500,
            "p{} estimate {est} understates the ~500ms truth",
            (q * 100.0) as u32
        );
        assert!(
            est <= c.fill_latency.max_ms.max(1_000),
            "p{q} wildly high: {est}"
        );
    }
    assert!(c.fill_latency.quantile_ms(0.5).is_some());
    assert_eq!(LatencyStat::default().quantile_ms(0.5), None, "empty: None");

    // Prometheus histogram export: cumulative buckets with le labels and
    // a +Inf bucket equal to the count; direct p90/95/99 gauges for the
    // dashboard.
    let buckets: Vec<_> = samples
        .iter()
        .filter(|s| s.name == "fortuna_fill_latency_ms_bucket")
        .collect();
    assert!(!buckets.is_empty(), "bucket series exported");
    let inf = buckets
        .iter()
        .find(|s| s.labels.iter().any(|(k, v)| k == "le" && v == "+Inf"))
        .expect("+Inf bucket");
    assert_eq!(inf.value as u64, c.fill_latency.count);
    let mut prev = 0i64;
    for b in &buckets {
        assert!(b.value >= prev, "buckets are cumulative");
        prev = b.value;
    }
    for name in [
        "fortuna_fill_latency_ms_p90",
        "fortuna_fill_latency_ms_p95",
        "fortuna_fill_latency_ms_p99",
        "fortuna_order_ack_latency_ms_p99",
    ] {
        assert!(
            samples.iter().any(|s| s.name == name),
            "{name} exported for the boards"
        );
    }

    // Deterministic: the same seed reproduces the same latency stats.
    let mut cfg2 = runner_config(77);
    cfg2.faults = Some(FaultConfig {
        ack_delay_pm: 1_000,
        ..FaultConfig::none(77)
    });
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

// ---- hot-path wall-time instrumentation (operator question: the mech
// ---- strategies race the market; what does OUR slice of the path cost?)

#[test]
fn tick_wall_time_is_reported() {
    use std::time::Instant;

    // Busy world: arb scan + 3-leg size/gate/submit/fill every tick.
    let mut r = SimRunner::new(
        runner_config(7),
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_arb_books(&r);
    // Warm + first trade tick.
    futures::executor::block_on(r.tick()).unwrap();

    // Steady-state ticks (books refreshed, no new arb after capture).
    let n = 2_000;
    let started = Instant::now();
    for _ in 0..n {
        set_fair_books(&r);
        r.clock.advance_millis(100).unwrap();
        futures::executor::block_on(r.tick()).unwrap();
    }
    let elapsed = started.elapsed();
    let per_tick_us = elapsed.as_micros() as f64 / n as f64;
    println!("[tick-bench] steady-state: {per_tick_us:.0}us/tick over {n} ticks");

    // Decision-heavy ticks: fresh runner per measurement of the full
    // scan->size->gate->submit->fill tick.
    let m = 200;
    let mut total_us = 0u128;
    for i in 0..m {
        let mut r = SimRunner::new(
            runner_config(1_000 + i),
            vec![strategy()],
            Box::new(MemoryAuditSink::default()),
            t0(),
        )
        .unwrap();
        set_arb_books(&r);
        let t = Instant::now();
        futures::executor::block_on(r.tick()).unwrap();
        total_us += t.elapsed().as_micros();
    }
    println!(
        "[tick-bench] full trade tick (scan+gates+3 submits+fills): {:.0}us avg over {m} runs",
        total_us as f64 / m as f64
    );

    // Sanity bound only — generous enough to never flake in CI; the
    // numbers above are the deliverable.
    assert!(per_tick_us < 50_000.0, "steady tick {per_tick_us}us > 50ms");
}

// ---- gate finding: the mid-group all-or-nothing abort path ----

/// Proposes one 3-leg group where the THIRD leg's price violates the
/// band: the phase-split must submit NOTHING and release every
/// reservation.
struct DoomedGroup {
    proposed: bool,
    metrics: StrategyMetrics,
}

#[async_trait::async_trait]
impl Strategy for DoomedGroup {
    fn id(&self) -> StrategyId {
        StrategyId::new("mech_structural").unwrap()
    }
    fn kind(&self) -> StrategyKind {
        StrategyKind::Mechanical
    }
    fn stage(&self) -> Stage {
        Stage::Sim
    }
    async fn on_event(
        &mut self,
        ev: &fortuna_core::bus::BusEvent,
        _core: &CoreHandle<'_>,
    ) -> Result<Vec<fortuna_runner::Proposal>, fortuna_runner::RunnerError> {
        self.metrics.events_seen += 1;
        if self.proposed {
            return Ok(Vec::new());
        }
        let fortuna_core::bus::EventPayload::BookSnapshot { book, .. } = &ev.payload else {
            return Ok(Vec::new());
        };
        if book.market != mkt("BKT-LO") {
            return Ok(Vec::new());
        }
        self.proposed = true;
        let leg = |m: &str, price: i64| fortuna_runner::ProposedLeg {
            market: mkt(m),
            side: Side::Yes,
            action: fortuna_core::market::Action::Buy,
            limit_price: Cents::new(price),
            fair_value: Cents::new((price + 6).min(99)),
            calibrated_p: None,
        };
        Ok(vec![fortuna_runner::Proposal {
            legs: vec![
                leg("BKT-LO", 25),
                leg("BKT-MID", 28),
                // Band breach: book ~25/30, band 45 -> 99 is way outside.
                leg("BKT-HI", 99),
            ],
            group_policy: Some(fortuna_exec::GroupPolicy {
                max_unhedged_notional: Cents::new(5_000),
                max_leg_open_ms: 60_000,
                value_per_set: Cents::new(100),
                min_completion_edge_bps: 100,
            }),
            urgency: fortuna_runner::Urgency::Taker,
            manifest_hash: None,
            thesis: "doomed group".into(),
        }])
    }
    fn metrics(&self) -> StrategyMetrics {
        self.metrics
    }
}

#[test]
fn a_mid_group_gate_rejection_submits_nothing_and_releases_reservations() {
    let mut r = SimRunner::new(
        runner_config(83),
        vec![Box::new(DoomedGroup {
            proposed: false,
            metrics: StrategyMetrics::default(),
        })],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_arb_books(&r);

    let report = futures::executor::block_on(r.tick()).unwrap();
    assert_eq!(report.proposals, 1, "the doomed group proposed");
    assert!(report.gate_rejections >= 1, "the third leg must reject");
    assert_eq!(
        report.orders_submitted, 0,
        "all-or-nothing: NOTHING submits when any leg rejects pre-network"
    );
    // Every reservation released: the strategy's reserved exposure is 0.
    let reserved = r
        .metrics_export()
        .into_iter()
        .find(|s| {
            s.name == "fortuna_reserved_exposure_cents"
                && s.labels
                    .iter()
                    .any(|(k, v)| k == "strategy" && v == "mech_structural")
        })
        .map(|s| s.value)
        .unwrap_or(-1);
    assert_eq!(reserved, 0, "reservations from passed legs were released");
}
