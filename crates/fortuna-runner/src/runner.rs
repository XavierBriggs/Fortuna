//! The composed Sim loop (spec Section 4 data flow; Phase 0 exit vehicle).
//!
//! Single-threaded and deterministic: every input lands on the bus first
//! (the byte-exact replay record), strategies run in registration order over
//! newly recorded events, proposals are sized from envelope headroom,
//! pushed through the universal gates, submitted via the order manager, and
//! every artifact (gate verdicts, submissions, fills, halts, group
//! decisions) is both audited and republished onto the bus.
//!
//! Fail-closed wiring proven here: audit-sink failure => global halt (I5);
//! drawdown breach => global halt (I2); both clear only via operator re-arm.

use crate::{AuditSink, CoreHandle, Proposal, RunnerError, Stage, Strategy};
use fortuna_cognition::cycle::haircut_kelly_fraction;
use fortuna_cognition::veto::{
    counterfactual_pnl, FillAssumption, VetoCandidate, VetoMind, VetoVerdict,
};
use fortuna_core::book::FeeModel;
use fortuna_core::bus::{EventBus, EventPayload, Recording};
use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentGroupId, IntentId};
use fortuna_core::market::{ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{FundingObservation, PerpMarks};
use fortuna_exec::{
    decide_complete_or_unwind, CancelOutcome, CompleteOrUnwind, ExecError, ExecPolicy,
    GroupDecision, GroupTracker, IntentJournal, IntentStatus, LegOutcome, MemoryJournal,
    OrderManager, RemainingLeg, SubmitOutcome,
};
use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline, HaltScope};
use fortuna_state::{
    affordable_sets, kelly_contracts, mark_lots, DrawdownMonitor, DrawdownVerdict, MarkPolicy,
    PositionBook, PositionLifecycle, ReservationLedger, SettlementLedger, SettlementSnapshot,
    SettlementStatus,
};
use fortuna_venues::fees::ScheduleFeeModel;
use fortuna_venues::sim::{FaultConfig, SimVenue};
use fortuna_venues::{
    Cursor, Market, MarketFilter, MarketStatus, SettlementNotice, SettlementOutcome, Venue,
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct RunnerConfig {
    pub seed: u64,
    pub gate_config: GateConfig,
    pub exec_policy: ExecPolicy,
    pub envelopes: BTreeMap<String, Cents>,
    pub max_daily_loss: Cents,
    pub fee_model: ScheduleFeeModel,
    pub markets: Vec<Market>,
    pub starting_cash: Cents,
    /// SimVenue-only fault injection (DST world arms). `Some(..)` on the
    /// `new_with_journal` (SimVenue) path; `None` on `new_with_venue`, where
    /// the venue is supplied and faults belong to the venue itself (Kalshi has
    /// no fault arms). `new_with_journal` REQUIRES `Some` (a `None` there is a
    /// construction error — the sim world cannot be built faultless-by-omission).
    pub faults: Option<FaultConfig>,
    pub mark_policy: MarkPolicy,
    /// Max sets per arb proposal (belt to the gates' braces).
    pub max_sets_per_proposal: i64,
    /// Base fractional-Kelly scalar for synthesis sizing (spec line 240;
    /// config [sizing] kelly_fraction, default 0.25). The effective
    /// fraction is this x the strategy's calibration quality.
    pub kelly_fraction: f64,
    /// The reduce-only model veto (spec Section 6). Strategies listed in
    /// `veto_strategies` have every sized candidate assessed BEFORE the
    /// gates; listing a strategy without providing a mind is a
    /// construction error (the spec ships mech_extremes WITH its veto,
    /// silently skipping it would be a lie).
    pub veto_mind: Option<Arc<dyn VetoMind>>,
    pub veto_strategies: Vec<StrategyId>,
}

#[derive(Debug, Default)]
pub struct TickReport {
    pub events_dispatched: usize,
    pub proposals: usize,
    pub orders_submitted: usize,
    pub gate_rejections: usize,
    pub fills_applied: usize,
    pub group_decisions: usize,
    pub halted: bool,
}

/// Outcome accounting for the graceful-shutdown contract (T4.1 req 8).
/// `unacked` is the honest count of orders that COULD NOT be cancelled
/// (submitted, never acked) — never folded into `cancelled`.
#[derive(Debug, Default)]
pub struct ShutdownReport {
    pub working: usize,
    pub cancelled: usize,
    pub already_gone: usize,
    pub unknown: usize,
    pub unacked: usize,
}

#[derive(Debug)]
pub struct RunnerReport {
    pub recording_jsonl: String,
    pub final_cash: Cents,
    pub realized_pnl: Cents,
    pub fees_paid: Cents,
}

/// The Phase 0 composition over a venue. Journal-generic since T4.1: the
/// daemon composes the SAME runner over `PgIntentJournal` (durable intents in
/// Postgres). Venue-generic since the demo-flip (Phase 1): `V` defaults to
/// `SimVenue`, so `SimRunner` and `SimRunner<SimVenue, J>` both resolve to the
/// pre-generalization type and stay byte-identical (A3). The param order is
/// `<V, J>` (V first) so the historical single-arg form `SimRunner<J>` keeps
/// J as the journal slot via the `V = SimVenue` default — NO; that resolution
/// would bind J into the V slot. Single-arg callers were migrated to the
/// explicit `SimRunner<SimVenue, J>` form; bare `SimRunner` still works.
pub struct SimRunner<V: Venue = SimVenue, J: IntentJournal + Send = MemoryJournal> {
    pub clock: Arc<SimClock>,
    bus: EventBus,
    venue: V,
    gates: GatePipeline,
    manager: OrderManager<J>,
    positions: PositionBook,
    reservations: ReservationLedger,
    drawdown: DrawdownMonitor,
    strategies: Vec<Box<dyn Strategy>>,
    audit: Box<dyn AuditSink>,
    groups: GroupTracker,
    fee_model: ScheduleFeeModel,
    mark_policy: MarkPolicy,
    books: BTreeMap<MarketId, fortuna_core::book::OrderBook>,
    markets: Vec<MarketId>,
    envelope_names: Vec<String>,
    ids: IdGen,
    cursor: Cursor,
    seen_events: usize,
    max_sets_per_proposal: i64,
    /// Set when the audit sink dies: hard stop beyond the gate halt.
    audit_dead: bool,
    /// Belief drafts drained from strategies, awaiting composition-side
    /// persistence (req 6). The runner never writes them (Pg-agnostic);
    /// the daemon drains via `drain_pending_beliefs`.
    pending_beliefs: Vec<(StrategyId, fortuna_cognition::beliefs::BeliefDraft)>,
    /// SCALAR belief drafts drained from strategies, awaiting composition-side
    /// persistence to the `scalar_beliefs` path (perp-strategies design §2.5).
    /// Parallel to `pending_beliefs`; the runner never writes them (Pg-
    /// agnostic), the daemon drains via `drain_pending_scalar_beliefs`.
    pending_scalar_beliefs: Vec<(
        StrategyId,
        fortuna_cognition::scalar_beliefs::ScalarBeliefDraft,
    )>,
    veto_mind: Option<Arc<dyn VetoMind>>,
    veto_strategies: std::collections::BTreeSet<StrategyId>,
    /// Vetoed-away quantity awaiting its market's settlement for
    /// counterfactual scoring (spec Section 6: veto value-add is measured,
    /// not believed). Provider-error suppressions are NOT tracked here.
    open_vetoes: Vec<OpenVeto>,
    market_meta: BTreeMap<MarketId, Market>,
    /// Settlement-entry chains (spec 5.13: pending -> posted -> confirmed
    /// | reversed, all superseding inserts).
    settlements: SettlementLedger,
    settle_cursor: Cursor,
    seen_notices: std::collections::BTreeSet<String>,
    /// Pre-settlement lot snapshots, kept so a venue CORRECTION can
    /// reverse the books exactly (spec 5.13 reversal path).
    settled_snapshots: BTreeMap<MarketId, SettlementSnapshot>,
    /// Vetoes already counterfactually scored, retained so a venue
    /// correction can RE-score them against the corrected outcome.
    scored_vetoes: BTreeMap<MarketId, Vec<OpenVeto>>,
    /// Watchdog debounce + streak state.
    overdue_alerted: std::collections::BTreeSet<MarketId>,
    dispute_frozen: std::collections::BTreeSet<MarketId>,
    mismatch_streak: BTreeMap<MarketId, u32>,
    /// Canonical event resolutions per market, fed by the composition's
    /// events pipeline. Settling against a disagreeing canonical truth
    /// records a settlement_divergence (spec 5.13: two truths coexist;
    /// PnL follows venue truth, divergence is recorded, never
    /// reconciled away). Value: (canonical outcome, edge id).
    canonical_resolutions: BTreeMap<MarketId, (Side, String)>,
    /// Calibration quality per strategy (T2.8 calibration_quality), fed
    /// by the composition. MISSING => 0.0 => synthesis sizes ZERO (fail
    /// closed; an unmeasured calibration earns no size).
    calibration_quality: BTreeMap<String, f64>,
    kelly_fraction: f64,
    /// Submit timestamps by client order id (epoch ms), for fill-latency
    /// measurement; pruned when the intent reaches a terminal state.
    submit_times: BTreeMap<String, i64>,
    /// Section 8 "gate rejection counts by reason": per-check tallies.
    gate_rejections_by_check: BTreeMap<String, u64>,
    /// Markets with a defined next processor (fresh open belief OR a
    /// mechanical owner), fed by the composition. None = the coverage
    /// view is not wired and the orphan watchdog stays silent.
    position_coverage: Option<std::collections::BTreeSet<MarketId>>,
    orphan_flagged: std::collections::BTreeSet<MarketId>,
    /// Historical venue fills can appear on a non-advancing cursor page across
    /// ticks. Record the orphan once; repeated pages are a known fact, not a new
    /// discrepancy.
    seen_orphan_fills: std::collections::BTreeSet<String>,
    /// Venue positions outside this runner's configured/local universe belong
    /// to the account, but not to this runner's book. Surface them once without
    /// turning them into a self-mismatch halt.
    seen_external_positions: std::collections::BTreeSet<MarketId>,
    counters: RunCounters,
}

struct OpenVeto {
    candidate: VetoCandidate,
    removed: Contracts,
}

/// Submit->ack / submit->execution latency aggregate (count, sum, max).
/// Sum/count give the mean downstream (Prometheus convention); max
/// catches the tail. Under SimClock, ack latency is truthfully ~0 (the
/// await is instantaneous); fill latency is real whenever the venue's
/// fault arms delay execution across ticks — and both become wall time
/// in paper/live, where they tune order TTLs and re-quote cadence.
#[derive(Debug, Default, Clone, Copy)]
pub struct LatencyStat {
    pub count: u64,
    pub sum_ms: i64,
    pub max_ms: i64,
    /// Per-bucket counts for LATENCY_BUCKETS_MS, plus the overflow
    /// bucket at the end. Fixed bounds keep the histogram deterministic
    /// and Copy; quantiles estimate from bucket UPPER edges
    /// (conservative: never understates latency).
    pub bucket_counts: [u64; LATENCY_BUCKETS_MS.len() + 1],
}

/// Histogram bucket upper bounds in ms. Chosen for a maker system whose
/// latency budget is seconds: sub-100ms resolution for venue RTTs,
/// coarse tail to a minute. Changing bounds is a config-style decision —
/// recorded in ASSUMPTIONS.
pub const LATENCY_BUCKETS_MS: [i64; 14] = [
    1, 5, 10, 25, 50, 100, 250, 500, 1_000, 2_500, 5_000, 15_000, 30_000, 60_000,
];

impl LatencyStat {
    fn observe(&mut self, ms: i64) {
        let ms = ms.max(0);
        self.count += 1;
        self.sum_ms = self.sum_ms.saturating_add(ms);
        self.max_ms = self.max_ms.max(ms);
        let idx = LATENCY_BUCKETS_MS
            .iter()
            .position(|bound| ms <= *bound)
            .unwrap_or(LATENCY_BUCKETS_MS.len());
        self.bucket_counts[idx] += 1;
    }

    /// Conservative quantile estimate: the UPPER edge of the first
    /// bucket whose cumulative count reaches q x count (the overflow
    /// bucket reports the observed max). None when nothing was observed
    /// or q is not a probability.
    pub fn quantile_ms(&self, q: f64) -> Option<i64> {
        if self.count == 0 || !(0.0..=1.0).contains(&q) || !q.is_finite() {
            return None;
        }
        let target = (q * self.count as f64).ceil().max(1.0) as u64;
        let mut cumulative = 0u64;
        for (idx, n) in self.bucket_counts.iter().enumerate() {
            cumulative += n;
            if cumulative >= target {
                return Some(if idx < LATENCY_BUCKETS_MS.len() {
                    LATENCY_BUCKETS_MS[idx]
                } else {
                    self.max_ms
                });
            }
        }
        Some(self.max_ms)
    }
}

/// Cumulative run counters feeding the Section 8 metrics export.
#[derive(Debug, Default, Clone, Copy)]
pub struct RunCounters {
    pub ticks: u64,
    pub fills_applied: u64,
    pub orders_submitted: u64,
    pub gate_rejections: u64,
    pub veto_decisions: u64,
    pub veto_suppressed: u64,
    pub discrepancies: u64,
    pub settlement_notices: u64,
    /// Aggregated from strategy metrics at read time (synthesis cycles).
    pub cognition_failures: u64,
    pub shadow_cycles: u64,
    pub beliefs_drafted: u64,
    pub model_proposals_discarded: u64,
    /// Spec Section 8 "order/fill latency": submit->ack and
    /// submit->execution (fill timestamp minus submit time).
    pub ack_latency: LatencyStat,
    pub fill_latency: LatencyStat,
    /// Cycles degraded by a cost-budget breach (spec line 238: every
    /// breach alerts). Counted at the audit drain, once per degraded
    /// cycle.
    pub budget_breaches: u64,
    /// Venue API failures observed by the polling loops (outages on
    /// fills/settlements/positions). Section 8 "venue API error rates".
    pub venue_api_errors: u64,
    /// Settlement lifecycle outcomes (Section 8 lifecycle metrics).
    pub settlement_voids: u64,
    pub settlement_reversals: u64,
    /// Cognition spend accrued by COMPLETED decisions, merged from
    /// strategy metrics. Per-decision cost rides in belief provenance;
    /// the budget-true day total (including failed-call burn) is
    /// `mind_spend_today_cents`.
    pub cognition_cost_cents: i64,
    /// Budget-true mind spend today, summed across strategies (each
    /// synthesis strategy owns its mind; a shared-mind composition would
    /// double-count and must export per-strategy instead).
    pub mind_spend_today_cents: i64,
}

/// One strategy's day in numbers for the rich daily digest (S6b). Plain data;
/// the ops layer maps it to its DigestInputs and renders dollars. PnL + fees
/// are summed over the strategy's positions (the same market->strategy
/// attribution metrics_export uses); `fills` counts the strategy's FILLED
/// orders (Position carries no fill-event count); exposure is its reserved
/// capital.
#[derive(Debug, Clone)]
pub struct DigestStrategyRow {
    pub strategy: String,
    pub realized_pnl_cents: i64,
    pub fees_cents: i64,
    pub fills: u64,
    pub open_exposure_cents: i64,
}

/// The runner-side raw inputs for the rich daily digest (S6b). The daemon maps
/// this to `fortuna_ops::digest::DigestInputs` (the runner stays free of the
/// ops/telemetry layer). Honesty numbers (halts, discrepancies, overdue
/// settlements, capital in limbo) ride alongside per-strategy PnL so the digest
/// can never become a PnL-only marketing email.
#[derive(Debug, Clone)]
pub struct DigestSnapshot {
    pub strategies: Vec<DigestStrategyRow>,
    pub halts_active: u64,
    pub discrepancies_open: u64,
    pub settlements_overdue: u64,
    pub capital_in_limbo_cents: i64,
    pub veto_decisions: u64,
    pub veto_suppressed: u64,
}

/// One exported metric sample (plain data: the ops layer maps these into
/// its registry; the runner stays free of telemetry dependencies).
#[derive(Debug, Clone)]
pub struct MetricSample {
    pub name: &'static str,
    pub help: &'static str,
    pub counter: bool,
    pub labels: Vec<(String, String)>,
    pub value: i64,
}

/// Settlement-overdue grace beyond close_at + expected_lag_hours
/// (ASSUMPTIONS T1.4; becomes config at T1.5 alongside its alert routing).
const OVERDUE_GRACE_MS: i64 = 3_600_000;
/// Books-vs-venue mismatch must persist this many consecutive ticks before
/// it is a discrepancy (in-flight fills explain transient drift).
const MISMATCH_STREAK_LIMIT: u32 = 3;

impl SimRunner {
    /// The historical constructor: in-memory journal (Sim/DST default).
    pub fn new(
        config: RunnerConfig,
        strategies: Vec<Box<dyn Strategy>>,
        audit: Box<dyn AuditSink>,
        start: UtcTimestamp,
    ) -> Result<SimRunner, RunnerError> {
        // block_on is safe HERE only because MemoryJournal::recover never
        // touches IO; a journal that does (PgIntentJournal) must come in
        // through the async constructor or it deadlocks the runtime.
        futures::executor::block_on(SimRunner::new_with_journal(
            config,
            strategies,
            audit,
            start,
            MemoryJournal::default(),
        ))
    }
}

/// The stage-allowlist check (I7 at construction): every composed strategy's
/// stage must appear in `allowed`. The Sim default path passes `&[Stage::Sim]`
/// (refusing Paper/LiveMin/Scaled exactly as the pre-generalization inline
/// check did); the demo-flip's Kalshi path passes `&[Stage::Sim, Stage::Paper]`.
/// Extracted from the inline loop so the one rule serves both constructors.
fn check_stage_allowlist(
    strategies: &[Box<dyn Strategy>],
    allowed: &[Stage],
) -> Result<(), RunnerError> {
    for s in strategies {
        if !allowed.contains(&s.stage()) {
            return Err(RunnerError::StageViolation {
                strategy: s.id(),
                stage: s.stage(),
            });
        }
    }
    Ok(())
}

impl<J: IntentJournal + Send> SimRunner<SimVenue, J> {
    /// The daemon constructor (T4.1): same composition, caller-supplied
    /// durable journal. Journal-before-network is the exec contract;
    /// supplying `PgIntentJournal` makes the trail survive a crash.
    /// ASYNC because recovery reads the journal (real IO for Postgres) —
    /// a blocking wrapper here deadlocks tokio (learned the hard way).
    ///
    /// Builds the SimVenue + SimClock and delegates to `new_with_venue` with
    /// `&[Stage::Sim]` — ONE construction code path, so the sim composition is
    /// byte-identical to (and shares every invariant with) the injected-venue
    /// path (A3). The SimVenue catalog is pre-loaded HERE (`add_market` is
    /// SimVenue-specific); `new_with_venue` only mirrors `config.markets` into
    /// the runner's own `markets`/`market_meta`, never touching the venue.
    pub async fn new_with_journal(
        mut config: RunnerConfig,
        strategies: Vec<Box<dyn Strategy>>,
        audit: Box<dyn AuditSink>,
        start: UtcTimestamp,
        journal: J,
    ) -> Result<SimRunner<SimVenue, J>, RunnerError> {
        // The sim world is built from `config.faults` (DST arms); requiring
        // Some here keeps the injected-venue path (faults = None) from ever
        // silently constructing a faultless sim world by omission. Take it out
        // (the rest of `config` flows on to `new_with_venue` by value).
        let faults = config.faults.take().ok_or_else(|| RunnerError::Config {
            reason: "new_with_journal (SimVenue path) requires config.faults = Some(..)".into(),
        })?;
        let clock = Arc::new(SimClock::new(start));
        let venue = SimVenue::new(
            VenueId::new("sim").map_err(|e| RunnerError::Config {
                reason: e.to_string(),
            })?,
            clock.clone(),
            config.fee_model.clone(),
            faults,
            config.starting_cash,
        );
        for m in &config.markets {
            venue.add_market(m.clone());
        }
        SimRunner::new_with_venue(
            config,
            strategies,
            audit,
            start,
            journal,
            venue,
            clock,
            &[Stage::Sim],
        )
        .await
    }
}

impl<V: Venue + 'static, J: IntentJournal + Send> SimRunner<V, J> {
    /// The venue-injecting constructor (demo-flip Phase 1): takes a
    /// already-built venue + clock and the per-composition stage allowlist.
    /// `new_with_journal` routes through here with a SimVenue + SimClock +
    /// `&[Stage::Sim]`, so the sim path is byte-identical; the Kalshi demo path
    /// (Phase 2) passes a `KalshiVenue` + `&[Stage::Sim, Stage::Paper]`.
    ///
    /// Does NOT build a venue and does NOT read `config.faults` (faults are a
    /// SimVenue concept owned by `new_with_journal`). The runner's catalog
    /// (`markets`/`market_meta`) mirrors `config.markets`; for the Sim path the
    /// venue catalog was pre-loaded by `new_with_journal`, for Kalshi the
    /// venue's own `markets()` polls the live API in `tick()`.
    #[allow(clippy::too_many_arguments)]
    pub async fn new_with_venue(
        config: RunnerConfig,
        strategies: Vec<Box<dyn Strategy>>,
        audit: Box<dyn AuditSink>,
        start: UtcTimestamp,
        journal: J,
        venue: V,
        clock: Arc<SimClock>,
        allowed_stages: &[Stage],
    ) -> Result<SimRunner<V, J>, RunnerError> {
        let _ = start; // the clock already carries `start`; kept for symmetry.
                       // I7 at construction: a runner accepts only allowed-stage strategies.
        check_stage_allowlist(&strategies, allowed_stages)?;
        // A veto-enrolled strategy with no mind configured must not boot:
        // the veto is part of the strategy's spec'd shape (Section 6), and
        // skipping it silently would un-measure the thing being measured.
        if !config.veto_strategies.is_empty() && config.veto_mind.is_none() {
            return Err(RunnerError::Config {
                reason: format!(
                    "strategies {:?} are veto-enrolled but no veto mind is configured",
                    config
                        .veto_strategies
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                ),
            });
        }
        let mut market_ids = Vec::new();
        let mut market_meta = BTreeMap::new();
        for m in &config.markets {
            market_ids.push(m.id.clone());
            market_meta.insert(m.id.clone(), m.clone());
        }
        let manager = OrderManager::recover(journal, clock.clone(), config.exec_policy).await?;
        // Fail closed on a nonsensical base fraction: NaN/negative/>1
        // collapses to 0.0 (synthesis sizes nothing) rather than erroring
        // the whole composition.
        let config_kelly_fraction = if config.kelly_fraction.is_finite() {
            config.kelly_fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        Ok(SimRunner {
            bus: EventBus::new(clock.clone()),
            venue,
            gates: GatePipeline::new(config.gate_config)?,
            manager,
            positions: PositionBook::new(),
            reservations: ReservationLedger::new(config.envelopes.clone()),
            drawdown: DrawdownMonitor::new(config.max_daily_loss),
            strategies,
            audit,
            groups: GroupTracker::default(),
            fee_model: config.fee_model,
            mark_policy: config.mark_policy,
            books: BTreeMap::new(),
            markets: market_ids,
            envelope_names: config.envelopes.keys().cloned().collect(),
            ids: IdGen::new(config.seed),
            cursor: Cursor::start(),
            seen_events: 0,
            max_sets_per_proposal: config.max_sets_per_proposal,
            clock,
            audit_dead: false,
            pending_beliefs: Vec::new(),
            pending_scalar_beliefs: Vec::new(),
            veto_mind: config.veto_mind,
            veto_strategies: config.veto_strategies.into_iter().collect(),
            open_vetoes: Vec::new(),
            market_meta,
            settlements: SettlementLedger::new(),
            settle_cursor: Cursor::start(),
            seen_notices: std::collections::BTreeSet::new(),
            settled_snapshots: BTreeMap::new(),
            scored_vetoes: BTreeMap::new(),
            overdue_alerted: std::collections::BTreeSet::new(),
            dispute_frozen: std::collections::BTreeSet::new(),
            mismatch_streak: BTreeMap::new(),
            canonical_resolutions: BTreeMap::new(),
            calibration_quality: BTreeMap::new(),
            kelly_fraction: config_kelly_fraction,
            submit_times: BTreeMap::new(),
            gate_rejections_by_check: BTreeMap::new(),
            position_coverage: None,
            orphan_flagged: std::collections::BTreeSet::new(),
            seen_orphan_fills: std::collections::BTreeSet::new(),
            seen_external_positions: std::collections::BTreeSet::new(),
            counters: RunCounters::default(),
        })
    }

    /// Composition feed (events pipeline): the canonical resolution for a
    /// market's mapped event, with the edge id whose confidence takes the
    /// documented hit on divergence (spec 5.13).
    pub fn set_canonical_resolution(&mut self, market: MarketId, outcome: Side, edge: &str) {
        self.canonical_resolutions
            .insert(market, (outcome, edge.to_string()));
    }

    /// Composition feed (cognition + strategy registry): markets whose
    /// open positions have a defined next processor — a fresh open belief
    /// or a mechanical owner. Wiring this ENABLES the orphan watchdog.
    pub fn set_position_coverage(&mut self, covered: std::collections::BTreeSet<MarketId>) {
        self.position_coverage = Some(covered);
    }

    /// Composition feed (T2.8): the strategy's calibration quality in
    /// [0,1] from its resolved record. Unwired => 0.0 => its synthesis
    /// proposals size ZERO (an unmeasured calibration earns no size).
    pub fn set_calibration_quality(&mut self, strategy: &str, quality: f64) {
        self.calibration_quality
            .insert(strategy.to_string(), quality);
    }

    /// Read-only view of the settlement-entry chains (tests, dashboards).
    pub fn settlements(&self) -> &SettlementLedger {
        &self.settlements
    }

    pub fn venue(&self) -> &V {
        &self.venue
    }

    pub fn gates(&self) -> &GatePipeline {
        &self.gates
    }

    pub fn gates_mut(&mut self) -> &mut GatePipeline {
        &mut self.gates
    }

    pub fn positions(&self) -> &PositionBook {
        &self.positions
    }

    /// Active reserved capital for a strategy (read-only; tests assert
    /// reservations release on abort paths — spec 5.14).
    pub fn reserved_total(&self, strategy: &str) -> Cents {
        self.reservations.active_total(strategy)
    }

    pub fn manager(&self) -> &OrderManager<J> {
        &self.manager
    }

    /// Apply a halt learned from OUTSIDE the composition (the daemon's
    /// durable halt-state poll, T4.1 req 5): sets the global gate halt
    /// and writes the audit row. Re-arms stay CLI-only out-of-band (I2)
    /// — nothing here or anywhere in the daemon clears a halt.
    pub fn apply_external_halt(&mut self, reason: &str) {
        self.gates
            .set_halt(HaltScope::Global, format!("halt poll: {reason}"));
        self.audit(
            "halt",
            None,
            serde_json::json!({ "source": "halt_poll", "reason": reason }),
        );
    }

    /// The active global halt reason, if any — a pure read accessor for the
    /// ROTA health view (the boards JSON exposes only the halt BOOL; the
    /// SYSTEM-HALTED takeover needs the reason string). Read-path only; no
    /// money-path effect.
    pub fn active_halt(&self) -> Option<String> {
        self.gates.halts().global_halted().map(|s| s.to_string())
    }

    /// Per-check gate rejection tallies (Section 8 "gate rejection counts by
    /// reason"): check name -> count, sorted by name. A pure read accessor for
    /// the ROTA gates view — the breakdown otherwise escapes only via the
    /// Prometheus text, which ROTA must not parse (design R2). The counts sum
    /// to `counters().gate_rejections`. Read-path only; no money-path effect.
    pub fn rejections_by_check(&self) -> std::collections::BTreeMap<String, u64> {
        self.gate_rejections_by_check.clone()
    }

    /// Take the belief drafts buffered since the last drain (req 6). The
    /// daemon persists them to BeliefsRepo (events upserted first for the
    /// FK); draining empties the buffer so a draft is persisted once.
    pub fn drain_pending_beliefs(
        &mut self,
    ) -> Vec<(StrategyId, fortuna_cognition::beliefs::BeliefDraft)> {
        std::mem::take(&mut self.pending_beliefs)
    }

    /// Take the SCALAR belief drafts buffered since the last drain (perp-
    /// strategies design §2.5). Parallel to `drain_pending_beliefs`; the
    /// daemon persists them to the `scalar_beliefs` path. Draining empties
    /// the buffer so a draft is persisted once.
    pub fn drain_pending_scalar_beliefs(
        &mut self,
    ) -> Vec<(
        StrategyId,
        fortuna_cognition::scalar_beliefs::ScalarBeliefDraft,
    )> {
        std::mem::take(&mut self.pending_scalar_beliefs)
    }

    /// Inject an external `PerpTick` onto the bus for the NEXT `tick()` to
    /// dispatch — the perp INGESTION seam (perp-strategies design §2.1/§3.2;
    /// slice 4b). The two perp strategies (`funding_forecast`, `perp_event_basis`)
    /// fire only on `PerpTick`s, and the deterministic `tick()` loop only sources
    /// `BookSnapshot`s from the venue — so a producer (a Sim soak, or the live
    /// kinetics feed) must hand perp data in HERE.
    ///
    /// The venue crate is bus-free, so the producer builds the perp-DOMAIN
    /// components (e.g. via `KineticsPerpObservation::from_ws_ticker`) and this is
    /// where the bus `PerpTick` is ASSEMBLED (the `venue` id is added). The event
    /// is `EventOrigin::External` (exactly like the `BookSnapshot`s `tick()`
    /// publishes), so it RECORDS and REPLAYS byte-stable — and crucially `tick()`
    /// itself is UNCHANGED, so the record/replay determinism contract and every
    /// existing DST recording (which never injects) are untouched.
    pub fn inject_perp_tick(
        &mut self,
        venue: VenueId,
        market: MarketId,
        marks: PerpMarks,
        funding: FundingObservation,
    ) {
        self.bus.publish_external(EventPayload::PerpTick {
            venue,
            market,
            marks,
            funding,
        });
    }

    /// Record an externally-raised ALERT (non-halting) on the audit
    /// trail — the daemon's degrade-scrape alerts ride this until Slack
    /// routing lands. Spec 8: every alert is also an audit row.
    pub fn apply_external_alert(&mut self, kind: &str, message: &str) {
        self.audit(
            "alert",
            None,
            serde_json::json!({ "source": "daemon", "kind": kind, "message": message }),
        );
    }

    /// The graceful-shutdown contract (T4.1 req 8; operator-BINDING:
    /// `fortuna stop` and the daemon's SIGTERM handler call exactly
    /// this). Cancels every WORKING order through the journaled path,
    /// releases reservations, and writes the final audit row. Idempotent:
    /// a second call finds nothing working. Unacked orders (submitted,
    /// no venue id yet) cannot be cancelled and are counted HONESTLY —
    /// that window belongs to boot reconciliation and venue TTLs.
    pub async fn shutdown(&mut self) -> Result<ShutdownReport, RunnerError> {
        let working: Vec<IntentId> = self
            .manager
            .intents()
            .iter()
            .filter(|(_, rec)| rec.status.is_working())
            .map(|(id, _)| **id)
            .collect();
        let mut report = ShutdownReport {
            working: working.len(),
            ..ShutdownReport::default()
        };
        for id in working {
            match self.manager.cancel_intent(id, &self.venue).await {
                Ok(CancelOutcome::Cancelled) => report.cancelled += 1,
                Ok(CancelOutcome::AlreadyGone) => report.already_gone += 1,
                Ok(CancelOutcome::Unknown) => report.unknown += 1,
                Err(ExecError::Transition { .. }) => report.unacked += 1,
                Err(_) => report.unknown += 1,
            }
            self.release_if_terminal(id)?;
        }
        self.audit(
            "daemon_shutdown",
            None,
            serde_json::json!({
                "working": report.working,
                "cancelled": report.cancelled,
                "already_gone": report.already_gone,
                "unknown": report.unknown,
                "unacked": report.unacked,
            }),
        );
        Ok(report)
    }

    pub fn recording(&self) -> &Recording {
        self.bus.recording()
    }

    /// Audit + tolerate-the-untolerable: an audit failure is a GLOBAL HALT
    /// (I5: no audit, no trading), recorded on the bus (which still works).
    fn audit(&mut self, kind: &str, ref_id: Option<&str>, payload: serde_json::Value) {
        if self.audit_dead {
            return;
        }
        if let Err(e) = self.audit.append(kind, ref_id, payload) {
            self.audit_dead = true;
            self.gates
                .set_halt(HaltScope::Global, format!("audit write failure: {e}"));
            self.bus.publish_external(EventPayload::Raw {
                kind: "audit_failure_halt".into(),
                data: serde_json::json!({ "error": e.to_string() }),
            });
        }
    }

    /// One deterministic cycle.
    pub async fn tick(&mut self) -> Result<TickReport, RunnerError> {
        let mut report = TickReport::default();
        self.counters.ticks += 1;

        // 0. External market-data refresh hook. Most venues are already
        // authoritative on `book()` and keep the default no-op. Paper-on-live
        // composites use this to pull live books/trades into local paper state
        // before the tick publishes BookSnapshot events.
        self.venue
            .refresh_market_data_for_markets(&self.markets)
            .await?;

        // 0. Catalog refresh: venue lifecycle statuses are watchdog inputs
        // (dispute freezes, overdue clocks) and gate book fetches below. We
        // request ALL statuses (default filter), NOT Trading-only: a Disputed /
        // Settled / Voided market must still enter `market_meta` or the watchdog
        // can never see it (a Trading-only filter silently starves the
        // dispute-freeze + overdue + terminal-skip paths). The KalshiVenue scopes
        // `markets()` to the configured series, so this stays bounded.
        // An outage here keeps the last-known catalog (point-in-time data).
        if let Ok(markets) = self.venue.markets(MarketFilter::default()).await {
            for m in markets {
                self.market_meta.insert(m.id.clone(), m);
            }
        }

        // Account visibility is the drawdown gate's prerequisite. Do it before
        // the book/fill burst so a venue read-limit miss fails closed before any
        // proposal can reach order placement.
        if self.check_drawdown().await? {
            report.halted = true;
            return Ok(report);
        }

        // 1. Venue data enters the bus (point-in-time record). Terminal
        // markets (settled/voided) have no book; skip, don't spam errors.
        for market in self.markets.clone() {
            let terminal = matches!(
                self.market_meta.get(&market).map(|m| m.status),
                Some(MarketStatus::Settled) | Some(MarketStatus::Voided)
            );
            if terminal {
                continue;
            }
            match self.venue.book(&market).await {
                Ok(book) => {
                    self.books.insert(market.clone(), book.clone());
                    self.bus.publish_external(EventPayload::BookSnapshot {
                        venue: self.venue.id(),
                        book,
                    });
                }
                Err(e) => self.bus.publish_external(EventPayload::Raw {
                    kind: "book_error".into(),
                    data: serde_json::json!({ "market": market.to_string(), "error": e.to_string() }),
                }),
            }
        }
        self.bus.run_until_idle()?;

        // 2. Strategies see newly recorded events, in registration order.
        let new_events: Vec<fortuna_core::bus::BusEvent> = self
            .bus
            .recording()
            .events()
            .iter()
            .skip(self.seen_events)
            .cloned()
            .collect();
        self.seen_events = self.bus.recording().events().len();
        report.events_dispatched = new_events.len();

        let mut proposals: Vec<(StrategyId, Proposal)> = Vec::new();
        for ev in &new_events {
            let core = CoreHandle {
                now: self.clock.now(),
                books: &self.books,
                markets: &self.market_meta,
                fee_model: &self.fee_model,
            };
            for strategy in &mut self.strategies {
                let out = strategy.on_event(ev, &core).await?;
                for p in out {
                    proposals.push((strategy.id(), p));
                }
            }
        }
        report.proposals = proposals.len();

        // 2b. Drain degraded-cognition events (F1, spec line 238: degrade
        // ALERTS): every degraded cycle becomes an audit row and a bus
        // event; budget breaches additionally count for the every-breach
        // ops alert rule.
        let mut degrades: Vec<(StrategyId, crate::DegradeRecord)> = Vec::new();
        for strategy in &mut self.strategies {
            let id = strategy.id();
            for record in strategy.drain_degrades() {
                degrades.push((id.clone(), record));
            }
            // Belief drafts (req 6): buffered on the runner for the
            // composition to persist (the runner is Pg-agnostic; the
            // daemon owns BeliefsRepo). A draft per (strategy, draft).
            for belief in strategy.drain_beliefs() {
                self.pending_beliefs.push((id.clone(), belief));
            }
            // Scalar belief drafts (perp-strategies design §2.5): the
            // parallel additive buffer, drained beside the binary path.
            for scalar in strategy.drain_scalar_beliefs() {
                self.pending_scalar_beliefs.push((id.clone(), scalar));
            }
        }
        for (strategy_id, record) in degrades {
            if record.degrade == "budget_exhausted" {
                self.counters.budget_breaches += 1;
            }
            let mut payload = serde_json::json!({
                "strategy": strategy_id.to_string(),
                "event_id": record.event_id,
                "degrade": record.degrade,
            });
            if let (Some(obj), Some(extra)) = (payload.as_object_mut(), record.detail.as_object()) {
                for (k, v) in extra {
                    obj.insert(k.clone(), v.clone());
                }
            }
            self.audit("cognition", Some(&record.event_id), payload);
            self.bus.publish_external(EventPayload::Raw {
                kind: "cognition_degrade".into(),
                data: serde_json::json!({
                    "strategy": strategy_id.to_string(),
                    "degrade": record.degrade,
                }),
            });
        }

        // 3. Size -> gate -> submit.
        for (strategy_id, proposal) in proposals {
            self.handle_proposal(strategy_id, proposal, &mut report)
                .await?;
        }

        // 4. Fills -> positions/reservations -> bus.
        self.drain_fills(&mut report).await?;

        // 4b. Settlement notices -> entry chains -> books (spec 5.13:
        // reconcile, never assume), then the lifecycle watchdogs.
        self.process_settlements().await?;
        self.run_watchdogs().await?;

        // 5. Group policy evaluation -> complete-or-unwind decisions.
        let decisions = self.groups.evaluate(&self.manager, self.clock.now());
        report.group_decisions = decisions.len();
        for decision in decisions {
            self.handle_group_decision(decision, &mut report).await?;
        }

        // 6. TTL sweep (re-quotes come from strategies re-proposing).
        let swept = self.manager.sweep_ttl(&self.venue).await?;
        for intent in swept {
            self.release_if_terminal(intent)?;
            self.audit(
                "order",
                Some(&intent.to_string()),
                serde_json::json!({ "action": "ttl_cancel" }),
            );
        }

        report.halted = self.gates.halts().global_halted().is_some();
        Ok(report)
    }

    async fn handle_proposal(
        &mut self,
        strategy_id: StrategyId,
        proposal: Proposal,
        report: &mut TickReport,
    ) -> Result<(), RunnerError> {
        if proposal.legs.is_empty() {
            return Ok(());
        }
        // Decision provenance (spec 5.7): every proposal audits with the
        // context-manifest hash of the cycle that produced it (synthesis)
        // so the decision is replayable; mechanical scans audit None.
        self.audit(
            "proposal",
            None,
            serde_json::json!({
                "strategy": strategy_id.to_string(),
                "legs": proposal.legs.len(),
                "urgency": format!("{:?}", proposal.urgency),
                "thesis": proposal.thesis,
                "manifest_hash": proposal.manifest_hash,
            }),
        );
        // Sizing (the harness's job): all-in cost per 1-contract set, then
        // envelope headroom / caps decide the count.
        let mut cost_per_set = Cents::ZERO;
        for leg in &proposal.legs {
            let fee = self
                .fee_model
                .fee(
                    fortuna_core::book::FillRole::Taker,
                    leg.limit_price,
                    Contracts::new(1),
                    None,
                    self.clock.now(),
                )
                .map_err(|e| RunnerError::Config {
                    reason: format!("fee in sizing: {e}"),
                })?
                .max(Cents::ZERO);
            cost_per_set = cost_per_set
                .checked_add(leg.limit_price)
                .and_then(|c| c.checked_add(fee))?;
        }
        let headroom = self
            .reservations
            .headroom(strategy_id.as_str())
            .unwrap_or(Cents::ZERO);
        let mut sets = affordable_sets(headroom, cost_per_set, self.max_sets_per_proposal);

        // E1 (spec 5.8 + line 240): synthesis legs carry a calibrated
        // win-probability; their size is haircut-Kelly BOUNDED BY
        // affordability — contracts = min(kelly, affordable). Fraction =
        // base kelly_fraction x calibration quality; an unwired or zero
        // quality, or any invalid Kelly input, fails CLOSED to zero —
        // never full envelope headroom.
        if let Some(p) = proposal.legs.first().and_then(|l| l.calibrated_p) {
            let quality = self
                .calibration_quality
                .get(strategy_id.as_str())
                .copied()
                .unwrap_or(0.0);
            let fraction = haircut_kelly_fraction(self.kelly_fraction, quality);
            let price = proposal.legs[0].limit_price;
            let kelly = kelly_contracts(
                p,
                price,
                fraction,
                headroom,
                cost_per_set,
                self.max_sets_per_proposal,
            )
            .unwrap_or(0);
            self.audit(
                "sizing",
                None,
                serde_json::json!({
                    "strategy": strategy_id.to_string(),
                    "mode": "haircut_kelly",
                    "calibrated_p": p,
                    "quality": quality,
                    "fraction": fraction,
                    "kelly_contracts": kelly,
                    "affordable_contracts": sets,
                }),
            );
            sets = sets.min(kelly);
        }
        if sets == 0 {
            self.audit(
                "gate_decision",
                None,
                serde_json::json!({
                    "strategy": strategy_id.to_string(),
                    "verdict": "unsized",
                    "reason": format!("no envelope headroom ({headroom}) for set cost {cost_per_set}"),
                }),
            );
            return Ok(());
        }

        // Model veto (spec Section 6): reduce-only, audited, and strictly
        // BEFORE the gates — the gates never consult the model (I1); the
        // model never sees the gates. Only enrolled strategies pay it.
        if self.veto_strategies.contains(&strategy_id) {
            sets = self.consult_veto(&strategy_id, &proposal, sets).await?;
            if sets == 0 {
                return Ok(());
            }
        }

        let group = if proposal.legs.len() > 1 {
            Some(IntentGroupId::new(self.ids.next(self.clock.now())?))
        } else {
            None
        };

        // Phase A (sequential, deterministic): gate every leg and reserve
        // its capital BEFORE any network call. The phase split upgrades
        // all-or-nothing: ANY pre-submission gate rejection on a
        // multi-leg group aborts the WHOLE group (reservations released)
        // — the old interleaved path could strand an imbalance for the
        // unwind machinery when a later leg rejected after an earlier
        // one had already submitted.
        let mut staged: Vec<(IntentId, fortuna_gates::GatedOrder)> = Vec::new();
        let mut group_rejected = false;
        for leg in &proposal.legs {
            let intent = IntentId::new(self.ids.next(self.clock.now())?);
            let candidate = CandidateOrder {
                intent_id: intent,
                strategy: strategy_id.clone(),
                venue: self.venue.id(),
                market: leg.market.clone(),
                side: leg.side,
                action: leg.action,
                limit_price: leg.limit_price,
                qty: Contracts::new(sets),
                fair_value: leg.fair_value,
                client_order_id: ClientOrderId::from_intent(intent),
            };
            let outcome = self.evaluate_gates(&candidate);
            for record in &outcome.records {
                self.audit(
                    "gate_decision",
                    Some(&intent.to_string()),
                    serde_json::to_value(record).unwrap_or_default(),
                );
            }
            // I5 hard stop (gate finding, 2026-06-11): if the audit store
            // died while RECORDING these gate decisions, nothing staged may
            // reach the venue — the probe that found this had three orders
            // placed after the store died, their trail lost. The halt set
            // by audit() only bites the NEXT tick's gate evaluations; this
            // tick's already-gated legs must be aborted here.
            if self.audit_dead {
                for (prior, _) in &staged {
                    let _ = self.reservations.release(*prior)?;
                }
                return Ok(());
            }
            match outcome.gated {
                Err(rejection) => {
                    report.gate_rejections += 1;
                    self.counters.gate_rejections += 1;
                    *self
                        .gate_rejections_by_check
                        .entry(format!("{:?}", rejection.check))
                        .or_insert(0) += 1;
                    self.bus.publish_external(EventPayload::Raw {
                        kind: "gate_reject".into(),
                        data: serde_json::json!({
                            "intent": intent.to_string(),
                            "check": format!("{:?}", rejection.check),
                            "reason": rejection.reason,
                        }),
                    });
                    if group.is_some() {
                        group_rejected = true;
                        break;
                    }
                }
                Ok(gated) => {
                    // Reserve BEFORE submission (spec 5.14: reserve at gate
                    // time); exact amount released on terminal states.
                    let leg_cost = notional_plus_worst_fee(
                        &self.fee_model,
                        leg.limit_price,
                        sets,
                        self.clock.now(),
                    )?;
                    self.reservations
                        .reserve(strategy_id.as_str(), intent, leg_cost)?;
                    staged.push((intent, gated));
                }
            }
        }
        if group_rejected {
            // All-or-nothing: nothing was submitted; release what was
            // reserved and walk away clean.
            for (intent, _) in &staged {
                let _ = self.reservations.release(*intent)?;
            }
            return Ok(());
        }

        // Belt for the same I5 stop: no staged leg crosses into Phase B
        // if the audit store is dead, whatever path killed it.
        if self.audit_dead {
            for (prior, _) in &staged {
                let _ = self.reservations.release(*prior)?;
            }
            return Ok(());
        }

        // Phase B (concurrent): all legs hit the venue together — a
        // multi-leg entry costs ~1 venue RTT instead of N sequential
        // RTTs. Outcomes come back in LEG ORDER (deterministic
        // processing over concurrent IO).
        let mut group_legs: Vec<IntentId> = Vec::new();
        if !staged.is_empty() {
            let submitted_at_ms = self.clock.now().epoch_millis();
            let intents: Vec<IntentId> = staged.iter().map(|(i, _)| *i).collect();
            let orders: Vec<fortuna_gates::GatedOrder> =
                staged.into_iter().map(|(_, o)| o).collect();
            let outcomes = self
                .manager
                .submit_group_concurrent(orders, group, &self.venue)
                .await?;

            // Phase C (sequential, leg order): account each outcome.
            for (intent, leg_outcome) in intents.into_iter().zip(outcomes) {
                match leg_outcome {
                    LegOutcome::Submitted(SubmitOutcome::Acked { .. }) => {
                        report.orders_submitted += 1;
                        self.counters.orders_submitted += 1;
                        self.counters
                            .ack_latency
                            .observe(self.clock.now().epoch_millis() - submitted_at_ms);
                        self.submit_times.insert(
                            ClientOrderId::from_intent(intent).as_str().to_string(),
                            submitted_at_ms,
                        );
                        group_legs.push(intent);
                    }
                    LegOutcome::Submitted(SubmitOutcome::Rejected { reason }) => {
                        let _ = self.reservations.release(intent)?;
                        self.audit(
                            "order",
                            Some(&intent.to_string()),
                            serde_json::json!({ "venue_rejected": reason }),
                        );
                    }
                    LegOutcome::Submitted(SubmitOutcome::Unknown { error }) => {
                        // Reservation stays (the order may be live);
                        // reconciliation resolves it. Latency still
                        // measures from here if fills arrive.
                        report.orders_submitted += 1;
                        self.submit_times.insert(
                            ClientOrderId::from_intent(intent).as_str().to_string(),
                            submitted_at_ms,
                        );
                        group_legs.push(intent);
                        self.audit(
                            "order",
                            Some(&intent.to_string()),
                            serde_json::json!({ "submit_unknown": error }),
                        );
                    }
                    LegOutcome::NotSubmitted(ExecError::WorkingOrderExists { .. }) => {
                        let _ = self.reservations.release(intent)?;
                    }
                    LegOutcome::NotSubmitted(e) => return Err(e.into()),
                }
            }
        }

        if let (Some(gid), policy) = (group, proposal.group_policy) {
            if !group_legs.is_empty() {
                let policy = policy.ok_or(RunnerError::Config {
                    reason: "multi-leg proposal without group policy".into(),
                })?;
                self.groups.open(gid, policy, group_legs, self.clock.now());
            }
        }
        let _ = proposal.urgency; // v0: arbs go taker via crossing limits
        Ok(())
    }

    /// Assess one sized proposal with the veto mind. Returns the kept
    /// quantity (0 = suppressed). Reduce-only is structural: the verdict
    /// type cannot express growth, and this method only ever returns
    /// `min(kept, sets)` quantities derived from `KeepBps::apply`.
    ///
    /// Failure semantics (ASSUMPTIONS, T1.3): an unanswered veto fails
    /// CLOSED (suppress — within the veto's reduce-only authority, risking
    /// zero capital) but is flagged `veto_error` and NEVER counterfactually
    /// scored: a provider outage is not model judgment. Multi-leg
    /// proposals from veto-enrolled strategies are suppressed whole —
    /// partial-group vetoes would manufacture unhedged legs, and no
    /// spec'd strategy needs group vetoes this phase.
    async fn consult_veto(
        &mut self,
        strategy_id: &StrategyId,
        proposal: &Proposal,
        sets: i64,
    ) -> Result<i64, RunnerError> {
        let Some(mind) = self.veto_mind.clone() else {
            // Construction guard makes this unreachable; fail closed anyway.
            return Ok(0);
        };
        self.counters.veto_decisions += 1;
        if proposal.legs.len() != 1 {
            self.counters.veto_suppressed += 1;
            self.audit(
                "veto_decision",
                None,
                serde_json::json!({
                    "strategy": strategy_id.to_string(),
                    "veto_unsupported_multileg": true,
                    "legs": proposal.legs.len(),
                    "qty_before": sets,
                    "qty_after": 0,
                    "veto_error": false,
                }),
            );
            self.bus.publish_external(EventPayload::Raw {
                kind: "veto_decision".into(),
                data: serde_json::json!({
                    "strategy": strategy_id.to_string(),
                    "suppressed_multileg": proposal.legs.len(),
                }),
            });
            return Ok(0);
        }
        let leg = &proposal.legs[0];
        let book = self.books.get(&leg.market);
        let candidate = VetoCandidate {
            strategy: strategy_id.clone(),
            market: leg.market.clone(),
            side: leg.side,
            action: leg.action,
            limit_price: leg.limit_price,
            fair_value: leg.fair_value,
            qty: Contracts::new(sets),
            yes_bid: book.and_then(|b| b.yes_bids.first()).map(|l| l.price),
            yes_ask: book.and_then(|b| b.yes_asks.first()).map(|l| l.price),
            category: self
                .market_meta
                .get(&leg.market)
                .map(|m| m.category.clone()),
            thesis: proposal.thesis.clone(),
            as_of: self.clock.now(),
        };
        match mind.assess(&candidate).await {
            Ok(assessment) => {
                let kept = match &assessment.verdict {
                    VetoVerdict::Allow => sets,
                    VetoVerdict::Shrink { keep, .. } => keep.apply(Contracts::new(sets)).raw(),
                    VetoVerdict::Suppress { .. } => 0,
                };
                if kept == 0 {
                    self.counters.veto_suppressed += 1;
                }
                self.audit(
                    "veto_decision",
                    None,
                    serde_json::json!({
                        "candidate": serde_json::to_value(&candidate).unwrap_or_default(),
                        "assessment": serde_json::to_value(&assessment).unwrap_or_default(),
                        "qty_before": sets,
                        "qty_after": kept,
                        "veto_error": false,
                    }),
                );
                if kept < sets {
                    self.bus.publish_external(EventPayload::Raw {
                        kind: "veto_decision".into(),
                        data: serde_json::json!({
                            "market": leg.market.to_string(),
                            "qty_before": sets,
                            "qty_after": kept,
                        }),
                    });
                    self.open_vetoes.push(OpenVeto {
                        candidate,
                        removed: Contracts::new(sets - kept),
                    });
                }
                Ok(kept)
            }
            Err(e) => {
                self.counters.veto_suppressed += 1;
                self.audit(
                    "veto_decision",
                    None,
                    serde_json::json!({
                        "candidate": serde_json::to_value(&candidate).unwrap_or_default(),
                        "error": e.to_string(),
                        "qty_before": sets,
                        "qty_after": 0,
                        "veto_error": true,
                    }),
                );
                self.bus.publish_external(EventPayload::Raw {
                    kind: "veto_decision".into(),
                    data: serde_json::json!({
                        "market": leg.market.to_string(),
                        "veto_error": e.to_string(),
                    }),
                });
                Ok(0)
            }
        }
    }

    fn evaluate_gates(&mut self, candidate: &CandidateOrder) -> fortuna_gates::GateOutcome {
        // Exposure views: worst case = active reservations + open cost
        // basis (long binary risk = premium paid). Spec 5.13/5.14.
        let reserved_total: Cents = {
            let mut sum = Cents::ZERO;
            for s in &self.envelope_names {
                sum = sum
                    .checked_add(self.reservations.active_total(s))
                    .unwrap_or(sum);
            }
            sum
        };
        let positions_cost: Cents = self
            .positions
            .positions()
            .map(|p| {
                p.yes
                    .cost_basis
                    .checked_add(p.no.cost_basis)
                    .unwrap_or(Cents::ZERO)
            })
            .fold(Cents::ZERO, |acc, c| acc.checked_add(c).unwrap_or(acc));
        let open_exposure = reserved_total
            .checked_add(positions_cost)
            .unwrap_or(reserved_total);
        let market_exposure = self
            .positions
            .position(&candidate.market)
            .map(|p| {
                p.yes
                    .cost_basis
                    .checked_add(p.no.cost_basis)
                    .unwrap_or(Cents::ZERO)
            })
            .unwrap_or(Cents::ZERO);
        let strategy_exposure = self.reservations.active_total(candidate.strategy.as_str());
        let recent = self.manager.known_client_order_ids();
        let own_resting: Vec<fortuna_gates::RestingOrderView> = self
            .manager
            .intents()
            .iter()
            .filter(|(_, r)| r.status.is_working())
            .map(|(_, r)| fortuna_gates::RestingOrderView {
                market: r.order.market.clone(),
                side: r.order.side,
                action: r.order.action,
                price: r.order.limit_price,
            })
            .collect();
        let inputs = GateInputs {
            now: self.clock.now(),
            open_exposure_cents: open_exposure,
            market_exposure_cents: market_exposure,
            strategy_exposure_cents: strategy_exposure,
            event_exposure_cents: Cents::ZERO,
            event_id: None,
            book: self.books.get(&candidate.market),
            last_trade_price: None,
            fee_model: &self.fee_model,
            category: None,
            own_resting: &own_resting,
            recent_client_order_ids: &recent,
        };
        self.gates.evaluate(candidate, &inputs)
    }

    async fn drain_fills(&mut self, report: &mut TickReport) -> Result<(), RunnerError> {
        for _ in 0..1_000 {
            let page = match self.venue.fills_since(self.cursor.clone()).await {
                Ok(p) => p,
                Err(
                    fortuna_venues::VenueError::Outage { .. }
                    | fortuna_venues::VenueError::RateLimited
                    | fortuna_venues::VenueError::Timeout { .. },
                ) => {
                    self.counters.venue_api_errors += 1;
                    break; // next tick
                }
                Err(e) => return Err(e.into()),
            };
            let advanced = page.next_cursor != self.cursor;
            for fill in &page.fills {
                let app = match self.manager.ingest_fill(fill).await {
                    Ok(app) => app,
                    Err(ExecError::OrphanFill {
                        fill_id,
                        client_order_id,
                    }) => {
                        if self.seen_orphan_fills.insert(fill_id.clone()) {
                            self.record_discrepancy(
                                "orphan_fill",
                                serde_json::json!({
                                    "fill_id": fill_id,
                                    "client_order_id": client_order_id,
                                    "market": fill.market.to_string(),
                                    "venue_order_id": fill.venue_order_id.to_string(),
                                }),
                            );
                        }
                        continue;
                    }
                    Err(e) => return Err(e.into()),
                };
                if app.applied {
                    if let Some(submitted_ms) = self.submit_times.get(fill.client_order_id.as_str())
                    {
                        self.counters
                            .fill_latency
                            .observe(fill.at.epoch_millis() - submitted_ms);
                    }
                    self.positions.apply_fill(fill)?;
                    report.fills_applied += 1;
                    self.counters.fills_applied += 1;
                    self.bus.publish_external(EventPayload::FillSeen {
                        venue: self.venue.id(),
                        fill: fill.clone(),
                    });
                    self.audit(
                        "fill",
                        Some(&fill.fill_id),
                        serde_json::to_value(fill).unwrap_or_default(),
                    );
                    self.release_if_terminal(app.intent)?;
                }
            }
            self.cursor = page.next_cursor;
            if !advanced {
                break;
            }
        }
        self.bus.run_until_idle()?;
        self.seen_events = self.bus.recording().events().len();
        Ok(())
    }

    fn release_if_terminal(&mut self, intent: IntentId) -> Result<(), RunnerError> {
        let terminal = self
            .manager
            .intent(intent)
            .map(|r| {
                matches!(
                    r.status,
                    IntentStatus::Filled
                        | IntentStatus::Cancelled
                        | IntentStatus::Rejected
                        | IntentStatus::BootClosed
                )
            })
            .unwrap_or(false);
        if terminal {
            let _ = self.reservations.release(intent)?;
            self.submit_times
                .remove(ClientOrderId::from_intent(intent).as_str());
        }
        Ok(())
    }

    async fn check_drawdown(&mut self) -> Result<bool, RunnerError> {
        // Equity = venue cash (total incl. reserved) + conservative marks.
        let (cash, _reserved) = match self.venue.account().await {
            Ok(account) => account,
            Err(
                e @ (fortuna_venues::VenueError::Outage { .. }
                | fortuna_venues::VenueError::RateLimited
                | fortuna_venues::VenueError::Timeout { .. }),
            ) => {
                self.counters.venue_api_errors += 1;
                if self.gates.halts().global_halted().is_none() {
                    let reason = format!("venue account unavailable during drawdown check: {e}");
                    self.gates.set_halt(HaltScope::Global, reason.clone());
                    self.audit(
                        "halt",
                        None,
                        serde_json::json!({
                            "scope": "global",
                            "reason": reason,
                        }),
                    );
                    self.bus.publish_external(EventPayload::Raw {
                        kind: "halt".into(),
                        data: serde_json::json!({ "reason": "venue_account_unavailable" }),
                    });
                }
                return Ok(true);
            }
            Err(e) => return Err(e.into()),
        };
        let mut marks = Cents::ZERO;
        for p in self.positions.positions() {
            let mark = mark_lots(
                p.yes.qty,
                p.no.qty,
                self.books.get(&p.market),
                self.clock.now(),
                &self.mark_policy,
            )?;
            marks = marks.checked_add(mark.value)?;
        }
        let equity = cash.checked_add(marks)?;
        if let DrawdownVerdict::Breach { loss, limit } =
            self.drawdown.check(self.clock.now(), equity)?
        {
            if self.gates.halts().global_halted().is_none() {
                self.gates.set_halt(
                    HaltScope::Global,
                    format!("drawdown breach: loss {loss} >= limit {limit}"),
                );
                self.audit(
                    "halt",
                    None,
                    serde_json::json!({
                        "scope": "global",
                        "reason": format!("drawdown breach: loss {loss} >= limit {limit}"),
                    }),
                );
                self.bus.publish_external(EventPayload::Raw {
                    kind: "halt".into(),
                    data: serde_json::json!({ "reason": "drawdown" }),
                });
            }
        }
        Ok(false)
    }

    async fn handle_group_decision(
        &mut self,
        decision: GroupDecision,
        report: &mut TickReport,
    ) -> Result<(), RunnerError> {
        match decision {
            GroupDecision::Complete { group } => {
                self.groups.mark_closed(group);
                self.audit(
                    "order",
                    None,
                    serde_json::json!({ "group": group.to_string(), "group_complete": true }),
                );
                Ok(())
            }
            GroupDecision::Breached {
                group,
                reason,
                unfilled_legs,
            } => {
                self.audit(
                    "order",
                    None,
                    serde_json::json!({
                        "group": group.to_string(),
                        "group_breached": reason,
                    }),
                );
                // Deterministic complete-or-unwind over current books.
                let remaining: Vec<RemainingLeg> = unfilled_legs
                    .iter()
                    .filter_map(|leg| self.manager.intent(*leg))
                    .map(|r| RemainingLeg {
                        market: r.order.market.clone(),
                        side: r.order.side,
                        action: r.order.action,
                        remaining: Contracts::new(r.order.qty.raw() - r.cum_filled.raw()),
                    })
                    .collect();
                let policy = match self.groups.group(group) {
                    Some(g) => g.policy.clone(),
                    None => return Ok(()),
                };
                let verdict = decide_complete_or_unwind(
                    &remaining,
                    &self.books,
                    &self.fee_model,
                    &policy,
                    self.clock.now(),
                );
                match verdict {
                    CompleteOrUnwind::TakerComplete {
                        est_cost,
                        net_edge_bps,
                    } => {
                        // Completion orders go BACK through the gates as
                        // fresh candidates (I1 applies to recovery too).
                        self.audit(
                            "order",
                            None,
                            serde_json::json!({
                                "group": group.to_string(),
                                "taker_complete": { "est_cost": est_cost.raw(), "bps": net_edge_bps },
                            }),
                        );
                        // v0: cancel stale resting legs; strategies re-propose
                        // against fresh books (the scan fires again).
                        for leg in unfilled_legs {
                            let _ = self.manager.cancel_intent(leg, &self.venue).await;
                            self.release_if_terminal(leg)?;
                        }
                        self.groups.mark_closed(group);
                    }
                    CompleteOrUnwind::Unwind { reason } => {
                        // v0 unwind = freeze the group: cancel unfilled legs,
                        // HOLD filled lots (panic-selling thin books is the
                        // flatten planner's veto philosophy), alert loudly.
                        for leg in unfilled_legs {
                            let _ = self.manager.cancel_intent(leg, &self.venue).await;
                            self.release_if_terminal(leg)?;
                        }
                        self.groups.mark_unwinding(group);
                        self.audit(
                            "order",
                            None,
                            serde_json::json!({
                                "group": group.to_string(),
                                "unwind_frozen": reason,
                            }),
                        );
                        self.bus.publish_external(EventPayload::Raw {
                            kind: "group_unwind_frozen".into(),
                            data: serde_json::json!({ "group": group.to_string() }),
                        });
                    }
                }
                report.group_decisions += 1;
                Ok(())
            }
        }
    }

    /// Final accounting for reports/tests. ASYNC since the demo-flip: cash now
    /// comes from the generic `Venue::account()` (an `async` trait method) so
    /// the report works over any venue, not just the synchronous SimVenue read.
    pub async fn report(&self) -> Result<RunnerReport, RunnerError> {
        let (cash, _reserved) = self.venue.account().await?;
        let mut realized = Cents::ZERO;
        let mut fees = Cents::ZERO;
        for p in self.positions.positions() {
            realized = realized.checked_add(p.realized_pnl)?;
            fees = fees.checked_add(p.fees_paid)?;
        }
        Ok(RunnerReport {
            recording_jsonl: self.bus.recording().to_jsonl()?,
            final_cash: cash,
            realized_pnl: realized,
            fees_paid: fees,
        })
    }

    /// Counterfactually score (or RE-score, on a venue correction) the
    /// vetoed-away quantity for one market against its settlement outcome.
    /// Drained entries move to `scored_vetoes` so a correction can re-emit
    /// corrected rows (append-only: the new row supersedes by reference).
    fn score_vetoes_at_settlement(
        &mut self,
        market: &MarketId,
        winner: fortuna_core::market::Side,
        correction: bool,
    ) {
        let mut due: Vec<OpenVeto> = if correction {
            self.scored_vetoes.remove(market).unwrap_or_default()
        } else {
            let drained = std::mem::take(&mut self.open_vetoes);
            let (due, keep): (Vec<OpenVeto>, Vec<OpenVeto>) = drained
                .into_iter()
                .partition(|v| &v.candidate.market == market);
            self.open_vetoes = keep;
            due
        };
        for v in &due {
            let payload = match counterfactual_pnl(
                &v.candidate,
                v.removed,
                winner,
                Cents::new(100),
                &self.fee_model,
                FillAssumption::FilledAtLimit,
            ) {
                Ok(pnl) => serde_json::json!({
                    "candidate": serde_json::to_value(&v.candidate).unwrap_or_default(),
                    "removed": v.removed.raw(),
                    "winner": format!("{winner:?}"),
                    "hypothetical_pnl_cents": pnl.raw(),
                    "fill_assumption": serde_json::to_value(FillAssumption::FilledAtLimit)
                        .unwrap_or_default(),
                    "correction": correction,
                }),
                Err(e) => serde_json::json!({
                    "candidate": serde_json::to_value(&v.candidate).unwrap_or_default(),
                    "removed": v.removed.raw(),
                    "winner": format!("{winner:?}"),
                    "score_error": e.to_string(),
                    "correction": correction,
                }),
            };
            self.audit("veto_counterfactual", Some(market.as_str()), payload);
        }
        if !due.is_empty() {
            self.scored_vetoes
                .entry(market.clone())
                .or_default()
                .append(&mut due);
        }
    }

    /// Vetoes on a VOIDED market are abandoned, scored neither right nor
    /// wrong (spec 5.13 belief-disposition discipline applied to the veto
    /// program: a voided market is the world breaking the question).
    fn abandon_vetoes_on_void(&mut self, market: &MarketId) {
        let drained = std::mem::take(&mut self.open_vetoes);
        let (due, keep): (Vec<OpenVeto>, Vec<OpenVeto>) = drained
            .into_iter()
            .partition(|v| &v.candidate.market == market);
        self.open_vetoes = keep;
        for v in &due {
            self.audit(
                "veto_abandoned",
                Some(market.as_str()),
                serde_json::json!({
                    "candidate": serde_json::to_value(&v.candidate).unwrap_or_default(),
                    "removed": v.removed.raw(),
                    "reason": "market voided",
                }),
            );
        }
    }

    /// Poll the venue's settlement-notice stream and reconcile every NEW
    /// notice into the entry chains and the position book (spec 5.13).
    async fn process_settlements(&mut self) -> Result<(), RunnerError> {
        for _ in 0..100 {
            let page = match self
                .venue
                .settlements_since(self.settle_cursor.clone())
                .await
            {
                Ok(p) => p,
                Err(
                    fortuna_venues::VenueError::Outage { .. }
                    | fortuna_venues::VenueError::RateLimited
                    | fortuna_venues::VenueError::Timeout { .. },
                ) => {
                    self.counters.venue_api_errors += 1;
                    break; // next tick
                }
                Err(e) => return Err(e.into()),
            };
            let advanced = page.next_cursor != self.settle_cursor;
            for notice in &page.notices {
                if !self.seen_notices.insert(notice.notice_id.clone()) {
                    continue; // at-least-once dedup
                }
                self.counters.settlement_notices += 1;
                self.apply_notice(notice).await?;
            }
            self.settle_cursor = page.next_cursor;
            if !advanced {
                break;
            }
        }
        self.bus.run_until_idle()?;
        self.seen_events = self.bus.recording().events().len();
        Ok(())
    }

    /// One notice into the books: fresh settlement, duplicate, correction,
    /// or void. Every branch audits; illegal chain transitions surface as
    /// errors (never coerced).
    async fn apply_notice(&mut self, notice: &SettlementNotice) -> Result<(), RunnerError> {
        let market = &notice.market;
        self.bus.publish_external(EventPayload::Raw {
            kind: "settlement_notice".into(),
            data: serde_json::json!({
                "notice_id": notice.notice_id,
                "market": market.to_string(),
                "outcome": serde_json::to_value(notice.outcome).unwrap_or_default(),
            }),
        });
        let head_status = self.settlements.head(market).map(|h| h.status);
        match notice.outcome {
            SettlementOutcome::Winner(winner) => {
                let already_applied = matches!(
                    head_status,
                    Some(SettlementStatus::Pending)
                        | Some(SettlementStatus::Posted)
                        | Some(SettlementStatus::Confirmed)
                );
                if already_applied {
                    let prior_winner = self
                        .settled_snapshots
                        .get(market)
                        .map(|_| ())
                        .and_then(|_| {
                            self.settlements
                                .chain(market)
                                .iter()
                                .rev()
                                .find_map(|e| e.detail.get("winner").cloned())
                        })
                        .and_then(|w| w.as_str().map(String::from));
                    let same = prior_winner.as_deref() == Some(&format!("{winner:?}"));
                    if same {
                        self.audit(
                            "settlement_duplicate",
                            Some(market.as_str()),
                            serde_json::json!({ "notice_id": notice.notice_id }),
                        );
                        return Ok(());
                    }
                    // CORRECTION: reverse the books exactly, then re-settle.
                    return self.apply_correction(market, winner, notice).await;
                }
                self.apply_fresh_settlement(market, winner, notice).await
            }
            SettlementOutcome::Voided => self.apply_void(market, notice).await,
        }
    }

    async fn apply_fresh_settlement(
        &mut self,
        market: &MarketId,
        winner: fortuna_core::market::Side,
        notice: &SettlementNotice,
    ) -> Result<(), RunnerError> {
        self.score_vetoes_at_settlement(market, winner, false);
        self.check_settlement_divergence(market, winner);
        let held = self
            .positions
            .position(market)
            .map(|p| p.yes.qty != 0 || p.no.qty != 0)
            .unwrap_or(false);
        if !held {
            self.audit(
                "settlement",
                Some(market.as_str()),
                serde_json::json!({
                    "winner": format!("{winner:?}"),
                    "owed": 0,
                    "held": false,
                    "notice_id": notice.notice_id,
                }),
            );
            return Ok(());
        }
        let snap = self.positions.settlement_snapshot(market)?;
        let expected = {
            let winning = match winner {
                fortuna_core::market::Side::Yes => snap.yes.qty,
                fortuna_core::market::Side::No => snap.no.qty,
            };
            Cents::new(100).checked_mul(winning.max(0))?
        };
        let pending_id = self.ids.next(self.clock.now())?.to_string();
        self.settlements.record_pending(
            pending_id,
            market.clone(),
            self.venue.id(),
            expected,
            serde_json::json!({
                "winner": format!("{winner:?}"),
                "notice_id": notice.notice_id,
                "venue_detail": notice.detail,
            }),
            self.clock.now(),
        )?;
        let owed = self
            .positions
            .apply_settlement(market, winner, Cents::new(100))?;
        self.settled_snapshots.insert(market.clone(), snap);
        let posted_id = self.ids.next(self.clock.now())?.to_string();
        self.settlements.advance(
            posted_id,
            market,
            SettlementStatus::Posted,
            self.clock.now(),
        )?;
        // Reconcile the venue-reported amount against our computation
        // (a mismatch is a discrepancy, never silently absorbed).
        if let Some(venue_paid) = notice
            .detail
            .get("paid_cents")
            .and_then(serde_json::Value::as_i64)
        {
            if venue_paid != owed.raw() {
                self.record_discrepancy(
                    "settlement_payout_mismatch",
                    serde_json::json!({
                        "market": market.to_string(),
                        "venue_paid_cents": venue_paid,
                        "computed_cents": owed.raw(),
                        "notice_id": notice.notice_id,
                    }),
                );
            }
        }
        self.bus.publish_external(EventPayload::Settled {
            venue: self.venue.id(),
            market: market.clone(),
            payout_cents: owed.raw(),
        });
        self.audit(
            "settlement",
            Some(market.as_str()),
            serde_json::json!({
                "winner": format!("{winner:?}"),
                "owed": owed.raw(),
                "held": true,
                "notice_id": notice.notice_id,
            }),
        );
        Ok(())
    }

    async fn apply_correction(
        &mut self,
        market: &MarketId,
        corrected_winner: fortuna_core::market::Side,
        notice: &SettlementNotice,
    ) -> Result<(), RunnerError> {
        self.counters.settlement_reversals += 1;
        // A corrected outcome re-checks against canonical truth: the
        // correction may introduce OR resolve a divergence.
        self.check_settlement_divergence(market, corrected_winner);
        let Some(snap) = self.settled_snapshots.get(market).cloned() else {
            // A correction for a settlement we never applied to the books
            // (held nothing). Score-correct the vetoes and record it.
            self.score_vetoes_at_settlement(market, corrected_winner, true);
            self.audit(
                "settlement_reversal",
                Some(market.as_str()),
                serde_json::json!({
                    "corrected_winner": format!("{corrected_winner:?}"),
                    "held": false,
                    "notice_id": notice.notice_id,
                }),
            );
            return Ok(());
        };
        let reverse_id = self.ids.next(self.clock.now())?.to_string();
        self.settlements.reverse(
            reverse_id,
            market,
            serde_json::json!({
                "corrected_winner": format!("{corrected_winner:?}"),
                "notice_id": notice.notice_id,
            }),
            self.clock.now(),
        )?;
        let clawback = self.positions.reverse_settlement(market, &snap)?;
        self.audit(
            "settlement_reversal",
            Some(market.as_str()),
            serde_json::json!({
                "clawback_cents": clawback.raw(),
                "corrected_winner": format!("{corrected_winner:?}"),
                "notice_id": notice.notice_id,
            }),
        );
        // Re-score the veto counterfactuals against the corrected truth.
        self.score_vetoes_at_settlement(market, corrected_winner, true);
        // Corrected re-settlement through the same fresh path (new pending
        // chain over the Reversed head).
        self.apply_fresh_settlement(market, corrected_winner, notice)
            .await
    }

    async fn apply_void(
        &mut self,
        market: &MarketId,
        notice: &SettlementNotice,
    ) -> Result<(), RunnerError> {
        self.counters.settlement_voids += 1;
        self.abandon_vetoes_on_void(market);
        let held = self
            .positions
            .position(market)
            .map(|p| p.yes.qty != 0 || p.no.qty != 0)
            .unwrap_or(false);
        if !held {
            self.audit(
                "settlement",
                Some(market.as_str()),
                serde_json::json!({
                    "voided": true,
                    "refund": 0,
                    "held": false,
                    "notice_id": notice.notice_id,
                }),
            );
            return Ok(());
        }
        let refund = self.positions.apply_void_refund(market)?;
        let pending_id = self.ids.next(self.clock.now())?.to_string();
        self.settlements.record_pending(
            pending_id,
            market.clone(),
            self.venue.id(),
            refund,
            serde_json::json!({
                "voided": true,
                "notice_id": notice.notice_id,
                "venue_detail": notice.detail,
            }),
            self.clock.now(),
        )?;
        let posted_id = self.ids.next(self.clock.now())?.to_string();
        self.settlements.advance(
            posted_id,
            market,
            SettlementStatus::Posted,
            self.clock.now(),
        )?;
        self.audit(
            "settlement",
            Some(market.as_str()),
            serde_json::json!({
                "voided": true,
                "refund": refund.raw(),
                "held": true,
                "notice_id": notice.notice_id,
            }),
        );
        Ok(())
    }

    /// An explicit books-vs-venue mismatch record (spec 5.13: no silent
    /// corrections; resolution is a matching entry, an adjustment with
    /// reason, or operator escalation — recorded by the ledger repos in
    /// the live composition).
    /// The 5.13 divergence detector: venue outcome vs canonical event
    /// criteria. PnL follows venue truth (the settlement applies
    /// normally); the divergence is RECORDED with both truths and the
    /// edge whose confidence takes the documented hit — the composition
    /// applies that hit as a superseding edge insert and the belief
    /// scores against canonical truth.
    fn check_settlement_divergence(&mut self, market: &MarketId, venue_outcome: Side) {
        let Some((canonical, edge)) = self.canonical_resolutions.get(market).cloned() else {
            return;
        };
        if canonical == venue_outcome {
            return;
        }
        let venue_str = match venue_outcome {
            Side::Yes => "yes",
            Side::No => "no",
        };
        let canon_str = match canonical {
            Side::Yes => "yes",
            Side::No => "no",
        };
        self.record_discrepancy(
            "settlement_divergence",
            serde_json::json!({
                "market": market.as_str(),
                "venue_outcome": venue_str,
                "canonical_outcome": canon_str,
                "edge": edge,
                "edge_confidence_hit":
                    "supersede the edge with reduced confidence; restrict to operator-confirmed use",
            }),
        );
    }

    fn record_discrepancy(&mut self, kind: &str, detail: serde_json::Value) {
        self.counters.discrepancies += 1;
        self.audit(
            "discrepancy",
            None,
            serde_json::json!({ "kind": kind, "detail": detail }),
        );
        self.bus.publish_external(EventPayload::Raw {
            kind: "discrepancy".into(),
            data: serde_json::json!({ "kind": kind }),
        });
    }

    /// Lifecycle watchdogs (spec 5.13): confirm posted entries against
    /// venue positions, overdue-settlement alerts, dispute freezes, and
    /// the persistent books-vs-venue mismatch detector. No orphans: every
    /// stranded state is surfaced and dispositioned, never discovered by
    /// accident.
    async fn run_watchdogs(&mut self) -> Result<(), RunnerError> {
        // PARTITIONED (E5): the venue-DEPENDENT checks (posted->confirmed,
        // books-vs-venue mismatch) skip during an outage and watch again
        // next tick; the venue-INDEPENDENT checks (overdue via clock +
        // last-known meta, dispute freeze on last-known meta, the orphan
        // scan over local books) run regardless — a dark venue must not
        // starve them.
        let venue_by_market: Option<BTreeMap<MarketId, (i64, i64)>> =
            match self.venue.positions().await {
                Ok(positions) => Some(
                    positions
                        .iter()
                        .map(|vp| (vp.market.clone(), (vp.yes, vp.no)))
                        .collect(),
                ),
                Err(_) => {
                    self.counters.venue_api_errors += 1;
                    None // outage: venue-dependent checks wait
                }
            };

        // Confirm step (venue-dependent): a Posted entry confirms when the
        // venue shows no residual position for the market.
        if let Some(venue_by_market) = &venue_by_market {
            let posted: Vec<MarketId> = self
                .settlements
                .markets()
                .filter(|m| {
                    self.settlements.head(m).map(|h| h.status) == Some(SettlementStatus::Posted)
                })
                .cloned()
                .collect();
            for market in posted {
                let residual = venue_by_market
                    .get(&market)
                    .map(|(y, n)| *y != 0 || *n != 0)
                    .unwrap_or(false);
                if !residual {
                    let id = self.ids.next(self.clock.now())?.to_string();
                    self.settlements.advance(
                        id,
                        &market,
                        SettlementStatus::Confirmed,
                        self.clock.now(),
                    )?;
                }
            }
        }

        let now_ms = self.clock.now().epoch_millis();
        let metas: Vec<Market> = self.market_meta.values().cloned().collect();
        for meta in metas {
            let market = meta.id.clone();
            let held = self
                .positions
                .position(&market)
                .map(|p| p.yes.qty != 0 || p.no.qty != 0)
                .unwrap_or(false);

            // Dispute freeze: a disputed market's position leaves bankroll
            // but REMAINS in exposure (reversal risk is real risk).
            if meta.status == MarketStatus::Disputed
                && held
                && self.dispute_frozen.insert(market.clone())
            {
                self.positions
                    .set_lifecycle(&market, PositionLifecycle::Disputed)?;
                self.audit(
                    "watchdog",
                    Some(market.as_str()),
                    serde_json::json!({ "kind": "dispute_freeze" }),
                );
                self.bus.publish_external(EventPayload::Raw {
                    kind: "dispute_freeze".into(),
                    data: serde_json::json!({ "market": market.to_string() }),
                });
            }

            // Settlement-overdue: close_at + expected lag + grace passed,
            // position still live, no settlement chain. Alert once.
            if held && self.settlements.head(&market).is_none() {
                if let Some(close) = meta.close_at {
                    let expected_ms = close.epoch_millis()
                        + i64::from(meta.settlement.expected_lag_hours) * 3_600_000
                        + OVERDUE_GRACE_MS;
                    if now_ms > expected_ms && self.overdue_alerted.insert(market.clone()) {
                        self.audit(
                            "watchdog",
                            Some(market.as_str()),
                            serde_json::json!({
                                "kind": "settlement_overdue",
                                "expected_by_epoch_ms": expected_ms,
                            }),
                        );
                        self.bus.publish_external(EventPayload::Raw {
                            kind: "settlement_overdue".into(),
                            data: serde_json::json!({ "market": market.to_string() }),
                        });
                    }
                }
            }
        }

        // Books-vs-venue position mismatch (venue-dependent): transient
        // drift is explained by in-flight fills; PERSISTENT drift is a
        // discrepancy and a GLOBAL HALT. During an outage the streak
        // neither advances nor clears — drift detection resumes on the
        // next successful poll.
        if let Some(venue_by_market) = &venue_by_market {
            let mut runner_markets: std::collections::BTreeSet<MarketId> =
                self.markets.iter().cloned().collect();
            for p in self.positions.positions() {
                runner_markets.insert(p.market.clone());
            }
            for (market, (vy, vn)) in venue_by_market {
                if !runner_markets.contains(market)
                    && (*vy != 0 || *vn != 0)
                    && self.seen_external_positions.insert(market.clone())
                {
                    self.record_discrepancy(
                        "external_venue_position",
                        serde_json::json!({
                            "market": market.to_string(),
                            "venue": { "yes": vy, "no": vn },
                        }),
                    );
                }
            }
            for market in runner_markets {
                let (vy, vn) = venue_by_market.get(&market).copied().unwrap_or((0, 0));
                let (by, bn) = self
                    .positions
                    .position(&market)
                    .map(|p| (p.yes.qty, p.no.qty))
                    .unwrap_or((0, 0));
                if (vy, vn) == (by, bn) {
                    self.mismatch_streak.remove(&market);
                    continue;
                }
                let streak = {
                    let e = self.mismatch_streak.entry(market.clone()).or_insert(0);
                    *e += 1;
                    *e
                };
                if streak == MISMATCH_STREAK_LIMIT {
                    self.record_discrepancy(
                        "position_mismatch",
                        serde_json::json!({
                            "market": market.to_string(),
                            "venue": { "yes": vy, "no": vn },
                            "books": { "yes": by, "no": bn },
                            "consecutive_ticks": streak,
                        }),
                    );
                    if self.gates.halts().global_halted().is_none() {
                        self.gates.set_halt(
                            HaltScope::Global,
                            format!("books-vs-venue position mismatch on {market}"),
                        );
                    }
                }
            }
        }

        // Stranded-state orphan watchdog (spec 5.13): an open position
        // with no fresh open belief and no mechanical owner is an orphan
        // — alert once, forced exit evaluation is the operator/composition
        // disposition. Runs only when the composition wired the coverage
        // view; silence here would otherwise be indistinguishable from
        // "everything covered".
        if let Some(covered) = self.position_coverage.clone() {
            let held_markets: Vec<MarketId> = self
                .markets
                .iter()
                .filter(|m| {
                    self.positions
                        .position(m)
                        .map(|p| p.yes.qty != 0 || p.no.qty != 0)
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
            for market in held_markets {
                if !covered.contains(&market) && self.orphan_flagged.insert(market.clone()) {
                    self.audit(
                        "watchdog",
                        Some(market.as_str()),
                        serde_json::json!({
                            "kind": "orphaned_position",
                            "market": market.as_str(),
                            "disposition": "forced exit evaluation (operator/composition)",
                        }),
                    );
                    self.bus.publish_external(EventPayload::Raw {
                        kind: "orphaned_position".into(),
                        data: serde_json::json!({ "market": market.as_str() }),
                    });
                }
            }
        }
        Ok(())
    }

    /// Section 8 metrics as plain samples (the ops layer maps them into
    /// its registry; the runner carries no telemetry dependency). Market
    /// PnL/fees attribute to the strategy that traded the market (exact
    /// under the one-working-order discipline; a market touched by
    /// multiple strategies labels "shared" rather than guessing).
    pub fn metrics_export(&self) -> Vec<MetricSample> {
        let mut samples = Vec::new();
        let c = self.counters();
        for (name, help, value) in [
            (
                "fortuna_cognition_cost_cents_total",
                "Cognition spend accrued by completed decisions (cents)",
                c.cognition_cost_cents,
            ),
            (
                "fortuna_order_ack_latency_ms_sum",
                "Total submit->ack latency (ms; mean = sum/count)",
                c.ack_latency.sum_ms,
            ),
            (
                "fortuna_fill_latency_ms_sum",
                "Total submit->execution latency (ms; mean = sum/count)",
                c.fill_latency.sum_ms,
            ),
        ] {
            samples.push(MetricSample {
                name,
                help,
                counter: true,
                labels: Vec::new(),
                value,
            });
        }
        for (name, help, value) in [
            (
                "fortuna_mind_spend_today_cents",
                "Budget-true mind spend today incl. failed-call burn (cents; resets 00:00 UTC)",
                c.mind_spend_today_cents,
            ),
            (
                "fortuna_order_ack_latency_ms_max",
                "Max submit->ack latency (ms)",
                c.ack_latency.max_ms,
            ),
            (
                "fortuna_fill_latency_ms_max",
                "Max submit->execution latency (ms)",
                c.fill_latency.max_ms,
            ),
        ] {
            samples.push(MetricSample {
                name,
                help,
                counter: false,
                labels: Vec::new(),
                value,
            });
        }
        // Histogram buckets (Prometheus convention: cumulative, le
        // labels, +Inf == count) and direct conservative percentile
        // gauges for the boards.
        for (base, stat) in [
            ("fortuna_order_ack_latency_ms", &c.ack_latency),
            ("fortuna_fill_latency_ms", &c.fill_latency),
        ] {
            let mut cumulative = 0u64;
            for (idx, bound) in LATENCY_BUCKETS_MS.iter().enumerate() {
                cumulative += stat.bucket_counts[idx];
                samples.push(MetricSample {
                    name: match base {
                        "fortuna_order_ack_latency_ms" => "fortuna_order_ack_latency_ms_bucket",
                        _ => "fortuna_fill_latency_ms_bucket",
                    },
                    help: "Latency histogram bucket (cumulative)",
                    counter: true,
                    labels: vec![("le".to_string(), bound.to_string())],
                    value: cumulative as i64,
                });
            }
            samples.push(MetricSample {
                name: match base {
                    "fortuna_order_ack_latency_ms" => "fortuna_order_ack_latency_ms_bucket",
                    _ => "fortuna_fill_latency_ms_bucket",
                },
                help: "Latency histogram bucket (cumulative)",
                counter: true,
                labels: vec![("le".to_string(), "+Inf".to_string())],
                value: stat.count as i64,
            });
            for (suffix, q) in [("_p90", 0.90), ("_p95", 0.95), ("_p99", 0.99)] {
                samples.push(MetricSample {
                    name: match (base, suffix) {
                        ("fortuna_order_ack_latency_ms", "_p90") => {
                            "fortuna_order_ack_latency_ms_p90"
                        }
                        ("fortuna_order_ack_latency_ms", "_p95") => {
                            "fortuna_order_ack_latency_ms_p95"
                        }
                        ("fortuna_order_ack_latency_ms", "_p99") => {
                            "fortuna_order_ack_latency_ms_p99"
                        }
                        ("fortuna_fill_latency_ms", "_p90") => "fortuna_fill_latency_ms_p90",
                        ("fortuna_fill_latency_ms", "_p95") => "fortuna_fill_latency_ms_p95",
                        (_, _) => "fortuna_fill_latency_ms_p99",
                    },
                    help: "Conservative percentile estimate (bucket upper edge)",
                    counter: false,
                    labels: Vec::new(),
                    value: stat.quantile_ms(q).unwrap_or(0),
                });
            }
        }
        for (name, help, value) in [
            ("fortuna_ticks_total", "Loop heartbeats", c.ticks),
            (
                "fortuna_fills_applied_total",
                "Fills applied to the books",
                c.fills_applied,
            ),
            (
                "fortuna_orders_submitted_total",
                "Orders acked by the venue",
                c.orders_submitted,
            ),
            (
                "fortuna_gate_rejections_total",
                "Gate pipeline rejections",
                c.gate_rejections,
            ),
            (
                "fortuna_veto_decisions_total",
                "Model veto consultations",
                c.veto_decisions,
            ),
            (
                "fortuna_veto_suppressed_total",
                "Veto suppressions (incl. errors)",
                c.veto_suppressed,
            ),
            (
                "fortuna_discrepancies_total",
                "Books-vs-venue discrepancy records",
                c.discrepancies,
            ),
            (
                "fortuna_settlement_notices_total",
                "Venue settlement notices processed",
                c.settlement_notices,
            ),
            (
                "fortuna_cognition_failures_total",
                "Decision cycles degraded by cognition failure",
                c.cognition_failures,
            ),
            (
                "fortuna_shadow_cycles_total",
                "Declined-trigger cycles run in shadow",
                c.shadow_cycles,
            ),
            (
                "fortuna_beliefs_drafted_total",
                "Belief drafts produced by minds",
                c.beliefs_drafted,
            ),
            (
                "fortuna_model_proposals_discarded_total",
                "Model ProposalDrafts discarded by the cycle",
                c.model_proposals_discarded,
            ),
            (
                "fortuna_budget_breaches_total",
                "Cycles degraded by a cost-budget breach (each one alerts)",
                c.budget_breaches,
            ),
            (
                "fortuna_venue_api_errors_total",
                "Venue API failures seen by the polling loops",
                c.venue_api_errors,
            ),
            (
                "fortuna_settlement_voids_total",
                "Voided-market settlements processed",
                c.settlement_voids,
            ),
            (
                "fortuna_settlement_reversals_total",
                "Settlement corrections (reversals) processed",
                c.settlement_reversals,
            ),
            (
                "fortuna_order_ack_latency_ms_count",
                "Orders with measured submit->ack latency",
                c.ack_latency.count,
            ),
            (
                "fortuna_fill_latency_ms_count",
                "Fills with measured submit->execution latency",
                c.fill_latency.count,
            ),
        ] {
            samples.push(MetricSample {
                name,
                help,
                counter: true,
                labels: Vec::new(),
                value: value as i64,
            });
        }
        // Strategy attribution: market -> strategy from the intent set.
        let mut market_strategy: BTreeMap<MarketId, String> = BTreeMap::new();
        for (_, rec) in self.manager.intents() {
            let m = rec.order.market.clone();
            let strat = rec.order.strategy.to_string();
            match market_strategy.get(&m) {
                Some(existing) if existing != &strat => {
                    market_strategy.insert(m, "shared".to_string());
                }
                None => {
                    market_strategy.insert(m, strat);
                }
                _ => {}
            }
        }
        let mut pnl_by: BTreeMap<String, i64> = BTreeMap::new();
        let mut fees_by: BTreeMap<String, i64> = BTreeMap::new();
        for p in self.positions.positions() {
            let owner = market_strategy
                .get(&p.market)
                .cloned()
                .unwrap_or_else(|| "unattributed".to_string());
            *pnl_by.entry(owner.clone()).or_insert(0) += p.realized_pnl.raw();
            *fees_by.entry(owner).or_insert(0) += p.fees_paid.raw();
        }
        for (owner, v) in &pnl_by {
            samples.push(MetricSample {
                name: "fortuna_realized_pnl_cents",
                help: "Realized PnL by strategy attribution",
                counter: false,
                labels: vec![("strategy".to_string(), owner.clone())],
                value: *v,
            });
        }
        for (owner, v) in &fees_by {
            samples.push(MetricSample {
                name: "fortuna_fees_paid_cents",
                help: "Fees paid by strategy attribution",
                counter: false,
                labels: vec![("strategy".to_string(), owner.clone())],
                value: *v,
            });
        }
        for name in &self.envelope_names {
            samples.push(MetricSample {
                name: "fortuna_reserved_exposure_cents",
                help: "Active envelope reservations (worst case)",
                counter: false,
                labels: vec![("strategy".to_string(), name.clone())],
                value: self.reservations.active_total(name).raw(),
            });
            // Section 8 "envelope reservation utilization": reserved
            // fraction of the configured envelope, in bps.
            let active = self.reservations.active_total(name).raw();
            if let Ok(headroom) = self.reservations.headroom(name) {
                let envelope = headroom.raw().saturating_add(active);
                if envelope > 0 {
                    samples.push(MetricSample {
                        name: "fortuna_envelope_utilization_bps",
                        help: "Reserved fraction of the strategy envelope (bps)",
                        counter: false,
                        labels: vec![("strategy".to_string(), name.clone())],
                        value: active.saturating_mul(10_000) / envelope,
                    });
                }
            }
        }
        // Section 8 "gate rejection counts by reason".
        for (check, count) in &self.gate_rejections_by_check {
            samples.push(MetricSample {
                name: "fortuna_gate_rejections_by_check_total",
                help: "Gate rejections attributed to the refusing check",
                counter: true,
                labels: vec![("check".to_string(), check.clone())],
                value: *count as i64,
            });
        }
        samples.push(MetricSample {
            name: "fortuna_capital_in_limbo_cents",
            help: "Settlement value announced but not venue-confirmed",
            counter: false,
            labels: Vec::new(),
            value: self
                .settlements
                .capital_in_limbo()
                .map(|c| c.raw())
                .unwrap_or(-1),
        });
        samples.push(MetricSample {
            name: "fortuna_settlements_overdue",
            help: "Markets past close + lag + grace without settlement",
            counter: false,
            labels: Vec::new(),
            value: self.overdue_alerted.len() as i64,
        });
        samples.push(MetricSample {
            name: "fortuna_halt_active",
            help: "1 when a global halt is set (operator re-arm required)",
            counter: false,
            labels: Vec::new(),
            value: i64::from(self.gates.halts().global_halted().is_some()),
        });
        // T5.B8: per-strategy diagnostic gauges (e.g. perp basis-v2's A10
        // numbers). Appended AFTER the runner's own samples, in strategy
        // REGISTRATION order, so the export stays deterministic. Default-empty
        // for every strategy that does not override `metric_samples`, so this
        // adds nothing for the mechanical/synthesis arms — it is purely
        // additive. Read-only (`metric_samples` does not drain).
        for strategy in &self.strategies {
            samples.extend(strategy.metric_samples());
        }
        samples
    }

    /// Compose the rich daily digest's raw inputs (S6b) — per-strategy PnL/
    /// fees/fills/exposure + the honesty numbers + veto accounting — from the
    /// runner's own state. The daemon maps this to `fortuna_ops` DigestInputs
    /// and renders it; the runner stays free of the telemetry layer.
    pub fn digest_snapshot(&self) -> DigestSnapshot {
        // market -> strategy, the SAME attribution metrics_export uses
        // ("shared" when two strategies traded one market; "unattributed" for a
        // position whose intent did not survive).
        let mut market_strategy: BTreeMap<MarketId, String> = BTreeMap::new();
        for (_, rec) in self.manager.intents() {
            let m = rec.order.market.clone();
            let strat = rec.order.strategy.to_string();
            match market_strategy.get(&m) {
                Some(existing) if existing != &strat => {
                    market_strategy.insert(m, "shared".to_string());
                }
                None => {
                    market_strategy.insert(m, strat);
                }
                _ => {}
            }
        }
        // Per-strategy realized PnL + fees (over positions) and FILLED-order
        // count (over intents), all attributed via market_strategy.
        let mut pnl_by: BTreeMap<String, i64> = BTreeMap::new();
        let mut fees_by: BTreeMap<String, i64> = BTreeMap::new();
        let mut fills_by: BTreeMap<String, u64> = BTreeMap::new();
        for p in self.positions.positions() {
            let owner = market_strategy
                .get(&p.market)
                .cloned()
                .unwrap_or_else(|| "unattributed".to_string());
            *pnl_by.entry(owner.clone()).or_insert(0) += p.realized_pnl.raw();
            *fees_by.entry(owner).or_insert(0) += p.fees_paid.raw();
        }
        for (_, rec) in self.manager.intents() {
            if rec.cum_filled.raw() > 0 {
                let owner = market_strategy
                    .get(&rec.order.market)
                    .cloned()
                    .unwrap_or_else(|| "unattributed".to_string());
                *fills_by.entry(owner).or_insert(0) += 1;
            }
        }
        let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        names.extend(pnl_by.keys().cloned());
        names.extend(fills_by.keys().cloned());
        let strategies = names
            .into_iter()
            .map(|s| DigestStrategyRow {
                realized_pnl_cents: *pnl_by.get(&s).unwrap_or(&0),
                fees_cents: *fees_by.get(&s).unwrap_or(&0),
                fills: *fills_by.get(&s).unwrap_or(&0),
                open_exposure_cents: self.reserved_total(&s).raw(),
                strategy: s,
            })
            .collect();
        let c = self.counters();
        DigestSnapshot {
            strategies,
            halts_active: u64::from(self.gates.halts().global_halted().is_some()),
            discrepancies_open: c.discrepancies,
            settlements_overdue: self.overdue_alerted.len() as u64,
            capital_in_limbo_cents: self
                .settlements
                .capital_in_limbo()
                .map(|x| x.raw())
                .unwrap_or(0),
            veto_decisions: c.veto_decisions,
            veto_suppressed: c.veto_suppressed,
        }
    }

    pub fn counters(&self) -> RunCounters {
        let mut counters = self.counters;
        for strategy in &self.strategies {
            let m = strategy.metrics();
            counters.cognition_failures += m.cognition_failures;
            counters.shadow_cycles += m.shadow_cycles;
            counters.beliefs_drafted += m.beliefs_drafted;
            counters.model_proposals_discarded += m.model_proposals_discarded;
            counters.cognition_cost_cents += m.cognition_cost_cents;
            counters.mind_spend_today_cents += m.mind_spend_today_cents;
        }
        counters
    }

    /// The composed strategies' ids in registration order — the seam the
    /// fortuna-live composition (S3) asserts on: WHICH strategies booted (that
    /// synthesis was wired alongside the mechanical strategies, or that an
    /// empty / disabled synthesis config left the daemon mechanically-only).
    pub fn strategy_ids(&self) -> Vec<StrategyId> {
        self.strategies.iter().map(|s| s.id()).collect()
    }

    /// The composed strategies' (id, stage) in registration order (I7 read
    /// seam): WHICH stage each booted arm runs at. The stage allowlist is
    /// enforced at construction, but a composition still chooses each arm's
    /// stage — the demo-flip's Kalshi path runs the synthesis arm at
    /// `Stage::Paper` while the Sim path runs it at `Stage::Sim`; this exposes
    /// that choice for the composition's tests (and future promotion/ROTA
    /// views). Pure read; no money-path effect.
    pub fn strategy_stages(&self) -> Vec<(StrategyId, Stage)> {
        self.strategies
            .iter()
            .map(|s| (s.id(), s.stage()))
            .collect()
    }

    /// Push a freshly loaded confirmed-tier edge set into the composed
    /// synthesis arm (S4 per-segment refresh; the daemon calls this once per
    /// `drive()` segment — synthesis-edge-source-decision req 2). Returns the
    /// new edge count, or `None` when no synthesis arm is composed (a
    /// mechanically-only daemon — the refresh is then a no-op, never an
    /// error). The set replaces the arm's prior set wholesale; an empty set
    /// is VALID (req 3 fail-closed). The daemon composes exactly one synthesis
    /// arm, but every edge-trading arm is refreshed for robustness.
    pub fn refresh_synthesis_edges(
        &mut self,
        edges: &[fortuna_cognition::cycle::EdgeView],
    ) -> Option<usize> {
        let mut count = None;
        for s in self.strategies.iter_mut() {
            if let Some(n) = s.refresh_edges(edges) {
                count = Some(n);
            }
        }
        // The book-poll universe (self.markets, iterated in tick()) must include
        // every edge market, or the SynthesisStrategy — which fires only on a
        // BookSnapshot for one of its edge markets — never sees a quote. Edges
        // discovered at runtime (F7 weather day-set, world-forward) reference
        // markets OUTSIDE the static configured set; fold them in (deduped both
        // against the existing set and within the batch). tick() skips terminal
        // markets via market_meta, so stale-dated discoveries cost no real poll.
        let mut known: std::collections::BTreeSet<MarketId> =
            self.markets.iter().cloned().collect();
        for e in edges {
            if let Ok(mid) = MarketId::new(&e.market) {
                if known.insert(mid.clone()) {
                    self.markets.push(mid);
                }
            }
        }
        count
    }

    /// The live catalog as discovery `MarketView`s (T4.2): the all-status catalog
    /// the venue last returned (`market_meta`), projected for the market-back
    /// discovery prefilter. Empty until the first successful catalog poll.
    pub fn market_views(&self) -> Vec<fortuna_cognition::discovery::MarketView> {
        self.market_meta
            .values()
            .map(|m| fortuna_cognition::discovery::MarketView {
                market_id: m.id.to_string(),
                venue: m.venue.to_string(),
                title: m.title.clone(),
                category: m.category.clone(),
                volume_contracts: m.volume_contracts,
                resolution_source: m.settlement.resolution_source.clone(),
                close_at: m.close_at,
            })
            .collect()
    }

    /// The composed synthesis arm's live edge count, or `None` for a
    /// mechanically-only daemon — the read seam `drive()`'s smoke asserts a
    /// per-segment refresh took (it reflects ONLY an edge-trading arm, never
    /// a mechanical one).
    pub fn synthesis_edge_count(&self) -> Option<usize> {
        self.strategies.iter().find_map(|s| s.edge_count())
    }

    /// Apply a venue settlement to local books (sim convenience mirroring
    /// what the settlement processors automate at T1.4).
    pub fn apply_settlement(
        &mut self,
        market: &MarketId,
        winner: fortuna_core::market::Side,
        payout: Cents,
    ) -> Result<(), RunnerError> {
        // Counterfactually score vetoed-away quantity FIRST (spec Section
        // 6): a fully suppressed trade left no position behind, but the
        // settlement outcome is observable either way and the veto's
        // value-add must be measured. Exactly-once: scored entries drain.
        self.score_vetoes_at_settlement(market, winner, false);
        // Markets settle whether we hold them or not; the strict state
        // layer (untracked-market settlement is an error there) is only
        // consulted when a position actually exists.
        let owed = if self.positions.position(market).is_some() {
            self.positions
                .apply_settlement(market, winner, Cents::new(100))?
        } else {
            Cents::ZERO
        };
        self.bus.publish_external(EventPayload::Settled {
            venue: self.venue.id(),
            market: market.clone(),
            payout_cents: payout.raw(),
        });
        self.audit(
            "settlement",
            Some(market.as_str()),
            serde_json::json!({ "winner": format!("{winner:?}"), "owed": owed.raw() }),
        );
        self.bus.run_until_idle()?;
        self.seen_events = self.bus.recording().events().len();
        Ok(())
    }
}

/// SimVenue-specialized reads. `boards_json`'s account block is SIM-ONLY
/// (design R6): it reads the SimVenue's synchronous ground-truth totals
/// (`inspect_totals`) so the dashboard stays a pure, clock-free, NON-async
/// accessor — `views_from` and the daemon's sync `between_segments` closure
/// depend on that. The generic `Venue::account()` is `async`, so a venue-
/// agnostic board would force this read (and its callers) async; offering
/// `boards_json` ONLY for `V = SimVenue` keeps the sim path byte-identical
/// (A3) and defers the Kalshi board to Phase 2 (the `ActiveRunner` seam).
impl<J: IntentJournal + Send> SimRunner<SimVenue, J> {
    /// Read-only board data for the dashboard (positions + ops boards).
    pub fn boards_json(&self) -> serde_json::Value {
        let positions: Vec<serde_json::Value> = self
            .positions
            .positions()
            .map(|p| {
                serde_json::json!({
                    "market": p.market.to_string(),
                    "yes": p.yes.qty,
                    "no": p.no.qty,
                    "realized_pnl_cents": p.realized_pnl.raw(),
                    "fees_cents": p.fees_paid.raw(),
                    "lifecycle": format!("{:?}", p.lifecycle),
                })
            })
            .collect();
        let c = self.counters;
        // SIM-ONLY account block (design R6): cash + reserved exposure from the
        // venue's ground-truth totals. §5's floating/total need the mark loop
        // (not yet exposed), so the dashboard reports those null — never faked.
        let (cash, reserved, _, _) = self.venue.inspect_totals();
        serde_json::json!({
            "positions": positions,
            "account": {
                "cash_cents": cash.raw(),
                "reserved_cents": reserved.raw(),
            },
            "ops": {
                "ticks": c.ticks,
                "halt_active": self.gates.halts().global_halted().is_some(),
                "discrepancies": c.discrepancies,
                "settlements_overdue": self.overdue_alerted.len(),
                "capital_in_limbo_cents": self
                    .settlements
                    .capital_in_limbo()
                    .map(|x| x.raw())
                    .unwrap_or(-1),
            },
        })
    }
}

fn notional_plus_worst_fee(
    fees: &ScheduleFeeModel,
    price: Cents,
    qty: i64,
    at: UtcTimestamp,
) -> Result<Cents, RunnerError> {
    use fortuna_core::book::FillRole;
    let cost = price.checked_mul(qty)?;
    let taker = fees
        .fee(FillRole::Taker, price, Contracts::new(qty), None, at)
        .map_err(|e| RunnerError::Config {
            reason: format!("fee: {e}"),
        })?;
    let maker = fees
        .fee(FillRole::Maker, price, Contracts::new(qty), None, at)
        .map_err(|e| RunnerError::Config {
            reason: format!("fee: {e}"),
        })?;
    Ok(cost.checked_add(taker.max(maker).max(Cents::ZERO))?)
}

#[cfg(test)]
mod a3_type_level {
    //! Type-level A3 guard for the demo-flip Phase 1 generalization: the
    //! defaulted `SimRunner` MUST resolve to `SimRunner<SimVenue, MemoryJournal>`
    //! and the historical `SimRunner::new(...)` MUST produce that exact type with
    //! the prior default behavior. If the param order or the defaults regress,
    //! these stop compiling (the binding type-check) or fail (the cash assert).
    use super::*;
    use crate::MemoryAuditSink;
    use crate::{StrategyKind, StrategyMetrics};
    use async_trait::async_trait;
    use fortuna_core::book::{Fill, FillPage, OrderBook};
    use fortuna_core::market::{Action, ClientOrderId, VenueOrderId};
    use fortuna_gates::GatedOrder;
    use fortuna_venues::{
        OpenOrder, SettlementNotice, SettlementOutcome, SettlementPage, VenueError, VenuePosition,
    };
    use std::sync::Mutex;

    fn minimal_config() -> RunnerConfig {
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

            [rate.sim]
            burst = 100
            sustained_per_min = 600
            market_burst = 50
            market_sustained_per_min = 300
            "#,
        )
        .expect("minimal gate config parses");
        let fees: fortuna_venues::fees::FeeSchedule = toml::from_str(
            r#"
                formula = "quadratic"
                effective_date = "2026-01-01"
                taker_coeff = "0.07"
                maker_coeff = "0.0175"
            "#,
        )
        .expect("fee schedule parses");
        RunnerConfig {
            seed: 7,
            gate_config,
            exec_policy: fortuna_exec::ExecPolicy::default(),
            envelopes: BTreeMap::new(),
            max_daily_loss: Cents::new(50_000),
            fee_model: ScheduleFeeModel::new(vec![fees]).expect("fee model"),
            markets: Vec::new(),
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
    fn sim_runner_new_resolves_to_sim_venue_memory_journal_default() {
        let start = UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap();
        // The historical constructor: the binding's EXPLICIT type pins that
        // `SimRunner::new` yields `SimRunner<SimVenue, MemoryJournal>` (the
        // defaults), i.e. the param order is `<V, J>` with both defaulting.
        let default_runner: SimRunner<SimVenue, MemoryJournal> = SimRunner::new(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
        )
        .expect("default SimRunner::new constructs");

        // And the fully-spelled turbofish form resolves to the SAME inherent
        // `new` (proves `impl SimRunner` == `impl SimRunner<SimVenue, MemoryJournal>`).
        let spelled = SimRunner::<SimVenue, MemoryJournal>::new(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
        )
        .expect("turbofish SimRunner::<SimVenue, MemoryJournal>::new constructs");

        // Old-default behavior: an untraded runner reports starting cash on both.
        let a = futures::executor::block_on(default_runner.report()).expect("report");
        let b = futures::executor::block_on(spelled.report()).expect("report");
        assert_eq!(a.final_cash, Cents::new(1_000_000));
        assert_eq!(a.final_cash, b.final_cash);
        assert_eq!(a.realized_pnl, Cents::ZERO);
    }

    struct RecordingVenue {
        id: VenueId,
        filters: Arc<Mutex<Vec<MarketFilter>>>,
        fees: ScheduleFeeModel,
        fills: Vec<Fill>,
        fill_next: Cursor,
        fill_calls: Arc<Mutex<u64>>,
        settlement_notices: Vec<SettlementNotice>,
        settle_next: Cursor,
        settlement_calls: Arc<Mutex<u64>>,
        positions: Vec<VenuePosition>,
        rate_limit_fills: bool,
        rate_limit_balance: bool,
    }

    impl RecordingVenue {
        fn new(filters: Arc<Mutex<Vec<MarketFilter>>>) -> Self {
            Self::with_fills(filters, Vec::new(), Cursor::start())
        }

        fn with_fills(
            filters: Arc<Mutex<Vec<MarketFilter>>>,
            fills: Vec<Fill>,
            fill_next: Cursor,
        ) -> Self {
            let fees: fortuna_venues::fees::FeeSchedule = toml::from_str(
                r#"
                formula = "quadratic"
                effective_date = "2026-01-01"
                taker_coeff = "0.07"
                maker_coeff = "0.0175"
            "#,
            )
            .expect("fee schedule parses");
            Self {
                id: VenueId::new("recording").expect("venue id parses"),
                filters,
                fees: ScheduleFeeModel::new(vec![fees]).expect("fee model"),
                fills,
                fill_next,
                fill_calls: Arc::new(Mutex::new(0)),
                settlement_notices: Vec::new(),
                settle_next: Cursor::start(),
                settlement_calls: Arc::new(Mutex::new(0)),
                positions: Vec::new(),
                rate_limit_fills: false,
                rate_limit_balance: false,
            }
        }

        fn with_settlements(
            filters: Arc<Mutex<Vec<MarketFilter>>>,
            notices: Vec<SettlementNotice>,
            settle_next: Cursor,
        ) -> Self {
            let mut venue = Self::new(filters);
            venue.settlement_notices = notices;
            venue.settle_next = settle_next;
            venue
        }

        fn with_positions(
            filters: Arc<Mutex<Vec<MarketFilter>>>,
            positions: Vec<VenuePosition>,
        ) -> Self {
            let mut venue = Self::new(filters);
            venue.positions = positions;
            venue
        }

        fn rate_limited_fills(filters: Arc<Mutex<Vec<MarketFilter>>>) -> Self {
            let mut venue = Self::new(filters);
            venue.rate_limit_fills = true;
            venue
        }

        fn rate_limited_balance(filters: Arc<Mutex<Vec<MarketFilter>>>) -> Self {
            let mut venue = Self::new(filters);
            venue.rate_limit_balance = true;
            venue
        }
    }

    #[async_trait]
    impl Venue for RecordingVenue {
        fn id(&self) -> VenueId {
            self.id.clone()
        }

        async fn markets(&self, filter: MarketFilter) -> Result<Vec<Market>, VenueError> {
            self.filters
                .lock()
                .expect("recorded market filters lock")
                .push(filter);
            Ok(Vec::new())
        }

        async fn book(&self, market: &MarketId) -> Result<OrderBook, VenueError> {
            Err(VenueError::NotFound {
                what: format!("book {market}"),
            })
        }

        async fn place(&self, _order: GatedOrder) -> Result<VenueOrderId, VenueError> {
            Err(VenueError::Invalid {
                reason: "recording venue does not place orders".into(),
            })
        }

        async fn cancel(&self, _id: &VenueOrderId) -> Result<(), VenueError> {
            Ok(())
        }

        async fn positions(&self) -> Result<Vec<VenuePosition>, VenueError> {
            Ok(self.positions.clone())
        }

        async fn open_orders(&self) -> Result<Vec<OpenOrder>, VenueError> {
            Ok(Vec::new())
        }

        async fn balance(&self) -> Result<Cents, VenueError> {
            if self.rate_limit_balance {
                return Err(VenueError::RateLimited);
            }
            Ok(Cents::new(1_000_000))
        }

        async fn fills_since(&self, cursor: Cursor) -> Result<FillPage, VenueError> {
            *self.fill_calls.lock().expect("fill call counter lock") += 1;
            if self.rate_limit_fills {
                return Err(VenueError::RateLimited);
            }
            if cursor == Cursor::start() {
                Ok(FillPage {
                    fills: self.fills.clone(),
                    next_cursor: self.fill_next.clone(),
                })
            } else {
                Ok(FillPage {
                    fills: Vec::new(),
                    next_cursor: cursor,
                })
            }
        }

        async fn settlements_since(&self, cursor: Cursor) -> Result<SettlementPage, VenueError> {
            *self
                .settlement_calls
                .lock()
                .expect("settlement call counter lock") += 1;
            if cursor == Cursor::start() {
                Ok(SettlementPage {
                    notices: self.settlement_notices.clone(),
                    next_cursor: self.settle_next.clone(),
                })
            } else {
                Ok(SettlementPage {
                    notices: Vec::new(),
                    next_cursor: cursor,
                })
            }
        }

        fn fee_model(&self) -> &dyn FeeModel {
            &self.fees
        }
    }

    #[test]
    fn catalog_refresh_requests_all_statuses_so_the_watchdog_sees_lifecycle() {
        // The catalog refresh feeds the lifecycle watchdogs (dispute freeze,
        // overdue, terminal-skip), so it MUST request every status — a
        // Trading-only filter silently hides Disputed/Settled/Voided markets and
        // the watchdog can never act on them (regression: settlement_dst dispute
        // arm). The venue scopes the catalog to its configured series.
        let start = UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap();
        let filters = Arc::new(Mutex::new(Vec::new()));
        let venue = RecordingVenue::new(filters.clone());
        let clock = Arc::new(SimClock::new(start));
        let mut runner = futures::executor::block_on(SimRunner::new_with_venue(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        ))
        .expect("runner constructs with recording venue");

        futures::executor::block_on(runner.tick()).expect("empty tick succeeds");

        assert_eq!(
            filters
                .lock()
                .expect("recorded market filters lock")
                .as_slice(),
            &[MarketFilter::default()],
            "catalog refresh must request all statuses (default filter) for the watchdog"
        );
    }

    #[test]
    fn market_views_projects_the_live_catalog_for_discovery() {
        // T4.2: market_views() is what the daemon feeds market-back discovery —
        // the runner's last venue catalog (market_meta, ALL statuses) projected
        // to MarketView. The empty case (pre-first-poll) yields no views.
        let start = UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap();
        let venue = RecordingVenue::new(Arc::new(Mutex::new(Vec::new())));
        let clock = Arc::new(SimClock::new(start));
        let mut runner = futures::executor::block_on(SimRunner::new_with_venue(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        ))
        .expect("runner constructs");
        assert!(
            runner.market_views().is_empty(),
            "no views before the first catalog poll"
        );

        let m = paper_live_market();
        runner.market_meta.insert(m.id.clone(), m);

        let views = runner.market_views();
        assert_eq!(views.len(), 1);
        let v = &views[0];
        assert_eq!(v.market_id, "KXTEST-26JUN16-T50");
        assert_eq!(v.venue, "paper-live");
        assert_eq!(v.category, "weather");
        assert_eq!(v.resolution_source, "test");
        assert_eq!(v.volume_contracts, Some(10));
    }

    fn paper_live_market() -> Market {
        Market {
            id: MarketId::new("KXTEST-26JUN16-T50").expect("market id parses"),
            venue: VenueId::new("paper-live").expect("venue id parses"),
            title: "Will the test settle yes?".into(),
            category: "weather".into(),
            status: MarketStatus::Trading,
            close_at: None,
            settlement: fortuna_venues::SettlementMeta {
                oracle_type: "weather".into(),
                resolution_source: "test".into(),
                expected_lag_hours: 1,
            },
            volume_contracts: Some(10),
            payout_per_contract: Cents::new(100),
        }
    }

    fn kalshi_series_body() -> serde_json::Value {
        serde_json::json!({
            "series": {
                "ticker": "KXTEST",
                "title": "Test series",
                "category": "weather",
                "fee_type": "quadratic",
                "fee_multiplier": 1.0,
                "settlement_sources": [{"name": "test", "url": "https://example.test/rules"}]
            }
        })
    }

    fn kalshi_markets_body() -> serde_json::Value {
        serde_json::json!({
            "markets": [{
                "ticker": "KXTEST-26JUN16-T50",
                "event_ticker": "KXTEST-26JUN16",
                "market_type": "binary",
                "title": "Will the test settle yes?",
                "yes_sub_title": "Yes",
                "no_sub_title": "No",
                "status": "active",
                "strike_type": "greater",
                "floor_strike": 50,
                "close_time": "2026-06-16T17:00:00Z",
                "settlement_timer_seconds": 3600,
                "notional_value_dollars": "1.0000",
                "price_level_structure": "linear_cent",
                "volume_fp": "10.00"
            }],
            "cursor": ""
        })
    }

    fn kalshi_book_body() -> serde_json::Value {
        serde_json::json!({
            "orderbook_fp": {
                "yes_dollars": [["0.3000", "4.00"]],
                "no_dollars": [["0.4000", "5.00"]]
            }
        })
    }

    #[derive(Clone)]
    struct BookQuoteRecorder {
        seen: Arc<Mutex<Vec<(Cents, Cents)>>>,
    }

    #[async_trait]
    impl Strategy for BookQuoteRecorder {
        fn id(&self) -> StrategyId {
            StrategyId::new("paper_live_quote_recorder").expect("strategy id parses")
        }

        fn kind(&self) -> StrategyKind {
            StrategyKind::Mechanical
        }

        fn stage(&self) -> Stage {
            Stage::Paper
        }

        async fn on_event(
            &mut self,
            ev: &fortuna_core::bus::BusEvent,
            core: &CoreHandle<'_>,
        ) -> Result<Vec<Proposal>, RunnerError> {
            if let fortuna_core::bus::EventPayload::BookSnapshot { book, .. } = &ev.payload {
                let snapshot = core
                    .books
                    .get(&book.market)
                    .ok_or_else(|| RunnerError::Config {
                        reason: format!("missing core book for {}", book.market),
                    })?;
                self.seen
                    .lock()
                    .expect("quote recorder lock")
                    .push((snapshot.yes_bids[0].price, snapshot.yes_asks[0].price));
            }
            Ok(Vec::new())
        }

        fn metrics(&self) -> StrategyMetrics {
            StrategyMetrics::default()
        }
    }

    #[test]
    fn tick_refreshes_paper_live_before_book_snapshot_dispatch() {
        use fortuna_paper::{PaperConfig, PaperLiveVenue};
        use fortuna_venues::kalshi::client::{KalshiTransport, MockKalshiTransport};
        use fortuna_venues::kalshi::KalshiReadClient;

        let start = UtcTimestamp::parse_iso8601("2026-06-16T12:00:00.000Z").unwrap();
        let mock = Arc::new(MockKalshiTransport::new());
        mock.push_ok(200, kalshi_series_body());
        mock.push_ok(200, kalshi_markets_body());
        mock.push_ok(200, kalshi_book_body());
        mock.push_ok(
            200,
            serde_json::json!({
                "trades": [],
                "cursor": ""
            }),
        );
        mock.push_ok(200, kalshi_markets_body());
        mock.push_ok(200, kalshi_book_body());

        let clock = Arc::new(SimClock::new(start));
        let mut config = minimal_config();
        config.markets = vec![paper_live_market()];
        let fees = config.fee_model.clone();
        let read = KalshiReadClient::new(
            mock.clone() as Arc<dyn KalshiTransport>,
            clock.clone(),
            vec!["KXTEST".to_string()],
        )
        .expect("read client constructs");
        let venue = PaperLiveVenue::new(
            read,
            clock.clone(),
            fees,
            PaperConfig {
                maker_haircut_pct: 100,
            },
            Cents::new(1_000_000),
        )
        .expect("paper-live venue constructs");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let strategy: Box<dyn Strategy> = Box::new(BookQuoteRecorder { seen: seen.clone() });

        let mut runner = futures::executor::block_on(SimRunner::new_with_venue(
            config,
            vec![strategy],
            Box::new(MemoryAuditSink::default()),
            start,
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        ))
        .expect("runner constructs with paper-live venue");

        futures::executor::block_on(runner.tick()).expect("paper-live tick succeeds");

        assert_eq!(
            seen.lock().expect("quote recorder lock").as_slice(),
            &[(Cents::new(30), Cents::new(60))]
        );
        let calls = mock.calls();
        assert_eq!(calls[0].path, "/series/KXTEST");
        assert_eq!(calls[1].path, "/markets");
        assert_eq!(calls[2].path, "/markets/KXTEST-26JUN16-T50/orderbook");
        assert_eq!(calls[3].path, "/markets/trades");
        assert_eq!(calls[4].path, "/markets");
        assert_eq!(calls[5].path, "/markets/KXTEST-26JUN16-T50/orderbook");
        assert!(
            calls.iter().all(|call| call.method == "GET"),
            "paper-live tick must only issue live data reads: {calls:?}"
        );
    }

    #[test]
    fn refresh_synthesis_edges_folds_edge_markets_into_the_book_poll_universe() {
        // The SynthesisStrategy fires ONLY on a BookSnapshot for one of its edge
        // markets, and tick() polls books for self.markets. Edges discovered at
        // runtime (the F7 weather day-set, world-forward) reference markets
        // OUTSIDE the static configured set — refresh must fold them into the
        // poll universe, or the comparator never sees a quote and never trades.
        let start = UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap();
        let filters = Arc::new(Mutex::new(Vec::new()));
        let venue = RecordingVenue::new(filters.clone());
        let clock = Arc::new(SimClock::new(start));
        let mut runner = futures::executor::block_on(SimRunner::new_with_venue(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        ))
        .expect("runner constructs with recording venue");

        let discovered = "KXHIGHPHIL-26JUN17-B85.5";
        assert!(
            !runner.markets.iter().any(|m| m.to_string() == discovered),
            "precondition: the discovered market is NOT in the configured set"
        );

        let edge = |m: &str| fortuna_cognition::cycle::EdgeView {
            market: m.to_string(),
            event_id: format!("aeolus:{m}"),
            mapping: fortuna_cognition::events::MappingType::Direct,
            tier: fortuna_cognition::events::EdgeTier::Confirmed,
        };

        runner.refresh_synthesis_edges(&[edge(discovered)]);
        assert!(
            runner.markets.iter().any(|m| m.to_string() == discovered),
            "edge market must enter the book-poll universe after refresh"
        );

        // Idempotent: a later refresh with the same edge does not duplicate it.
        runner.refresh_synthesis_edges(&[edge(discovered), edge(discovered)]);
        let occurrences = runner
            .markets
            .iter()
            .filter(|m| m.to_string() == discovered)
            .count();
        assert_eq!(occurrences, 1, "edge market must not be duplicated");
    }

    #[test]
    fn external_venue_position_is_reported_once_without_halting() {
        let start = UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap();
        let external_market = MarketId::new("KXEXTERNAL-1").expect("market id");
        let filters = Arc::new(Mutex::new(Vec::new()));
        let venue = RecordingVenue::with_positions(
            filters,
            vec![VenuePosition {
                market: external_market,
                yes: 1,
                no: 0,
                cost: Cents::new(47),
            }],
        );
        let clock = Arc::new(SimClock::new(start));
        let mut runner = futures::executor::block_on(SimRunner::new_with_venue(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        ))
        .expect("runner constructs with recording venue");

        let first =
            futures::executor::block_on(runner.tick()).expect("first tick reports external");
        let second = futures::executor::block_on(runner.tick()).expect("second tick keeps running");
        let third = futures::executor::block_on(runner.tick()).expect("third tick keeps running");

        assert!(!first.halted);
        assert!(!second.halted);
        assert!(!third.halted);
        assert!(
            runner.active_halt().is_none(),
            "external account positions are quarantined, not treated as runner book drift"
        );
        assert_eq!(
            runner.counters.discrepancies, 1,
            "the same external venue position should not spam discrepancy records"
        );
    }

    #[test]
    fn orphan_historical_fill_records_discrepancy_without_crashing_tick() {
        let start = UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap();
        let fill = Fill {
            fill_id: "historical-fill".into(),
            venue_order_id: VenueOrderId::new("venue-order").expect("venue order id"),
            client_order_id: ClientOrderId::new("unknown-client-order").expect("client order id"),
            market: MarketId::new("KXHIST-1").expect("market id"),
            side: Side::Yes,
            action: Action::Buy,
            price: Cents::new(47),
            qty: Contracts::new(1),
            fee: Cents::ZERO,
            is_maker: false,
            at: start,
        };
        let next = Cursor("after-historical-fill".into());
        let filters = Arc::new(Mutex::new(Vec::new()));
        let venue = RecordingVenue::with_fills(filters, vec![fill], next.clone());
        let clock = Arc::new(SimClock::new(start));
        let mut runner = futures::executor::block_on(SimRunner::new_with_venue(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        ))
        .expect("runner constructs with recording venue");

        let report = futures::executor::block_on(runner.tick()).expect("orphan fill is recorded");

        assert_eq!(report.fills_applied, 0);
        assert_eq!(runner.cursor, next);
    }

    #[test]
    fn non_advancing_orphan_fill_page_is_polled_once() {
        let start = UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap();
        let fill = Fill {
            fill_id: "historical-fill".into(),
            venue_order_id: VenueOrderId::new("venue-order").expect("venue order id"),
            client_order_id: ClientOrderId::new("unknown-client-order").expect("client order id"),
            market: MarketId::new("KXHIST-1").expect("market id"),
            side: Side::Yes,
            action: Action::Buy,
            price: Cents::new(47),
            qty: Contracts::new(1),
            fee: Cents::ZERO,
            is_maker: false,
            at: start,
        };
        let filters = Arc::new(Mutex::new(Vec::new()));
        let venue = RecordingVenue::with_fills(filters, vec![fill], Cursor::start());
        let fill_calls = venue.fill_calls.clone();
        let clock = Arc::new(SimClock::new(start));
        let mut runner = futures::executor::block_on(SimRunner::new_with_venue(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        ))
        .expect("runner constructs with recording venue");

        let report = futures::executor::block_on(runner.tick())
            .expect("non-advancing orphan fill is bounded");

        assert_eq!(report.fills_applied, 0);
        assert_eq!(
            *fill_calls.lock().expect("fill call counter lock"),
            1,
            "a non-advancing fill page must not be re-polled within the same tick"
        );
    }

    #[test]
    fn repeated_non_advancing_orphan_fill_is_reported_once() {
        let start = UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap();
        let fill = Fill {
            fill_id: "historical-fill".into(),
            venue_order_id: VenueOrderId::new("venue-order").expect("venue order id"),
            client_order_id: ClientOrderId::new("unknown-client-order").expect("client order id"),
            market: MarketId::new("KXHIST-1").expect("market id"),
            side: Side::Yes,
            action: Action::Buy,
            price: Cents::new(47),
            qty: Contracts::new(1),
            fee: Cents::ZERO,
            is_maker: false,
            at: start,
        };
        let filters = Arc::new(Mutex::new(Vec::new()));
        let venue = RecordingVenue::with_fills(filters, vec![fill], Cursor::start());
        let fill_calls = venue.fill_calls.clone();
        let clock = Arc::new(SimClock::new(start));
        let mut runner = futures::executor::block_on(SimRunner::new_with_venue(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        ))
        .expect("runner constructs with recording venue");

        let first = futures::executor::block_on(runner.tick()).expect("first tick reports orphan");
        let second =
            futures::executor::block_on(runner.tick()).expect("second tick dedupes orphan");

        assert_eq!(first.fills_applied, 0);
        assert_eq!(second.fills_applied, 0);
        assert_eq!(
            *fill_calls.lock().expect("fill call counter lock"),
            2,
            "the venue may repeat the page on later ticks, but the discrepancy is already known"
        );
        assert_eq!(
            runner.counters.discrepancies, 1,
            "a historical fill with a non-advancing cursor must not spam audit every tick"
        );
    }

    #[test]
    fn non_advancing_settlement_page_is_polled_once() {
        let start = UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap();
        let notice = SettlementNotice {
            notice_id: "historical-settlement".into(),
            market: MarketId::new("KXSETTLE-1").expect("market id"),
            outcome: SettlementOutcome::Voided,
            at: start,
            detail: serde_json::json!({ "source": "test" }),
        };
        let filters = Arc::new(Mutex::new(Vec::new()));
        let venue = RecordingVenue::with_settlements(filters, vec![notice], Cursor::start());
        let settlement_calls = venue.settlement_calls.clone();
        let clock = Arc::new(SimClock::new(start));
        let mut runner = futures::executor::block_on(SimRunner::new_with_venue(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        ))
        .expect("runner constructs with recording venue");

        futures::executor::block_on(runner.tick())
            .expect("non-advancing settlement page is bounded");

        assert_eq!(
            *settlement_calls
                .lock()
                .expect("settlement call counter lock"),
            1,
            "a non-advancing settlement page must not be re-polled within the same tick"
        );
        assert_eq!(runner.counters.settlement_notices, 1);
        assert_eq!(
            runner.counters.venue_api_errors, 0,
            "bounded settlement polling should not hit the rate-limit error path"
        );
    }

    #[test]
    fn rate_limited_fill_poll_does_not_crash_tick() {
        let start = UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap();
        let filters = Arc::new(Mutex::new(Vec::new()));
        let venue = RecordingVenue::rate_limited_fills(filters);
        let clock = Arc::new(SimClock::new(start));
        let mut runner = futures::executor::block_on(SimRunner::new_with_venue(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        ))
        .expect("runner constructs with recording venue");

        let report = futures::executor::block_on(runner.tick())
            .expect("rate limit is a transient poll miss");

        assert_eq!(report.fills_applied, 0);
        assert_eq!(runner.counters.venue_api_errors, 1);
    }

    #[test]
    fn rate_limited_account_read_halts_without_crashing_tick() {
        let start = UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap();
        let filters = Arc::new(Mutex::new(Vec::new()));
        let venue = RecordingVenue::rate_limited_balance(filters);
        let clock = Arc::new(SimClock::new(start));
        let mut runner = futures::executor::block_on(SimRunner::new_with_venue(
            minimal_config(),
            Vec::new(),
            Box::new(MemoryAuditSink::default()),
            start,
            MemoryJournal::default(),
            venue,
            clock,
            &[Stage::Sim, Stage::Paper],
        ))
        .expect("runner constructs with recording venue");

        let report = futures::executor::block_on(runner.tick())
            .expect("account rate limit becomes a halt, not a crash");

        assert!(report.halted);
        assert_eq!(runner.counters.venue_api_errors, 1);
    }
}
