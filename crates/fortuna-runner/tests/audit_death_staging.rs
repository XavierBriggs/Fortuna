//! Regression for the t41-increments gate's MAJOR (2026-06-11): an audit
//! append failure landing MID-STAGING (after legs passed gates, before
//! submission) must abort the whole staged group — the probe that found
//! the defect had three orders placed at the venue AFTER the audit store
//! died, their trail lost ("no audit, no trading", spec line 367 / I5).
//!
//! The pin is a SWEEP, not a magic index: a healthy run establishes how
//! many audit appends precede the first venue contact; then every
//! fail-point strictly before that boundary must yield ZERO submissions.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_gates::GateConfig;
use fortuna_runner::mech_structural::{MechStructural, MechStructuralConfig};
use fortuna_runner::{AuditSink, RunnerConfig, RunnerError, SimRunner, Strategy};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, PriceLevel, SettlementMeta};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-11T12:00:00.000Z").unwrap()
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

fn arb_books<J: fortuna_exec::IntentJournal + Send>(r: &SimRunner<J>) {
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: fortuna_core::market::Contracts::new(q),
    };
    for (m, bid, ask) in [("BKT-LO", 20, 25), ("BKT-MID", 23, 28), ("BKT-HI", 25, 30)] {
        r.venue()
            .set_book(&mkt(m), vec![lvl(bid, 80)], vec![lvl(ask, 80)])
            .unwrap();
    }
}

/// Shared-handle audit sink: the test keeps a window into the records
/// AFTER the runner consumes the box, and controls the fail point.
#[derive(Default)]
struct SharedState {
    records: Vec<(String, Option<String>, serde_json::Value)>,
    fail_after: Option<usize>,
}

#[derive(Clone)]
struct SharedSink(Arc<Mutex<SharedState>>);

impl AuditSink for SharedSink {
    fn append(
        &mut self,
        kind: &str,
        ref_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Result<(), RunnerError> {
        let mut st = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(limit) = st.fail_after {
            if st.records.len() >= limit {
                return Err(RunnerError::AuditFailed {
                    reason: "injected audit store failure (staging sweep)".into(),
                });
            }
        }
        st.records
            .push((kind.to_string(), ref_id.map(str::to_string), payload));
        Ok(())
    }
}

#[test]
fn audit_death_before_any_venue_contact_means_zero_venue_contact() {
    // Healthy pass: find the audit-append boundary of first venue
    // contact (the first record written AFTER submissions begin —
    // anything kind != gate_decision that follows a gate_decision).
    let shared = Arc::new(Mutex::new(SharedState::default()));
    let mut r = futures::executor::block_on(SimRunner::new_with_journal(
        runner_config(42),
        vec![strategy()],
        Box::new(SharedSink(shared.clone())),
        t0(),
        fortuna_exec::MemoryJournal::default(),
    ))
    .unwrap();
    arb_books(&r);
    let healthy = futures::executor::block_on(r.tick()).unwrap();
    assert_eq!(healthy.orders_submitted, 3, "healthy baseline trades");
    let records = {
        let st = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        st.records.clone()
    };
    let first_gate = records
        .iter()
        .position(|(k, _, _)| k == "gate_decision")
        .expect("staging audited gate decisions");
    let staging_boundary = records[first_gate..]
        .iter()
        .position(|(k, _, _)| k != "gate_decision")
        .map(|off| first_gate + off)
        .unwrap_or(records.len());
    assert!(
        staging_boundary > first_gate,
        "baseline must contain the staging trail (records: {})",
        records.len()
    );

    // The sweep: every fail point up to and including the LAST staging
    // append must yield zero submissions — the trail either exists in
    // full before venue contact, or the venue is never contacted.
    for fail_after in 0..staging_boundary {
        let shared = Arc::new(Mutex::new(SharedState {
            fail_after: Some(fail_after),
            ..SharedState::default()
        }));
        let mut r = futures::executor::block_on(SimRunner::new_with_journal(
            runner_config(42),
            vec![strategy()],
            Box::new(SharedSink(shared.clone())),
            t0(),
            fortuna_exec::MemoryJournal::default(),
        ))
        .unwrap();
        arb_books(&r);
        let report = futures::executor::block_on(r.tick()).unwrap();
        assert_eq!(
            report.orders_submitted, 0,
            "audit store died at append #{fail_after} (< staging boundary \
             {staging_boundary}): NOTHING may reach the venue"
        );
        assert!(
            report.halted,
            "audit death must surface as the global halt (fail point {fail_after})"
        );
        // Reservations made while staging MUST be released on the abort
        // (gate finding: the probe verified release; the test now does
        // too) — a leaked reservation permanently locks envelope capital.
        assert_eq!(
            r.reserved_total("mech_structural").raw(),
            0,
            "staged reservations released on the audit-death abort (fail point {fail_after})"
        );
    }
}
