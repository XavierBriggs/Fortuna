//! Perp-strategies design §2.5: the SCALAR belief egress seam. Written from
//! the design text BEFORE implementation.
//!
//! Contract under test (the additive seam, parallel to the binary belief
//! drain pinned by fortuna-live/tests/belief_persist.rs):
//! - `Strategy::drain_scalar_beliefs()` defaults to empty, so every existing
//!   strategy is unaffected.
//! - A strategy that DOES emit scalar drafts has them buffered on the runner
//!   each tick and returned, tagged with the producing `StrategyId`, by
//!   `SimRunner::drain_pending_scalar_beliefs()`.
//! - The buffer drains ONCE (a second drain after the strategy stops emitting
//!   is empty), mirroring the binary `drain_pending_beliefs` guarantee.

use fortuna_cognition::scalar_beliefs::ScalarBeliefDraft;
use fortuna_cognition::scoring::{PredictiveDistribution, Quantile};
use fortuna_core::bus::BusEvent;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::StrategyId;
use fortuna_core::money::Cents;
use fortuna_runner::{
    CoreHandle, MemoryAuditSink, Proposal, RunnerConfig, RunnerError, SimRunner, Stage, Strategy,
    StrategyKind, StrategyMetrics,
};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use std::collections::BTreeMap;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-13T12:00:00.000Z").unwrap()
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        "formula = \"quadratic\"\neffective_date = \"2026-01-01\"\ntaker_coeff = \"0.07\"\nmaker_coeff = \"0.0175\"\n",
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

fn runner_config() -> RunnerConfig {
    RunnerConfig {
        seed: 1,
        gate_config: toml::from_str(
            "[global]\nmax_total_exposure_cents=800000\nmax_daily_loss_cents=50000\nmin_order_contracts=1\nmax_order_contracts=1000\nprice_band_cents=45\nmax_cross_cents=10\nper_market_exposure_cents=100000\nper_event_exposure_cents=150000\nrequire_event_mapping=false\n[rate.sim]\nburst=100\nsustained_per_min=600\nmarket_burst=50\nmarket_sustained_per_min=300\n",
        )
        .unwrap(),
        exec_policy: fortuna_exec::ExecPolicy::default(),
        envelopes: BTreeMap::new(),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![],
        starting_cash: Cents::new(1_000_000),
        faults: Some(FaultConfig::none(1)),
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

fn scalar_draft(event_key: &str) -> ScalarBeliefDraft {
    ScalarBeliefDraft {
        event_key: event_key.to_string(),
        predictive: PredictiveDistribution::Scalar {
            quantiles: vec![
                Quantile { q: 0.1, v: -0.0003 },
                Quantile { q: 0.5, v: 0.0001 },
                Quantile { q: 0.9, v: 0.0007 },
            ],
            unit: "rate".to_string(),
        },
        horizon: t0(),
        evidence: serde_json::json!([{"source": "funding_estimate"}]),
        provenance: serde_json::json!({}),
    }
}

/// Emits two scalar drafts on its first drain, then none — the minimal
/// scalar producer (no proposals; scalar beliefs are the product). Models the
/// shape funding_forecast (slice 2b) will take.
struct ScalarProducer {
    id: StrategyId,
    emitted: bool,
}

#[async_trait::async_trait]
impl Strategy for ScalarProducer {
    fn id(&self) -> StrategyId {
        self.id.clone()
    }
    fn kind(&self) -> StrategyKind {
        StrategyKind::Synthesis
    }
    fn stage(&self) -> Stage {
        Stage::Sim
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
    fn drain_scalar_beliefs(&mut self) -> Vec<ScalarBeliefDraft> {
        if self.emitted {
            return Vec::new();
        }
        self.emitted = true;
        vec![
            scalar_draft("perp:KXBTCPERP:funding:2026-06-13T16:00:00Z"),
            scalar_draft("perp:KXBTCPERP1:funding:2026-06-13T16:00:00Z"),
        ]
    }
}

/// A plain strategy that overrides NOTHING scalar — proves the default impl.
struct NoScalarStrategy {
    id: StrategyId,
}

#[async_trait::async_trait]
impl Strategy for NoScalarStrategy {
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
        _ev: &BusEvent,
        _core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        Ok(Vec::new())
    }
    fn metrics(&self) -> StrategyMetrics {
        StrategyMetrics::default()
    }
    // No drain_scalar_beliefs override: the trait default applies.
}

#[test]
fn drain_scalar_beliefs_default_is_empty() {
    // The default impl returns empty for a strategy that does not override it,
    // so every existing strategy is unaffected by the additive seam.
    let mut s = NoScalarStrategy {
        id: StrategyId::new("mech_x").unwrap(),
    };
    assert!(s.drain_scalar_beliefs().is_empty());
}

#[test]
fn scalar_drafts_drain_through_the_runner_tagged_and_once() {
    let mut r = SimRunner::new(
        runner_config(),
        vec![Box::new(ScalarProducer {
            id: StrategyId::new("funding_forecast").unwrap(),
            emitted: false,
        })],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();

    futures::executor::block_on(r.tick()).unwrap();
    let drained = r.drain_pending_scalar_beliefs();
    assert_eq!(
        drained.len(),
        2,
        "two scalar drafts drained from the runner"
    );
    // Each is tagged with the producing strategy id.
    for (sid, draft) in &drained {
        assert_eq!(sid.as_str(), "funding_forecast");
        assert!(matches!(
            draft.predictive,
            PredictiveDistribution::Scalar { .. }
        ));
    }
    assert_eq!(
        drained[0].1.event_key,
        "perp:KXBTCPERP:funding:2026-06-13T16:00:00Z"
    );

    // Drains once: a second tick (producer now silent) yields nothing.
    futures::executor::block_on(r.tick()).unwrap();
    assert!(
        r.drain_pending_scalar_beliefs().is_empty(),
        "scalar beliefs drain once"
    );
}

#[test]
fn binary_belief_buffer_is_untouched_by_a_scalar_only_producer() {
    // The additive seam does not leak into the binary path: a scalar-only
    // producer adds nothing to the binary pending_beliefs buffer.
    let mut r = SimRunner::new(
        runner_config(),
        vec![Box::new(ScalarProducer {
            id: StrategyId::new("funding_forecast").unwrap(),
            emitted: false,
        })],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();

    futures::executor::block_on(r.tick()).unwrap();
    assert!(
        r.drain_pending_beliefs().is_empty(),
        "scalar producer must not populate the binary belief buffer"
    );
    assert_eq!(
        r.drain_pending_scalar_beliefs().len(),
        2,
        "the scalar drafts are in the scalar buffer"
    );
}
