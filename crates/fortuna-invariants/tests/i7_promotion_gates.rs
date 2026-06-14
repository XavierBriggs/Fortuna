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
use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::market::{MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::{ExecPolicy, MemoryJournal};
use fortuna_runner::{
    CoreHandle, MemoryAuditSink, Proposal, RunnerConfig, RunnerError, SimRunner, Stage, Strategy,
    StrategyKind, StrategyMetrics,
};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::{FaultConfig, SimVenue};
use fortuna_venues::{Market, MarketStatus, SettlementMeta};
use std::collections::BTreeMap;
use std::sync::Arc;

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
        faults: Some(FaultConfig::none(7)),
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
fn i7_stage_promotion_requires_operator_action_record() {
    use fortuna_runner::promotion::{effective_stage, PromotionRecord};

    let record = |from: Stage, to: Stage, actor: &str| PromotionRecord {
        strategy: "synth_events".to_string(),
        from,
        to,
        actor: actor.to_string(),
        at: "2026-06-10T00:00:00.000Z".to_string(),
    };

    // No records: the strategy runs at Sim no matter what it declares.
    assert_eq!(effective_stage(Stage::Scaled, &[]), Stage::Sim);

    // Each promotion step requires an OPERATOR record; the chain must be
    // contiguous from Sim.
    let to_paper = [record(Stage::Sim, Stage::Paper, "operator:xavier")];
    assert_eq!(effective_stage(Stage::Scaled, &to_paper), Stage::Paper);

    // A gap in the chain stops the walk (no skipping stages).
    let skipping = [record(Stage::Paper, Stage::LiveMin, "operator:xavier")];
    assert_eq!(effective_stage(Stage::Scaled, &skipping), Stage::Sim);

    // "system" cannot promote — promotion is a HUMAN action.
    let robot = [record(Stage::Sim, Stage::Paper, "system")];
    assert_eq!(effective_stage(Stage::Scaled, &robot), Stage::Sim);
    let blank = [record(Stage::Sim, Stage::Paper, "  ")];
    assert_eq!(effective_stage(Stage::Scaled, &blank), Stage::Sim);

    // The declared stage is a CAP: records cannot raise a strategy above
    // what its code/config declares.
    let full_chain = [
        record(Stage::Sim, Stage::Paper, "operator:xavier"),
        record(Stage::Paper, Stage::LiveMin, "operator:xavier"),
        record(Stage::LiveMin, Stage::Scaled, "operator:xavier"),
    ];
    assert_eq!(effective_stage(Stage::Paper, &full_chain), Stage::Paper);
    assert_eq!(effective_stage(Stage::Scaled, &full_chain), Stage::Scaled);

    // Demotion is AUTOMATIC on breach (spec Section 11): a step DOWN
    // applies regardless of actor — a compromised path may never
    // promote, but the system may always retreat.
    let demoted = [
        record(Stage::Sim, Stage::Paper, "operator:xavier"),
        record(Stage::Paper, Stage::Sim, "system"),
    ];
    assert_eq!(effective_stage(Stage::Scaled, &demoted), Stage::Sim);
}

#[test]
fn i7_model_swap_requires_shadow_comparison_record() {
    use fortuna_cognition::shadow::{
        evaluate_model_swap, PairedScore, SwapThresholds, SwapVerdict,
    };

    let thresholds = SwapThresholds {
        min_resolved_per_category: 30,
    };
    let active = vec!["weather".to_string()];
    let pairs = |n: usize, challenger_brier: f64| -> Vec<PairedScore> {
        (0..n)
            .map(|i| PairedScore {
                category: "weather".to_string(),
                manifest_hash: format!("hash-{i}"),
                incumbent_brier: 0.20,
                challenger_brier,
                incumbent_clv_bps: Some(50.0),
                challenger_clv_bps: Some(55.0),
            })
            .collect()
    };

    // NO RECORD, NO PROMOTION — the empty record can never recommend.
    let eval = evaluate_model_swap(&[], &active, &thresholds);
    assert_eq!(eval.verdict, SwapVerdict::Hold);

    // An insufficient record (29 < 30 paired resolutions) holds.
    let eval = evaluate_model_swap(&pairs(29, 0.10), &active, &thresholds);
    assert_eq!(eval.verdict, SwapVerdict::Hold);

    // A worse challenger holds regardless of record size.
    let eval = evaluate_model_swap(&pairs(100, 0.30), &active, &thresholds);
    assert_eq!(eval.verdict, SwapVerdict::Hold);

    // A qualifying record yields a RECOMMENDATION only: the verdict type
    // is {PromoteRecommended, Hold} — there is no field, method, or
    // variant that mutates the live model id. Applying a swap is an
    // operator config change recorded in audit.
    let eval = evaluate_model_swap(&pairs(40, 0.15), &active, &thresholds);
    assert_eq!(eval.verdict, SwapVerdict::PromoteRecommended);
}

/// ADD-ONLY (demo-flip Phase 1, A3 guard): the venue-generic refactor split
/// `SimRunner` into `new_with_journal` (SimVenue, `&[Stage::Sim]`) and
/// `new_with_venue` (any venue, caller-supplied allowlist). The historical
/// `SimRunner::new(...)` MUST keep routing through the Sim path's
/// `&[Stage::Sim]` allowlist — so a `Stage::Paper` strategy still cannot board
/// the default sim runner. (The Kalshi demo path opens Paper ONLY via the
/// explicit `new_with_venue(..., &[Stage::Sim, Stage::Paper])` seam; the
/// default constructor never does.) Without the split this was an inline
/// `s.stage() != Stage::Sim` check; this pins that the extraction preserved it.
#[test]
fn i7_sim_runner_new_still_refuses_paper_staged_strategies() {
    let result = SimRunner::new(
        runner_config(),
        vec![Box::new(StagedStrategy {
            stage: Stage::Paper,
        })],
        Box::new(MemoryAuditSink::default()),
        t0(),
    );
    match result {
        Err(RunnerError::StageViolation { strategy, stage }) => {
            assert_eq!(strategy.to_string(), "staged_probe");
            assert_eq!(
                stage,
                Stage::Paper,
                "the default SimRunner::new path must reject Paper via &[Stage::Sim]"
            );
        }
        Err(other) => panic!("expected StageViolation for Paper, got {other}"),
        Ok(_) => {
            panic!("SimRunner::new accepted a Paper-staged strategy — the sim default path no longer pins &[Stage::Sim] (A3/I7 regression)")
        }
    }
}

/// Build a SimVenue + SimClock for the `new_with_venue` allowlist tests. A
/// SimVenue (not a live venue) keeps these tests off any network while still
/// exercising the EXACT construction seam the Kalshi demo path uses — the
/// allowlist check is venue-agnostic (it reads only `strategy.stage()`), so a
/// SimVenue proves it identically.
fn sim_venue_and_clock(config: &RunnerConfig) -> (SimVenue, Arc<SimClock>) {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = SimVenue::new(
        VenueId::new("sim").unwrap(),
        clock.clone(),
        config.fee_model.clone(),
        FaultConfig::none(config.seed),
        config.starting_cash,
    );
    (venue, clock)
}

/// ADD-ONLY (demo-flip Phase 2, I7): the Kalshi demo path constructs the runner
/// via `new_with_venue(..., &[Stage::Sim, Stage::Paper])`. That seam MUST ACCEPT
/// a `Stage::Paper` strategy — and ONLY because the allowlist explicitly admits
/// Paper, never because the stage gate was weakened. The companion test below
/// proves the SAME seam still REFUSES LiveMin/Scaled even with the Paper-opened
/// allowlist, so opening Paper does not open the live stages.
#[tokio::test]
async fn i7_new_with_venue_accepts_paper_when_allowlist_admits_it() {
    let mut config = runner_config();
    // new_with_venue does NOT read config.faults (faults are a SimVenue concept
    // owned by new_with_journal); the Kalshi path sets it None. Mirror that.
    config.faults = None;
    let (venue, clock) = sim_venue_and_clock(&config);
    let result = SimRunner::new_with_venue(
        config,
        vec![Box::new(StagedStrategy {
            stage: Stage::Paper,
        })],
        Box::new(MemoryAuditSink::default()),
        t0(),
        MemoryJournal::default(),
        venue,
        clock,
        &[Stage::Sim, Stage::Paper],
    )
    .await;
    assert!(
        result.is_ok(),
        "new_with_venue(&[Sim, Paper]) must ACCEPT a Paper-staged strategy (the demo seam): {:?}",
        result.err()
    );
}

/// ADD-ONLY (demo-flip Phase 2, I7): the Paper-opened allowlist still REFUSES
/// the live stages. Opening Paper for the demo must NOT silently admit LiveMin
/// or Scaled — promotion to live capital is the forward-validation gate's job
/// (a human action), never a side effect of the demo allowlist.
#[tokio::test]
async fn i7_new_with_venue_refuses_live_stages_even_with_paper_allowlist() {
    for stage in [Stage::LiveMin, Stage::Scaled] {
        let mut config = runner_config();
        config.faults = None;
        let (venue, clock) = sim_venue_and_clock(&config);
        let result = SimRunner::new_with_venue(
            config,
            vec![Box::new(StagedStrategy { stage })],
            Box::new(MemoryAuditSink::default()),
            t0(),
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        )
        .await;
        match result {
            Err(RunnerError::StageViolation {
                strategy,
                stage: violated,
            }) => {
                assert_eq!(strategy.to_string(), "staged_probe");
                assert_eq!(
                    violated, stage,
                    "the demo allowlist &[Sim, Paper] must still refuse {stage:?}"
                );
            }
            Err(other) => panic!("expected StageViolation for {stage:?}, got {other}"),
            Ok(_) => panic!(
                "new_with_venue(&[Sim, Paper]) admitted a {stage:?}-staged strategy — opening \
                 Paper for the demo must NOT open the live stages (I7 regression)"
            ),
        }
    }
}
