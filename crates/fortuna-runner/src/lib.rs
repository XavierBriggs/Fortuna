//! fortuna-runner: the composed deterministic core. Spec 4 (data flow), 6
//! (Strategy trait), 5.1 (single-threaded loop), I1-I7 wiring.
//!
//! One full cycle (spec Section 4): venue data enters the bus and is
//! recorded point-in-time -> strategies propose (UNSIZED) -> the comparator/
//! sizing layer derives candidate orders (fractional Kelly / arb sizing from
//! envelope headroom via reservations) -> the universal gate pipeline ->
//! the order manager -> the venue -> fills update positions/reservations ->
//! drawdown is checked -> everything lands in the audit sink and the bus
//! recording (replay).
//!
//! Strategies are iterated by the RUNNER in registration order after bus
//! dispatch (not as bus handlers): ownership stays simple and ordering stays
//! deterministic; the bus remains the byte-exact record of every input and
//! decision artifact (ASSUMPTIONS.md, T0.10).

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

pub mod mech_extremes;
pub mod mech_structural;
pub mod promotion;
mod runner;
pub mod synth_events;
pub mod synthesis;

pub use runner::{
    LatencyStat, MetricSample, RunCounters, RunnerConfig, RunnerReport, ShutdownReport, SimRunner,
    TickReport, LATENCY_BUCKETS_MS,
};

