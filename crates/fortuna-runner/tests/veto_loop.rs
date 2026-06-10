//! T1.3 veto wiring through the composed Sim loop (spec Section 6 item 2).
//!
//! Doctrine under test:
//! - The veto sits AFTER sizing and BEFORE the gates: it can suppress or
//!   shrink a sized candidate, never grow one, and the gates never consult
//!   it (I1: gates are model-blind pure functions).
//! - EVERY consultation is audited (allow included — cost tracking starts
//!   at zero).
//! - Suppressed/shrunk quantity is counterfactually scored at settlement
//!   against the observable outcome; provider ERRORS suppress fail-closed
//!   but are flagged and never scored (an outage is not model judgment).
//! - Same seed + same scripted mind => byte-identical recordings.

use async_trait::async_trait;
use fortuna_cognition::veto::{KeepBps, StubVetoMind, VetoVerdict};
use fortuna_core::book::{FeeModel, FillRole, PriceLevel};
use fortuna_core::bus::{BusEvent, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_gates::GateConfig;
use fortuna_runner::{
    AuditSink, CoreHandle, MemoryAuditSink, Proposal, ProposedLeg, RunnerConfig, RunnerError,
    SimRunner, Stage, Strategy, StrategyKind, StrategyMetrics, Urgency,
};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, SettlementMeta};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap()
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

fn lvl(price: i64, qty: i64) -> PriceLevel {
    PriceLevel {
        price: Cents::new(price),
        qty: Contracts::new(qty),
    }
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

fn fade_market(id: &str) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("sim").unwrap(),
        title: format!("fade {id}"),
        category: "politics".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "ap".into(),
            resolution_source: "ap".into(),
            expected_lag_hours: 2,
        },
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

        [per_strategy.test_fade]
        max_exposure_cents = 200000
        max_order_notional_cents = 100000
        min_net_edge_bps = 1

        [rate.sim]
        burst = 100
        sustained_per_min = 600
        market_burst = 50
        market_sustained_per_min = 300
        "#,
    )
    .unwrap()
}

/// Audit sink that shares its records with the test (the runner takes
/// ownership of its sink; the test keeps the other handle).
#[derive(Clone, Default)]
struct SharedAuditSink(Arc<Mutex<MemoryAuditSink>>);

impl AuditSink for SharedAuditSink {
    fn append(
        &mut self,
        kind: &str,
        ref_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Result<(), RunnerError> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append(kind, ref_id, payload)
    }
}

impl SharedAuditSink {
    fn rows_of_kind(&self, kind: &str) -> Vec<serde_json::Value> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .records
            .iter()
            .filter(|(k, _, _)| k == kind)
            .map(|(_, _, p)| p.clone())
            .collect()
    }
}

/// Deterministic single-leg test strategy: on the first book snapshot of
/// its market that shows an ask, propose BUY YES at the ask (one shot).
/// `legs` > 1 duplicates the leg (exercises the multi-leg veto guard).
struct TestFade {
    id: StrategyId,
    market: MarketId,
    legs: usize,
    proposed: bool,
    metrics: StrategyMetrics,
}

impl TestFade {
    fn new(market: &str) -> Self {
        TestFade {
            id: StrategyId::new("test_fade").unwrap(),
            market: mkt(market),
            legs: 1,
            proposed: false,
            metrics: StrategyMetrics::default(),
        }
    }

    fn multi_leg(market: &str, legs: usize) -> Self {
        let mut s = TestFade::new(market);
        s.legs = legs;
        s
    }
}

