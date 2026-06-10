//! I7: promotion gates
//!
//! Property to encode: a strategy in Sim/Paper stage cannot produce live orders regardless of input; stage promotion requires an operator action record; model swap into live decision flow requires a shadow comparison record
//!
//! Implemented in T2.6's owning slot per tests/README.md (stubs are
//! implemented, never weakened, by their owning BUILD_PLAN task):
//! - The composed runtime today is the SIM runner; it REFUSES at
//!   construction any strategy staged above Sim — before any input can
//!   reach the strategy ("regardless of input": the strategy never runs,
//!   so no input sequence can coax an order out of it).
//! - The Stage ladder is strictly ordered (Sim < Paper < LiveMin <
//!   Scaled) so promotion machinery compares stages, never strings.
//! - There is NO programmatic promotion path at all today: `stage()` is
//!   read-only on the strategy and no runner API mutates it. Promotion
//!   is definitionally an out-of-band operator change. The positive
//!   rails — "promotion requires an operator action record" (T3.1) and
//!   "model swap requires a shadow comparison record" (T3.3) — are
//!   staged below as ignored stubs implemented by their owning tasks,
//!   per the same README pattern this file follows.

use async_trait::async_trait;
use fortuna_core::bus::BusEvent;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_runner::{
    CoreHandle, MemoryAuditSink, Proposal, RunnerConfig, RunnerError, SimRunner, Stage, Strategy,
    StrategyKind, StrategyMetrics,
};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, SettlementMeta};
use std::collections::BTreeMap;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap()
}

/// A strategy whose ONLY distinguishing feature is its declared stage.
/// Its behavior is irrelevant to the invariant: the runner must refuse
/// it by stage before any event can reach it.
struct StagedStrategy {
    stage: Stage,
}

#[async_trait]
impl Strategy for StagedStrategy {
    fn id(&self) -> StrategyId {
        StrategyId::new("staged_probe").unwrap()
    }
    fn kind(&self) -> StrategyKind {
        StrategyKind::Mechanical
    }
    fn stage(&self) -> Stage {
        self.stage
    }
    async fn on_event(
        &mut self,
        _ev: &BusEvent,
        _core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        Ok(Vec::new())
    }
    fn metrics(&self) -> StrategyMetrics {
        StrategyMetrics::default()
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

fn market(id: &str) -> Market {
    Market {
        id: MarketId::new(id).unwrap(),
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

fn runner_config() -> RunnerConfig {
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

        [per_strategy.staged_probe]
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
        seed: 7,
        gate_config,
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("staged_probe".to_string(), Cents::new(100_000))]),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![market("M-1")],
        starting_cash: Cents::new(1_000_000),
        faults: FaultConfig::none(7),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 50,
        veto_mind: None,
        veto_strategies: Vec::new(),
    }
}

#[test]
fn i7_promotion_gates() {
    // The promotion ladder is strictly ordered; machinery that compares
    // stages gets a total order, not string parsing.
    assert!(Stage::Sim < Stage::Paper);
    assert!(Stage::Paper < Stage::LiveMin);
    assert!(Stage::LiveMin < Stage::Scaled);

    // The Sim composition refuses EVERY higher-staged strategy at
    // construction — the strategy's behavior never enters into it, so no
    // input sequence can produce an order from a mis-staged strategy.
    for stage in [Stage::Paper, Stage::LiveMin, Stage::Scaled] {
        let result = SimRunner::new(
            runner_config(),
            vec![Box::new(StagedStrategy { stage })],
            Box::new(MemoryAuditSink::default()),
            t0(),
        );
        match result {
            Err(RunnerError::StageViolation {
                strategy,
                stage: violated,
            }) => {
                assert_eq!(strategy.to_string(), "staged_probe");
                assert_eq!(violated, stage);
            }
            Err(other) => panic!("expected StageViolation for {stage:?}, got {other}"),
            Ok(_) => panic!("a Sim runner accepted a {stage:?}-staged strategy (I7 violation)"),
        }
    }

    // The gate is the STAGE, not the constructor: the same strategy at
    // Sim stage boots fine.
    assert!(SimRunner::new(
        runner_config(),
        vec![Box::new(StagedStrategy { stage: Stage::Sim })],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .is_ok());
}

#[test]
#[ignore = "implement per BUILD_PLAN (T3.1); see tests/README.md"]
fn i7_stage_promotion_requires_operator_action_record() {
    todo!(
        "encode: a stage promotion takes effect only with an operator \
         action record (actor, timestamp, prior stage, new stage) in the \
         ledger; absent the record, the strategy runs at its prior stage"
    );
}

#[test]
#[ignore = "implement per BUILD_PLAN (T3.3); see tests/README.md"]
fn i7_model_swap_requires_shadow_comparison_record() {
    todo!(
        "encode: a model swap into the live decision flow requires a \
         shadow comparison record (>= 30 resolved paired beliefs per \
         active category, Brier/CLV >= incumbent) per spec Section 11"
    );
}