use async_trait::async_trait;
use fortuna_core::book::{FeeModel, OrderBook};
use fortuna_core::bus::BusEvent;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, MarketId, Side, StrategyId};
use fortuna_core::money::Cents;
use fortuna_exec::GroupPolicy;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error(
        "strategy {strategy} is staged {stage:?}; a Sim runner only accepts Sim strategies (I7)"
    )]
    StageViolation { strategy: StrategyId, stage: Stage },
    #[error("audit sink failed: {reason} (no audit, no trading: halting)")]
    AuditFailed { reason: String },
    #[error("config error: {reason}")]
    Config { reason: String },
    #[error(transparent)]
    Exec(#[from] fortuna_exec::ExecError),
    #[error(transparent)]
    State(#[from] fortuna_state::StateError),
    #[error(transparent)]
    Gates(#[from] fortuna_gates::GateError),
    #[error(transparent)]
    Venue(#[from] fortuna_venues::VenueError),
    #[error(transparent)]
    Bus(#[from] fortuna_core::bus::BusError),
    #[error(transparent)]
    Money(#[from] fortuna_core::money::MoneyError),
    #[error("clock error: {0}")]
    Clock(#[from] fortuna_core::clock::ClockError),
    #[error("id error: {0}")]
    Id(#[from] fortuna_core::ids::IdError),
}

/// Mechanical strategies act without a model; Synthesis strategies trade
/// model beliefs (Phase 2+).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyKind {
    Mechanical,
    Synthesis,
}

/// Validation stage (spec Section 11, I7). Promotion is a HUMAN action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Sim,
    Paper,
    LiveMin,
    Scaled,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StrategyMetrics {
    pub events_seen: u64,
    pub proposals_emitted: u64,
    /// Decision cycles that failed in cognition (provider error, schema-
    /// invalid output, refusal, budget exhaustion, context failure) and
    /// degraded to zero proposals. Mechanical strategies leave this 0.
    pub cognition_failures: u64,
    /// Declined-trigger cycles run in shadow (beliefs scored, no trades).
    pub shadow_cycles: u64,
    /// Belief drafts produced by the mind across all cycles.
    pub beliefs_drafted: u64,
    /// Model-emitted ProposalDrafts the cycle discarded (the harness
    /// derives its own candidates; the discard is counted, never silent).
    pub model_proposals_discarded: u64,
    /// Cognition spend accrued by COMPLETED decisions (cents). Failed
    /// calls also burn tokens; the budget-true total including them is
    /// `mind_spend_today_cents`.
    pub cognition_cost_cents: i64,
    /// The mind's own running spend today (budget-true: includes failed
    /// calls). Gauge semantics; resets at the mind's 00:00 UTC roll.
    pub mind_spend_today_cents: i64,
}

/// One degraded-cognition event for the audit log (F1: degrade is never
/// silent). Strategies buffer these; the runner drains and audits them
/// each tick.
#[derive(Debug, Clone)]
pub struct DegradeRecord {
    pub event_id: String,
    /// 'budget_exhausted' | 'provider' | 'schema_invalid' | 'refused'
    /// | 'context' | 'model_proposals_discarded'.
    pub degrade: &'static str,
    pub detail: serde_json::Value,
}

/// Execution style request (never size — spec 5.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Passive,
    Taker,
}

/// One UNSIZED leg of a proposal. `fair_value` is the strategy's honest
/// deterministic value for one contract (the gates recompute net edge from
/// it; gaming it games your own risk checks).
#[derive(Debug, Clone)]
pub struct ProposedLeg {
    pub market: MarketId,
    pub side: Side,
    pub action: Action,
    pub limit_price: Cents,
    pub fair_value: Cents,
    /// CALIBRATED win-probability of this leg's side (synthesis legs
    /// only; mechanical legs carry None). The harness uses it for
    /// haircut-Kelly sizing — the strategy never sizes (I6).
    pub calibrated_p: Option<f64>,
}

/// What strategies emit. Sizing, gating, timing, and execution belong to
/// the harness (I6 discipline applies to mechanical strategies too).
#[derive(Debug, Clone)]
pub struct Proposal {
    pub legs: Vec<ProposedLeg>,
    /// Required when legs.len() > 1.
    pub group_policy: Option<GroupPolicy>,
    pub urgency: Urgency,
    pub thesis: String,
    /// The context-manifest hash of the decision cycle that produced
    /// this proposal (synthesis only; mechanical scans carry None). The
    /// runner audits it so any decision is replayable (spec 5.7).
    pub manifest_hash: Option<String>,
}

/// Read-only views a strategy may consult while handling an event.
pub struct CoreHandle<'a> {
    pub now: UtcTimestamp,
    pub books: &'a BTreeMap<MarketId, OrderBook>,
    /// Venue catalog metadata (status, close time, volume) as of the last
    /// sync — point-in-time data, same as books.
    pub markets: &'a BTreeMap<MarketId, fortuna_venues::Market>,
    pub fee_model: &'a dyn FeeModel,
}

/// The strategy plugin interface (spec Section 6).
#[async_trait]
pub trait Strategy: Send {
    fn id(&self) -> StrategyId;
    fn kind(&self) -> StrategyKind;
    fn stage(&self) -> Stage;
    async fn on_event(
        &mut self,
        ev: &BusEvent,
        core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError>;
    fn metrics(&self) -> StrategyMetrics;
    /// Degraded-cognition events since the last drain (F1). The runner
    /// audits each one; mechanical strategies have none.
    fn drain_degrades(&mut self) -> Vec<DegradeRecord> {
        Vec::new()
    }
    /// Belief drafts produced since the last drain (T4.1 req 6). The
    /// runner collects them; the composition persists them to
    /// BeliefsRepo. Mechanical strategies hold no beliefs.
    fn drain_beliefs(&mut self) -> Vec<fortuna_cognition::beliefs::BeliefDraft> {
        Vec::new()
    }
}

/// Where audit records go. The Postgres-backed writer satisfies this in the
/// live composition; tests use the in-memory sink (with failure injection
/// for the I5 no-audit-no-trading scenario).
pub trait AuditSink: Send {
    fn append(
        &mut self,
        kind: &str,
        ref_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Result<(), RunnerError>;
}

/// In-memory audit sink with failure injection (DST/I5).
#[derive(Debug, Default)]
pub struct MemoryAuditSink {
    pub records: Vec<(String, Option<String>, serde_json::Value)>,
    pub fail_after: Option<usize>,
}

impl AuditSink for MemoryAuditSink {
    fn append(
        &mut self,
        kind: &str,
        ref_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Result<(), RunnerError> {
        if let Some(limit) = self.fail_after {
            if self.records.len() >= limit {
                return Err(RunnerError::AuditFailed {
                    reason: "injected audit store failure".into(),
                });
            }
        }
        self.records
            .push((kind.to_string(), ref_id.map(String::from), payload));
        Ok(())
    }
}
