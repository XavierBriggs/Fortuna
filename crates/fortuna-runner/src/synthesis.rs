//! The synthesis strategy adapter (Phase 2 EXIT composition): wires the
//! T2.6 `DecisionCycle` into the Strategy plugin interface so model
//! beliefs trade through the SAME sizing/gate/execution path as every
//! mechanical strategy.
//!
//! Discipline:
//! - I1: the adapter emits `Proposal`s into the ordinary tick path; no
//!   special lane exists for model-originated orders.
//! - I6: legs are UNSIZED (limit + fair value only); sizing, timing, and
//!   execution stay with the harness. The mind never sees this type.
//! - Cognition failure (provider error, schema-invalid output, refusal,
//!   budget exhaustion, context-assembly failure) DEGRADES: the cycle is
//!   counted as failed, zero proposals are emitted, and the loop keeps
//!   running. Mechanical strategies are unaffected (spec 5.9).
//! - Trigger shape: a book event for an edge-mapped market triggers ONE
//!   cycle for that market's event. Debounce/coalescing across rapid
//!   repeated triggers belongs to the TriggerEngine (T2.2) and wires in
//!   at the live composition (Phase 3); the Sim tick cadence is already
//!   one book event per market per tick.

use crate::{
    CoreHandle, DegradeRecord, Proposal, ProposedLeg, RunnerError, Stage, Strategy, StrategyKind,
    StrategyMetrics, Urgency,
};
use async_trait::async_trait;
use fortuna_cognition::context::{content_hash_of, ContextItem, SectionKind};
use fortuna_cognition::cycle::{
    CalibrationContext, ComparatorConfig, DecisionCycle, EdgeView, MarketQuote, ShadowSampler,
    TriageDecision,
};
use fortuna_cognition::mind::Mind;
use fortuna_core::bus::{BusEvent, EventPayload};
use fortuna_core::market::{Action, MarketId, StrategyId};
use fortuna_core::money::Cents;
use std::sync::Arc;

/// Configuration for one synthesis strategy instance. Edges are the
/// market<->event mappings this strategy trades (fed from the edges
/// repo in the live composition; static in Sim).
pub struct SynthesisConfig {
    pub id: StrategyId,
    pub edges: Vec<EdgeView>,
    pub comparator: ComparatorConfig,
    pub triage: TriageDecision,
    /// Declined-trigger shadow runs per UTC day (T2.6 sampler).
    pub shadow_quota: u32,
    /// The scope's calibration (spec 5.10), fetched by the composition
    /// from the ledger. None => beliefs shrink fully to the market prior
    /// and the strategy structurally prices no edge (fail closed).
    pub calibration: Option<CalibrationContext>,
    /// The stage this instance runs at. The composition derives it via
    /// `promotion::effective_stage(declared_cap, operator_records)` —
    /// a strategy never promotes itself (I7).
    pub stage: Stage,
}

/// `DecisionCycle` adapted to the Strategy plugin interface.
pub struct SynthesisStrategy {
    id: StrategyId,
    edges: Vec<EdgeView>,
    cycle: DecisionCycle,
    mind: Arc<dyn Mind>,
    metrics: StrategyMetrics,
    stage: Stage,
    pending_degrades: Vec<DegradeRecord>,
}

impl SynthesisStrategy {
    pub fn new(config: SynthesisConfig, mind: Arc<dyn Mind>) -> SynthesisStrategy {
        let mut cycle = DecisionCycle::new(
            config.triage,
            ShadowSampler::new(config.shadow_quota),
            config.comparator,
        );
        if let Some(calibration) = config.calibration {
            cycle = cycle.with_calibration(calibration);
        }
        SynthesisStrategy {
            id: config.id,
            cycle,
            edges: config.edges,
            mind,
            metrics: StrategyMetrics::default(),
            stage: config.stage,
            pending_degrades: Vec::new(),
        }
    }

    /// Point-in-time quotes for every edge-mapped market with a live
    /// book. Markets without a two-sided book are skipped (the
    /// comparator never prices a one-sided market).
    fn quotes(&self, core: &CoreHandle<'_>) -> Vec<MarketQuote> {
        self.edges
            .iter()
            .filter_map(|edge| {
                let market = MarketId::new(&edge.market).ok()?;
                let book = core.books.get(&market)?;
                let bid = book.yes_bids.first()?.price.raw();
                let ask = book.yes_asks.first()?.price.raw();
                Some(MarketQuote {
                    market: edge.market.clone(),
                    yes_bid_cents: bid,
                    yes_ask_cents: ask,
                })
            })
            .collect()
    }
}

#[async_trait]
impl Strategy for SynthesisStrategy {
    fn id(&self) -> StrategyId {
        self.id.clone()
    }
    fn kind(&self) -> StrategyKind {
        StrategyKind::Synthesis
    }
    fn stage(&self) -> Stage {
        self.stage
    }