#[async_trait]
impl Strategy for TestFade {
    fn id(&self) -> StrategyId {
        self.id.clone()
    }
    fn kind(&self) -> StrategyKind {
        StrategyKind::Mechanical
    }
    fn stage(&self) -> Stage {
        Stage::Sim
    }
    async fn on_event(
        &mut self,
        ev: &BusEvent,
        _core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        self.metrics.events_seen += 1;
        if self.proposed {
            return Ok(Vec::new());
        }
        let EventPayload::BookSnapshot { book, .. } = &ev.payload else {
            return Ok(Vec::new());
        };
        if book.market != self.market {
            return Ok(Vec::new());
        }
        let Some(ask) = book.yes_asks.first() else {
            return Ok(Vec::new());
        };
        self.proposed = true;
        self.metrics.proposals_emitted += 1;
        let leg = ProposedLeg {
            market: self.market.clone(),
            side: Side::Yes,
            action: Action::Buy,
            limit_price: ask.price,
            fair_value: Cents::new(ask.price.raw() + 5),
        };
        Ok(vec![Proposal {
            legs: vec![leg; self.legs],
            group_policy: if self.legs > 1 {
                Some(fortuna_exec::GroupPolicy {
                    max_unhedged_notional: Cents::new(10_000),
                    max_leg_open_ms: 60_000,
                    value_per_set: Cents::new(100),
                    min_completion_edge_bps: 1,
                })
            } else {
                None
            },
            urgency: Urgency::Passive,
            thesis: "test fade".into(),
        }])
    }
    fn metrics(&self) -> StrategyMetrics {
        self.metrics
    }
}

struct World {
    runner: SimRunner,
    audit: SharedAuditSink,
}

/// max_sets_per_proposal=10 + ample envelope => the sized candidate is
/// exactly 10 contracts before any veto.
fn world(seed: u64, mind: Option<Arc<dyn fortuna_cognition::veto::VetoMind>>) -> World {
    let veto_strategies = if mind.is_some() {
        vec![StrategyId::new("test_fade").unwrap()]
    } else {
        Vec::new()
    };
    world_with(seed, mind, veto_strategies)
}

fn world_with(
    seed: u64,
    mind: Option<Arc<dyn fortuna_cognition::veto::VetoMind>>,
    veto_strategies: Vec<StrategyId>,
) -> World {
    let audit = SharedAuditSink::default();
    let config = RunnerConfig {
        seed,
        gate_config: gate_config(),
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("test_fade".to_string(), Cents::new(300_000))]),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![fade_market("KXFADE")],
        starting_cash: Cents::new(1_000_000),
        faults: FaultConfig::none(seed),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 10,
        veto_mind: mind,
        veto_strategies,
    };
    let runner = SimRunner::new(
        config,
        vec![Box::new(TestFade::new("KXFADE"))],
        Box::new(audit.clone()),
        t0(),
    )
    .unwrap();
    runner
        .venue()
        .set_book(&mkt("KXFADE"), vec![lvl(91, 50)], vec![lvl(92, 50)])
        .unwrap();
    World { runner, audit }
}

fn maker_fee(qty: i64) -> i64 {
    fee_model()
        .fee(
            FillRole::Maker,
            Cents::new(92),
            Contracts::new(qty),
            None,
            t0(),
        )
        .unwrap()
        .raw()
}

// --------------------------------------------------------------- suppress

#[test]
fn suppress_blocks_order_before_gates_and_scores_counterfactual() {
    let mind = Arc::new(StubVetoMind::scripted(vec![(
        mkt("KXFADE"),
        VetoVerdict::Suppress {
            reason: "scripted suppress".into(),
        },
    )]));
    let mut w = world(7, Some(mind));
    let report = futures::executor::block_on(w.runner.tick()).unwrap();

    assert_eq!(
        report.orders_submitted, 0,
        "suppressed order must not submit"
    );
    // The gates were never consulted for the suppressed candidate (the veto
    // sits BEFORE the pipeline; I1 keeps the gates model-blind).
    assert!(
        w.audit.rows_of_kind("gate_decision").is_empty(),
        "no gate rows for a vetoed candidate"
    );
    let vetoes = w.audit.rows_of_kind("veto_decision");
    assert_eq!(vetoes.len(), 1);
    assert_eq!(vetoes[0]["qty_before"], 10);
    assert_eq!(vetoes[0]["qty_after"], 0);
    assert_eq!(vetoes[0]["veto_error"], false);
    assert!(vetoes[0]["assessment"]["verdict"]["suppress"].is_object());

    // Settlement YES at 100c: the suppressed 10 contracts at 92c would have
    // netted 10*(100-92) - maker_fee. The veto FORFEITED that (positive).
    w.runner
        .apply_settlement(&mkt("KXFADE"), Side::Yes, Cents::new(100))
        .unwrap();
    let scores = w.audit.rows_of_kind("veto_counterfactual");
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0]["removed"], 10);
    assert_eq!(
        scores[0]["hypothetical_pnl_cents"],
        80 - maker_fee(10),
        "hypothetical = removed x (settle - limit) - maker fee"
    );
    assert_eq!(scores[0]["fill_assumption"], "filled_at_limit");

    // Scored once, never re-scored (a second settlement event drains
    // nothing; the market held no position so nothing else errors).
    w.runner
        .apply_settlement(&mkt("KXFADE"), Side::Yes, Cents::new(100))
        .unwrap();
    assert_eq!(w.audit.rows_of_kind("veto_counterfactual").len(), 1);
}

