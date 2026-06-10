//! T1.5 / Phase 1 exit evidence: "dashboards and digest render from sim
//! data". One sim run drives the whole observability chain:
//! runner -> MetricSample export -> MetricsRegistry -> Prometheus text +
//! dashboard boards (served over HTTP) + the daily digest text.

use fortuna_core::book::PriceLevel;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Contracts, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_ops::dashboard::{serve_dashboard, DashboardSnapshot};
use fortuna_ops::digest::{compose_daily_digest, DigestInputs, StrategyDigestRow};
use fortuna_ops::metrics::MetricsRegistry;
use fortuna_runner::mech_structural::{MechStructural, MechStructuralConfig};
use fortuna_runner::{MemoryAuditSink, RunnerConfig, SimRunner};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, SettlementMeta};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap()
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
        volume_contracts: Some(1_000),
        payout_per_contract: Cents::new(100),
    }
}

/// The T0.10 arb world: mech_structural captures a bracket sum < 100c.
fn arb_runner() -> SimRunner {
    let gate_config: fortuna_gates::GateConfig = toml::from_str(
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
    .unwrap();
    let config = RunnerConfig {
        seed: 99,
        gate_config,
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
        faults: FaultConfig::none(99),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 50,
        kelly_fraction: 0.25,
        veto_mind: None,
        veto_strategies: Vec::new(),
    };
    let strategy = MechStructural::new(MechStructuralConfig {
        bracket_sets: vec![vec![mkt("BKT-LO"), mkt("BKT-MID"), mkt("BKT-HI")]],
        min_edge_cents_per_set: 2,
        max_unhedged_notional: Cents::new(5_000),
        max_leg_open_ms: 60_000,
        min_completion_edge_bps: 100,
    })
    .unwrap();
    let runner = SimRunner::new(
        config,
        vec![Box::new(strategy)],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: Contracts::new(q),
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
    runner
}

fn registry_from(runner: &SimRunner) -> MetricsRegistry {
    let mut m = MetricsRegistry::new();
    for sample in runner.metrics_export() {
        let labels: Vec<(&str, &str)> = sample
            .labels
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        if sample.counter {
            m.describe_counter(sample.name, sample.help);
            m.inc_counter(sample.name, &labels, sample.value).unwrap();
        } else {
            m.describe_gauge(sample.name, sample.help);
            m.set_gauge(sample.name, &labels, sample.value);
        }
    }
    m
}

#[tokio::test]
async fn dashboards_and_digest_render_from_sim_data() {
    // --- run the arb through fill + settlement, via the processor ---
    let mut runner = arb_runner();
    runner.tick().await.unwrap();
    runner
        .venue()
        .settle_market(&mkt("BKT-MID"), Side::Yes)
        .unwrap();
    runner
        .venue()
        .settle_market(&mkt("BKT-LO"), Side::No)
        .unwrap();
    runner
        .venue()
        .settle_market(&mkt("BKT-HI"), Side::No)
        .unwrap();
    runner.tick().await.unwrap();

    let counters = runner.counters();
    assert!(counters.fills_applied >= 3, "the arb must have filled");
    assert_eq!(counters.settlement_notices, 3);

    // --- metrics: exposition text carries the Section 8 series ---
    let registry = registry_from(&runner);
    let text = registry.render_prometheus();
    assert!(text.contains("# TYPE fortuna_ticks_total counter"));
    assert!(text.contains("fortuna_ticks_total 2"));
    assert!(text.contains("fortuna_settlement_notices_total 3"));
    assert!(text.contains("fortuna_realized_pnl_cents{strategy=\"mech_structural\"}"));
    assert!(text.contains("fortuna_halt_active 0"));

    // The arb realizes positive PnL and the metric agrees with the books.
    let report = runner.report().unwrap();
    assert!(report.realized_pnl > Cents::ZERO);
    assert!(text.contains(&format!(
        "fortuna_realized_pnl_cents{{strategy=\"mech_structural\"}} {}",
        report.realized_pnl.raw()
    )));

    // --- dashboard: boards render over HTTP from the same run ---
    let snapshot = DashboardSnapshot {
        generated_at: t0().to_iso8601(),
        stage: "sim".to_string(),
        metrics_text: text.clone(),
        boards: runner.boards_json(),
    };
    let state = Arc::new(RwLock::new(snapshot));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_dashboard(listener, Arc::clone(&state)));
    let client = reqwest::Client::new();
    let boards: serde_json::Value = client
        .get(format!("http://{addr}/api/boards"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(boards["stage"], "sim");
    let positions = boards["boards"]["positions"].as_array().unwrap();
    assert_eq!(positions.len(), 3, "all three bracket legs on the board");
    let served_metrics = client
        .get(format!("http://{addr}/metrics"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(
        served_metrics, text,
        "the scrape serves the registry render"
    );
    server.abort();

    // --- digest: composes from the same run ---
    let digest = compose_daily_digest(&DigestInputs {
        date_utc: "2026-06-10".to_string(),
        stage: "sim".to_string(),
        strategies: vec![StrategyDigestRow {
            strategy: "mech_structural".to_string(),
            realized_pnl_cents: report.realized_pnl.raw(),
            fees_cents: report.fees_paid.raw(),
            fills: counters.fills_applied,
            open_exposure_cents: 0,
        }],
        halts_active: 0,
        discrepancies_open: counters.discrepancies,
        settlements_overdue: 0,
        capital_in_limbo_cents: 0,
        veto_decisions: counters.veto_decisions,
        veto_suppressed: counters.veto_suppressed,
    });
    assert!(digest.contains("mech_structural"));
    assert!(digest.contains("stage: sim"));
    assert!(digest.contains("discrepancies open: 0"));
}