    async fn on_event(
        &mut self,
        ev: &BusEvent,
        core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        let EventPayload::BookSnapshot { book, .. } = &ev.payload else {
            return Ok(Vec::new());
        };
        let market_str = book.market.to_string();
        let Some(edge) = self.edges.iter().find(|e| e.market == market_str) else {
            return Ok(Vec::new());
        };
        let event_id = edge.event_id.clone();
        self.metrics.events_seen += 1;

        let quotes = self.quotes(core);
        // Context: the point-in-time market snapshot the mind reasons
        // over (assembled, budgeted, and manifest-hashed by the cycle).
        let items: Vec<ContextItem> = quotes
            .iter()
            .map(|q| {
                let body = format!(
                    "{}: yes bid {}c / yes ask {}c",
                    q.market, q.yes_bid_cents, q.yes_ask_cents
                );
                ContextItem {
                    item_id: format!("quote-{}", q.market),
                    section: SectionKind::MarketSnapshot,
                    content_hash: content_hash_of(&body),
                    body,
                    at: core.now,
                }
            })
            .collect();

        let outcome = match self
            .cycle
            .run(
                &event_id,
                self.mind.as_ref(),
                &items,
                &self.edges,
                &quotes,
                core.now,
            )
            .await
        {
            Ok(outcome) => outcome,
            Err(failure) => {
                // Degrade, never crash — and NEVER SILENTLY (F1, spec
                // line 238): the failure kind is preserved and buffered
                // for the runner's audit log; budget breaches also count
                // separately (they alert on every occurrence).
                self.metrics.cognition_failures += 1;
                use fortuna_cognition::cycle::CycleError;
                use fortuna_cognition::mind::MindError;
                let (degrade, detail) = match &failure {
                    CycleError::Mind(MindError::BudgetExhausted {
                        scope,
                        spent_cents,
                        cap_cents,
                    }) => (
                        "budget_exhausted",
                        serde_json::json!({
                            "scope": scope,
                            "spent_cents": spent_cents,
                            "cap_cents": cap_cents,
                        }),
                    ),
                    CycleError::Mind(MindError::Provider { reason }) => {
                        ("provider", serde_json::json!({ "reason": reason }))
                    }
                    CycleError::Mind(MindError::SchemaInvalid { reason }) => {
                        ("schema_invalid", serde_json::json!({ "reason": reason }))
                    }
                    CycleError::Mind(MindError::Refused { explanation }) => {
                        ("refused", serde_json::json!({ "explanation": explanation }))
                    }
                    CycleError::Context(e) => {
                        ("context", serde_json::json!({ "reason": e.to_string() }))
                    }
                };
                self.pending_degrades.push(DegradeRecord {
                    event_id: event_id.clone(),
                    degrade,
                    detail,
                });
                return Ok(Vec::new());
            }
        };

        if outcome.shadow {
            self.metrics.shadow_cycles += 1;
        }
        self.metrics.beliefs_drafted += outcome.beliefs.len() as u64;
        self.metrics.model_proposals_discarded += outcome.discarded_model_proposals as u64;
        self.metrics.cognition_cost_cents += outcome.cost_cents;
        if outcome.discarded_model_proposals > 0 {
            // F3: a cycle whose model output is discarded leaves a trace.
            self.pending_degrades.push(DegradeRecord {
                event_id: event_id.clone(),
                degrade: "model_proposals_discarded",
                detail: serde_json::json!({ "count": outcome.discarded_model_proposals }),
            });
        }

        let mut proposals = Vec::with_capacity(outcome.candidates.len());
        for candidate in outcome.candidates {
            // The candidate's market came from this strategy's own edge
            // config; an unparseable id is a config defect, not data.
            let market = MarketId::new(&candidate.market).map_err(|e| RunnerError::Config {
                reason: format!("synthesis candidate market invalid: {e}"),
            })?;
            proposals.push(Proposal {
                legs: vec![ProposedLeg {
                    market,
                    side: candidate.side,
                    action: Action::Buy,
                    limit_price: Cents::new(candidate.max_price_cents),
                    fair_value: Cents::new(candidate.fair_cents),
                    // The Kelly input for the harness's sizing (I6: the
                    // strategy still sizes nothing).
                    calibrated_p: Some(candidate.calibrated_p),
                }],
                group_policy: None,
                urgency: Urgency::Passive,
                manifest_hash: Some(outcome.manifest_hash.clone()),
                thesis: format!(
                    "synthesis: belief {} fair {}c vs {}c (edge {}c)",
                    candidate.belief_id,
                    candidate.fair_cents,
                    candidate.max_price_cents,
                    candidate.edge_cents
                ),
            });
            self.metrics.proposals_emitted += 1;
        }
        Ok(proposals)
    }

    fn metrics(&self) -> StrategyMetrics {
        let mut m = self.metrics;
        m.mind_spend_today_cents = self.mind.spent_today_cents();
        m
    }

    fn drain_degrades(&mut self) -> Vec<DegradeRecord> {
        std::mem::take(&mut self.pending_degrades)
    }
}