// ----------------------------------------------------------------- shrink

#[test]
fn shrink_halves_the_sized_candidate_and_scores_the_removed_half() {
    let mind = Arc::new(StubVetoMind::scripted(vec![(
        mkt("KXFADE"),
        VetoVerdict::Shrink {
            keep: KeepBps::new(5_000).unwrap(),
            reason: "scripted shrink".into(),
        },
    )]));
    let mut w = world(11, Some(mind));
    let report = futures::executor::block_on(w.runner.tick()).unwrap();

    assert_eq!(report.orders_submitted, 1);
    let submitted: Vec<i64> = w
        .runner
        .manager()
        .intents()
        .iter()
        .map(|(_, r)| r.order.qty.raw())
        .collect();
    assert_eq!(submitted, vec![5], "10 sized, keep 50% => 5 submitted");

    let vetoes = w.audit.rows_of_kind("veto_decision");
    assert_eq!(vetoes.len(), 1);
    assert_eq!(vetoes[0]["qty_before"], 10);
    assert_eq!(vetoes[0]["qty_after"], 5);

    // Settles NO: the removed 5 contracts at 92c would have LOST
    // 5*92 + maker_fee. Negative hypothetical = the veto avoided a loss.
    w.runner
        .apply_settlement(&mkt("KXFADE"), Side::No, Cents::new(100))
        .unwrap();
    let scores = w.audit.rows_of_kind("veto_counterfactual");
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0]["removed"], 5);
    assert_eq!(scores[0]["hypothetical_pnl_cents"], -460 - maker_fee(5));
}

// ------------------------------------------------------------ veto errors

#[test]
fn provider_error_suppresses_fail_closed_flagged_and_unscored() {
    let mind = Arc::new(StubVetoMind::failing("provider down"));
    let mut w = world(13, Some(mind));
    let report = futures::executor::block_on(w.runner.tick()).unwrap();

    assert_eq!(report.orders_submitted, 0, "unanswered veto fails closed");
    let vetoes = w.audit.rows_of_kind("veto_decision");
    assert_eq!(vetoes.len(), 1);
    assert_eq!(vetoes[0]["veto_error"], true);
    assert_eq!(vetoes[0]["qty_after"], 0);

    // An outage is not model judgment: never counterfactually scored.
    w.runner
        .apply_settlement(&mkt("KXFADE"), Side::Yes, Cents::new(100))
        .unwrap();
    assert!(w.audit.rows_of_kind("veto_counterfactual").is_empty());
}

// -------------------------------------------------------------- allow path

#[test]
fn allow_is_audited_and_changes_nothing() {
    let mind = Arc::new(StubVetoMind::allow_all());
    let mut w = world(17, Some(mind));
    let report = futures::executor::block_on(w.runner.tick()).unwrap();

    assert_eq!(report.orders_submitted, 1);
    let submitted: Vec<i64> = w
        .runner
        .manager()
        .intents()
        .iter()
        .map(|(_, r)| r.order.qty.raw())
        .collect();
    assert_eq!(submitted, vec![10], "allow keeps the full sized qty");

    // Every consultation is audited, allows included (cost tracking).
    let vetoes = w.audit.rows_of_kind("veto_decision");
    assert_eq!(vetoes.len(), 1);
    assert_eq!(vetoes[0]["qty_before"], 10);
    assert_eq!(vetoes[0]["qty_after"], 10);
    assert_eq!(vetoes[0]["assessment"]["cost_cents"], 0);

    // Nothing to score at settlement (nothing was removed).
    w.runner
        .apply_settlement(&mkt("KXFADE"), Side::Yes, Cents::new(100))
        .unwrap();
    assert!(w.audit.rows_of_kind("veto_counterfactual").is_empty());
}

// ------------------------------------------------- non-veto strategy path

