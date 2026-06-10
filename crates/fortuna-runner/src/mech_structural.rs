//! mech_structural (spec Section 6, Atlas/Nike lineage): structural
//! mispricing scans. Pure mechanical, no model; the non-LLM baseline that
//! validates the execution path end to end.
//!
//! v0 scope (Phase 0): the BRACKET YES-SUM scan. A bracket family is a set
//! of mutually-exclusive-and-exhaustive markets (exactly one settles YES at
//! $1). When the sum of best YES asks plus taker fees drops below 100c by
//! at least the configured edge, buying one of each YES locks the spread.
//! Single-market YES/NO "sums" are deliberately absent: on a mirror-book
//! venue (Kalshi semantics: the YES ask ladder IS the NO bid ladder) such a
//! sum below 100c is a crossed book the exchange itself would match —
//! cross-MARKET structure is where the real arb lives. Bracket families are
//! strategy config in Phase 0; canonical-event edges take over at Phase 3
//! (ASSUMPTIONS.md, T0.10). Cross-venue divergence joins at T3.4 when a
//! second venue exists.
//!
//! Group economics for complete-or-unwind: value_per_set = 100c (one leg
//! pays out); legs carry fair values that distribute the captured edge so
//! each leg independently clears the gate's edge floor.

use crate::{
    CoreHandle, Proposal, ProposedLeg, RunnerError, Stage, Strategy, StrategyKind, StrategyMetrics,
    Urgency,
};
use async_trait::async_trait;
use fortuna_core::book::FillRole;
use fortuna_core::bus::{BusEvent, EventPayload};
use fortuna_core::market::{Action, Contracts, MarketId, Side, StrategyId};
use fortuna_core::money::Cents;
use fortuna_exec::GroupPolicy;

#[derive(Debug, Clone)]
pub struct MechStructuralConfig {
    /// Mutually-exclusive-exhaustive market families to scan.
    pub bracket_sets: Vec<Vec<MarketId>>,
    /// Minimum locked spread per set, net of modeled taker fees, in cents.
    pub min_edge_cents_per_set: i64,
    /// Group completion policy parameters (spec 5.4).
    pub max_unhedged_notional: Cents,
    pub max_leg_open_ms: i64,
    pub min_completion_edge_bps: i64,
}

pub struct MechStructural {
    id: StrategyId,
    config: MechStructuralConfig,
    metrics: StrategyMetrics,
    /// One shot per (bracket, book-state): avoid re-proposing the same arb
    /// while the first group is still working. Cleared when books move.
    last_proposal_key: Option<String>,
}

impl MechStructural {
    pub fn new(config: MechStructuralConfig) -> Result<MechStructural, RunnerError> {
        if config.bracket_sets.iter().any(|s| s.len() < 2) {
            return Err(RunnerError::Config {
                reason: "bracket sets need at least 2 markets".into(),
            });
        }
        Ok(MechStructural {
            id: StrategyId::new("mech_structural").map_err(|e| RunnerError::Config {
                reason: e.to_string(),
            })?,
            config,
            metrics: StrategyMetrics::default(),
            last_proposal_key: None,
        })
    }

    /// Scan one bracket family against current books. Returns a proposal
    /// when the YES basket is buyable below 100c net of fees by >= min edge.
    fn scan_bracket(
        &self,
        bracket: &[MarketId],
        core: &CoreHandle<'_>,
    ) -> Result<Option<(Proposal, String)>, RunnerError> {
        let mut legs = Vec::with_capacity(bracket.len());
        let mut total_cost = 0i64;
        let mut total_fees = 0i64;
        let mut key = String::new();
        for market in bracket {
            let Some(book) = core.books.get(market) else {
                return Ok(None); // a missing book = no certified scan
            };
            let Some(ask) = book.best_ask() else {
                return Ok(None);
            };
            // Estimate per-contract taker fee at a representative batch:
            // quantity-1 estimates overstate brutally (ceil rounding eats
            // sub-cent fees), and the gates re-verify at the SIZED quantity
            // anyway. Ceil per contract keeps the estimate conservative.
            const EST_QTY: i64 = 10;
            let batch_fee = core
                .fee_model
                .fee(
                    FillRole::Taker,
                    ask.price,
                    Contracts::new(EST_QTY),
                    None,
                    core.now,
                )
                .map_err(|e| RunnerError::Config {
                    reason: format!("fee model failed in scan: {e}"),
                })?
                .max(Cents::ZERO);
            let per_contract_fee = (batch_fee.raw() + EST_QTY - 1) / EST_QTY;
            total_cost += ask.price.raw();
            total_fees += per_contract_fee;
            key.push_str(&format!("{market}@{};", ask.price));
            legs.push((market.clone(), ask.price));
        }
        let edge = 100 - total_cost - total_fees;
        if edge < self.config.min_edge_cents_per_set {
            return Ok(None);
        }

        // Distribute the captured edge across legs so each clears the gate
        // edge floor on its own: fair_leg = ask + floor(edge / n).
        let n = legs.len() as i64;
        let per_leg_edge = edge / n;
        let proposed = legs
            .into_iter()
            .map(|(market, ask)| ProposedLeg {
                market,
                side: Side::Yes,
                action: Action::Buy,
                limit_price: ask,
                fair_value: Cents::new((ask.raw() + per_leg_edge).min(100)),
            })
            .collect();
        Ok(Some((
            Proposal {
                legs: proposed,
                group_policy: Some(GroupPolicy {
                    max_unhedged_notional: self.config.max_unhedged_notional,
                    max_leg_open_ms: self.config.max_leg_open_ms,
                    value_per_set: Cents::new(100),
                    min_completion_edge_bps: self.config.min_completion_edge_bps,
                }),
                urgency: Urgency::Taker, // arbs decay; cross the spread
                thesis: format!(
                    "bracket yes-sum arb: cost {total_cost}c + fees {total_fees}c \
                     locks {edge}c/set"
                ),
            },
            key,
        )))
    }
}

#[async_trait]
impl Strategy for MechStructural {
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
        core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        self.metrics.events_seen += 1;
        // Scan only on fresh book data, and only brackets containing the
        // updated market (a news burst is one decision, not five).
        let EventPayload::BookSnapshot { book, .. } = &ev.payload else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let sets: Vec<Vec<MarketId>> = self
            .config
            .bracket_sets
            .iter()
            .filter(|set| set.contains(&book.market))
            .cloned()
            .collect();
        for bracket in sets {
            if let Some((proposal, key)) = self.scan_bracket(&bracket, core)? {
                if self.last_proposal_key.as_deref() == Some(key.as_str()) {
                    continue; // same book state: already proposed
                }
                self.last_proposal_key = Some(key);
                self.metrics.proposals_emitted += 1;
                out.push(proposal);
            }
        }
        Ok(out)
    }

    fn metrics(&self) -> StrategyMetrics {
        self.metrics
    }
}
