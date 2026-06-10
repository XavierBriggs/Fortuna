//! Phase 2 EXIT: the FULL decision loop composed in Sim — book event
//! (trigger) -> triage -> context assembly -> StubMind -> comparator ->
//! UNSIZED proposal -> sizing -> gates -> execution -> fill -> position.
//!
//! Doctrine under test:
//! - The synthesis strategy is a Strategy like any other: its proposals
//!   ride the SAME tick path (sizing, gates, audit) as the mechanical
//!   strategies (I1). It emits UNSIZED legs only (I6).
//! - Cognition failure (provider error, schema-invalid, refusal, budget
//!   exhaustion) DEGRADES: zero proposals, a counted failure, the loop
//!   keeps running. It never panics, never halts mechanical trading,
//!   never produces a partial decision.
//! - Declined-triage shadow runs produce beliefs (scored like any other)
//!   and NEVER produce proposals.
//!
//! Written BEFORE src/synthesis.rs per the repository TDD doctrine.

use fortuna_cognition::cycle::{ComparatorConfig, EdgeView, TriageDecision};
use fortuna_cognition::events::{EdgeTier, MappingType};
use fortuna_cognition::mind::{Mind, MindError, MindOutput, StubMind};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_runner::synthesis::{SynthesisConfig, SynthesisStrategy};
use fortuna_runner::{MemoryAuditSink, RunnerConfig, SimRunner};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, PriceLevel, SettlementMeta};
use std::collections::BTreeMap;
use std::sync::Arc;

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

fn market(id: &str) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("sim").unwrap(),
        title: format!("market {id}"),
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

fn runner_config(seed: u64) -> RunnerConfig {
    let gate_config = toml::from_str(
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

        [per_strategy.synth_sim]
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
    RunnerConfig {
        seed,
        gate_config,
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("synth_sim".to_string(), Cents::new(300_000))]),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![market("KX-A")],
        starting_cash: Cents::new(1_000_000),
        faults: FaultConfig::none(seed),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 50,
        veto_mind: None,
        veto_strategies: Vec::new(),
    }
}

fn belief_output(event: &str, p: f64) -> MindOutput {
    serde_json::from_value(serde_json::json!({
        "beliefs": [{
            "event_id": event,
            "p": p,
            "p_raw": p,
            "horizon": "2026-06-20T18:00:00.000Z",
            "evidence": [{"source": "stub", "ref": "sig-1"}]
        }],
        "proposals": [],
        "journal": null
    }))
    .unwrap()
}

fn synthesis_config() -> SynthesisConfig {
    SynthesisConfig {
        id: StrategyId::new("synth_sim").unwrap(),
        edges: vec![EdgeView {
            market: "KX-A".to_string(),
            event_id: "evt-1".to_string(),
            mapping: MappingType::Direct,
            tier: EdgeTier::Confirmed,
        }],
        comparator: ComparatorConfig {
            min_edge_cents: 5,
            required_tier: EdgeTier::Proposed,
        },
        triage: TriageDecision::AlwaysAccept,
        shadow_quota: 1,
    }
}

fn set_book(r: &SimRunner, bid: i64, ask: i64) {
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: fortuna_core::market::Contracts::new(q),
    };
    r.venue()
        .set_book(&mkt("KX-A"), vec![lvl(bid, 80)], vec![lvl(ask, 80)])
        .unwrap();
}

// ------------------------------------------------------------- happy path

#[test]
fn synthesis_belief_trades_through_the_full_loop() {
    // StubMind believes p = 0.70 for evt-1; the book asks 60c. The
    // comparator finds a 10c edge; the harness sizes, gates, and fills.
    let mind: Arc<dyn Mind> = Arc::new(StubMind::scripted(vec![belief_output("evt-1", 0.70)]));
    let strategy = SynthesisStrategy::new(synthesis_config(), mind);
    let mut r = SimRunner::new(
        runner_config(11),
        vec![Box::new(strategy)],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_book(&r, 58, 60);

    let report = futures::executor::block_on(r.tick()).unwrap();
    assert!(report.proposals >= 1, "the decision cycle must propose");
    assert!(report.orders_submitted >= 1, "sized and gated");
    assert!(report.fills_applied >= 1, "limit 60 crosses ask 60");

    let pos = r.positions().position(&mkt("KX-A")).unwrap();
    assert!(pos.yes.qty > 0, "YES position opened (fair 70 > ask 60)");
}

// ------------------------------------------------------- cognition failure

/// A mind that always fails, with a scripted error kind.
struct FailingMind {
    error: fn() -> MindError,
}

#[async_trait::async_trait]
impl Mind for FailingMind {
    fn id(&self) -> &str {
        "failing-mind"
    }
    async fn decide(
        &self,
        _ctx: &fortuna_cognition::context::AssembledContext,
    ) -> Result<MindOutput, MindError> {
        Err((self.error)())
    }
}

#[test]
fn cognition_failure_degrades_to_zero_proposals_never_a_crash() {
    for error in [
        (|| MindError::Provider {
            reason: "529 overloaded".to_string(),
        }) as fn() -> MindError,
        || MindError::SchemaInvalid {
            reason: "model emitted prose".to_string(),
        },
        || MindError::Refused {
            explanation: "declined".to_string(),
        },
        || MindError::BudgetExhausted {
            scope: "day",
            spent_cents: 500,
            cap_cents: 500,
        },
    ] {
        let mind: Arc<dyn Mind> = Arc::new(FailingMind { error });
        let strategy = SynthesisStrategy::new(synthesis_config(), mind);
        let mut r = SimRunner::new(
            runner_config(13),
            vec![Box::new(strategy)],
            Box::new(MemoryAuditSink::default()),
            t0(),
        )
        .unwrap();
        set_book(&r, 58, 60);

        // Two ticks: the failure repeats; the loop never crashes and
        // never trades.
        for _ in 0..2 {
            let report = futures::executor::block_on(r.tick()).unwrap();
            assert_eq!(report.proposals, 0, "a failed cycle proposes nothing");
            assert_eq!(report.orders_submitted, 0);
        }
        assert!(
            r.positions().position(&mkt("KX-A")).is_none(),
            "no position from a failing mind"
        );
        // The failure is COUNTED (observable), not swallowed.
        assert!(r.counters().cognition_failures >= 2);
    }
}

// ----------------------------------------------------------- shadow sample

#[test]
fn declined_triage_shadow_runs_believe_but_never_trade() {
    // Two scripted outputs; quota 1: the first declined trigger shadow-
    // runs the mind (beliefs produced), the second is a plain decline.
    let mind: Arc<dyn Mind> = Arc::new(StubMind::scripted(vec![
        belief_output("evt-1", 0.70),
        belief_output("evt-1", 0.70),
    ]));
    let mut cfg = synthesis_config();
    cfg.triage = TriageDecision::AlwaysDecline;
    let strategy = SynthesisStrategy::new(cfg, mind);
    let mut r = SimRunner::new(
        runner_config(17),
        vec![Box::new(strategy)],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_book(&r, 58, 60);

    let report = futures::executor::block_on(r.tick()).unwrap();
    assert_eq!(
        report.proposals, 0,
        "shadow runs NEVER produce trade proposals"
    );
    assert_eq!(report.orders_submitted, 0);
    assert!(
        r.positions().position(&mkt("KX-A")).is_none(),
        "no position from a declined trigger"
    );
    // The shadow run produced beliefs (they are scored like any other).
    assert!(r.counters().shadow_cycles >= 1);
    assert!(r.counters().beliefs_drafted >= 1);
}