#[test]
fn strategy_outside_veto_list_is_never_consulted() {
    let mind: Arc<dyn fortuna_cognition::veto::VetoMind> =
        Arc::new(StubVetoMind::scripted(vec![(
            mkt("KXFADE"),
            VetoVerdict::Suppress {
                reason: "would suppress if consulted".into(),
            },
        )]));
    // Mind configured, but test_fade NOT enrolled.
    let mut w = world_with(19, Some(mind), Vec::new());
    let report = futures::executor::block_on(w.runner.tick()).unwrap();

    assert_eq!(report.orders_submitted, 1, "unenrolled strategy trades");
    assert!(w.audit.rows_of_kind("veto_decision").is_empty());
}

// ----------------------------------------------------------- config guard

#[test]
fn veto_enabled_strategy_without_a_mind_is_a_construction_error() {
    let audit = SharedAuditSink::default();
    let config = RunnerConfig {
        seed: 23,
        gate_config: gate_config(),
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("test_fade".to_string(), Cents::new(300_000))]),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![fade_market("KXFADE")],
        starting_cash: Cents::new(1_000_000),
        faults: FaultConfig::none(23),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 10,
        veto_mind: None,
        veto_strategies: vec![StrategyId::new("test_fade").unwrap()],
    };
    let err = SimRunner::new(
        config,
        vec![Box::new(TestFade::new("KXFADE"))],
        Box::new(audit),
        t0(),
    )
    .err()
    .expect("veto-enrolled strategy with no mind must not construct");
    assert!(matches!(err, RunnerError::Config { .. }));
}

// ------------------------------------------------- multi-leg veto guard

/// Veto semantics for grouped proposals are deliberately UNDEFINED this
/// phase (no veto-enrolled strategy emits them; partial-group vetoes would
/// create unhedged legs). A veto-enrolled multi-leg proposal is suppressed
/// whole, loudly — never half-vetoed, never silently passed through.
#[test]
fn multileg_proposal_from_veto_enrolled_strategy_is_suppressed_whole() {
    let mind: Arc<dyn fortuna_cognition::veto::VetoMind> = Arc::new(StubVetoMind::allow_all());
    let audit = SharedAuditSink::default();
    let config = RunnerConfig {
        seed: 31,
        gate_config: gate_config(),
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("test_fade".to_string(), Cents::new(300_000))]),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![fade_market("KXFADE")],
        starting_cash: Cents::new(1_000_000),
        faults: FaultConfig::none(31),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 10,
        veto_mind: Some(mind),
        veto_strategies: vec![StrategyId::new("test_fade").unwrap()],
    };
    let mut runner = SimRunner::new(
        config,
        vec![Box::new(TestFade::multi_leg("KXFADE", 2))],
        Box::new(audit.clone()),
        t0(),
    )
    .unwrap();
    runner
        .venue()
        .set_book(&mkt("KXFADE"), vec![lvl(91, 50)], vec![lvl(92, 50)])
        .unwrap();
    let report = futures::executor::block_on(runner.tick()).unwrap();

    assert_eq!(
        report.orders_submitted, 0,
        "suppressed whole, no half-group"
    );
    let vetoes = audit.rows_of_kind("veto_decision");
    assert_eq!(vetoes.len(), 1);
    assert_eq!(vetoes[0]["veto_unsupported_multileg"], true);
    assert_eq!(vetoes[0]["qty_after"], 0);
}

// ------------------------------------------------------------ determinism

#[test]
fn same_seed_same_script_byte_identical_recordings() {
    let run = || {
        let mind = Arc::new(StubVetoMind::scripted(vec![(
            mkt("KXFADE"),
            VetoVerdict::Shrink {
                keep: KeepBps::new(2_500).unwrap(),
                reason: "det".into(),
            },
        )]));
        let mut w = world(29, Some(mind));
        futures::executor::block_on(w.runner.tick()).unwrap();
        futures::executor::block_on(w.runner.tick()).unwrap();
        w.runner
            .apply_settlement(&mkt("KXFADE"), Side::Yes, Cents::new(100))
            .unwrap();
        w.runner.report().unwrap().recording_jsonl
    };
    let a = run();
    let b = run();
    assert_eq!(a, b, "veto wiring must not break byte-determinism");
}
