//! The decision cycle (spec 5.8), the comparator, the Kelly calibration
//! haircut (spec 5.14), and the triage tier with declined-trigger shadow
//! sampling.
//!
//! Flow: a fired trigger -> TRIAGE (cheap tier: worth frontier attention
//! or not; every verdict is loggable) -> on accept, assemble context and
//! run the frontier mind -> validated beliefs -> the COMPARATOR derives
//! two-sided UNSIZED candidates against live prices through the edges.
//! Sizing happens downstream (the runner draws from the envelope via
//! reservations using the haircut Kelly fraction); the gates re-check
//! everything (I1).
//!
//! Triage is itself scored: a deterministic fixed daily sample of
//! DECLINED triggers runs the full cycle in SHADOW — beliefs are
//! produced and scored normally, but a shadow run NEVER yields trade
//! candidates. This measures triage recall instead of assuming it.

use crate::beliefs::{BeliefDraft, Freshness};
use crate::context::{assemble_context, AssemblerConfig, ContextItem};
use crate::events::{EdgeTier, MappingType};
use crate::mind::{Mind, MindError};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::Side;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CycleError {
    #[error(transparent)]
    Mind(#[from] MindError),
    #[error("context assembly failed: {0}")]
    Context(#[from] crate::context::ContextError),
}

/// A belief as the comparator sees it: calibrated p plus the freshness
/// verdict (stale beliefs are EXCLUDED until refreshed, spec 5.5).
#[derive(Debug, Clone)]
pub struct BeliefView {
    pub belief_id: String,
    pub event_id: String,
    pub p: f64,
    pub freshness: Freshness,
}

/// A market-event edge as the comparator sees it.
#[derive(Debug, Clone)]
pub struct EdgeView {
    pub market: String,
    pub event_id: String,
    pub mapping: MappingType,
    pub tier: EdgeTier,
}

/// A live quote in YES space (integer cents).
#[derive(Debug, Clone)]
pub struct MarketQuote {
    pub market: String,
    pub yes_bid_cents: i64,
    pub yes_ask_cents: i64,
}

#[derive(Debug, Clone)]
pub struct ComparatorConfig {
    /// Gross edge floor for emitting a candidate (the gates recompute the
    /// NET edge; this floor just suppresses noise).
    pub min_edge_cents: i64,
    /// Minimum edge tier the strategy accepts (multi-leg/cross-venue
    /// compositions demand Confirmed, spec 5.12).
    pub required_tier: EdgeTier,
}

/// One UNSIZED trade candidate (the comparator's output; the runner
/// sizes and gates it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeCandidate {
    pub market: String,
    pub event_id: String,
    pub belief_id: String,
    pub side: Side,
    /// The belief-implied value of the candidate side, own-side cents.
    pub fair_cents: i64,
    /// The displayed price cap (own-side ask).
    pub max_price_cents: i64,
    pub edge_cents: i64,
}

/// Compare fresh calibrated beliefs to live prices through the edges.
/// Two-sided: a belief far below the market buys NO, far above buys YES.
/// Direct and Negation mappings only (bracket-component and
/// conditional-on carry composite semantics the v1 comparator must not
/// guess at — they are skipped, never mispriced).
pub fn compare_beliefs_to_markets(
    beliefs: &[BeliefView],
    edges: &[EdgeView],
    quotes: &[MarketQuote],
    config: &ComparatorConfig,
) -> Vec<EdgeCandidate> {
    let mut out = Vec::new();
    for belief in beliefs {
        if belief.freshness != Freshness::Fresh {
            continue;
        }
        for edge in edges.iter().filter(|e| e.event_id == belief.event_id) {
            if !edge.tier.satisfies(config.required_tier) {
                continue;
            }
            let market_p = match edge.mapping {
                MappingType::Direct => belief.p,
                MappingType::Negation => 1.0 - belief.p,
                MappingType::BracketComponent | MappingType::ConditionalOn => continue,
            };
            let Some(quote) = quotes.iter().find(|q| q.market == edge.market) else {
                continue;
            };
            // Integer fair value, floor (conservative: never round an
            // edge into existence).
            let fair_yes = (market_p * 100.0).floor() as i64;
            let fair_yes = fair_yes.clamp(0, 100);

            // Buy YES when fair exceeds the displayed ask by the floor.
            if quote.yes_ask_cents > 0 && fair_yes - quote.yes_ask_cents >= config.min_edge_cents {
                out.push(EdgeCandidate {
                    market: edge.market.clone(),
                    event_id: belief.event_id.clone(),
                    belief_id: belief.belief_id.clone(),
                    side: Side::Yes,
                    fair_cents: fair_yes,
                    max_price_cents: quote.yes_ask_cents,
                    edge_cents: fair_yes - quote.yes_ask_cents,
                });
            }
            // Buy NO when the NO fair exceeds the NO ask (= 100 - yes bid).
            let fair_no = 100 - fair_yes;
            let no_ask = 100 - quote.yes_bid_cents;
            if quote.yes_bid_cents > 0 && fair_no - no_ask >= config.min_edge_cents {
                out.push(EdgeCandidate {
                    market: edge.market.clone(),
                    event_id: belief.event_id.clone(),
                    belief_id: belief.belief_id.clone(),
                    side: Side::No,
                    fair_cents: fair_no,
                    max_price_cents: no_ask,
                    edge_cents: fair_no - no_ask,
                });
            }
        }
    }
    out
}

/// The spec 5.14 sizing haircut: fractional Kelly (base, default 0.25)
/// scaled by category calibration quality in [0,1]. Quality outside the
/// unit interval clamps; NaN fails CLOSED to zero (an unmeasured
/// calibration earns no size).
pub fn haircut_kelly_fraction(base_fraction: f64, calibration_quality: f64) -> f64 {
    if !calibration_quality.is_finite() || !base_fraction.is_finite() {
        return 0.0;
    }
    let quality = calibration_quality.clamp(0.0, 1.0);
    (base_fraction * quality).max(0.0)
}

/// The triage tier's verdict for one trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageVerdict {
    Accepted,
    Declined,
}

/// v1 triage policies: rule stubs now, a cheap-model Mind behind the
/// same enum when the live composition wires it (the verdict shape and
/// the scoring contract do not change).
#[derive(Debug, Clone, Copy)]
pub enum TriageDecision {
    AlwaysAccept,
    AlwaysDecline,
}

impl TriageDecision {
    fn assess(&self) -> TriageVerdict {
        match self {
            TriageDecision::AlwaysAccept => TriageVerdict::Accepted,
            TriageDecision::AlwaysDecline => TriageVerdict::Declined,
        }
    }
}

const DAY_MS: i64 = 86_400_000;

/// Deterministic declined-trigger sampler: the FIRST `daily_quota`
/// declined triggers of each UTC day shadow-run the full cycle (spec
/// 5.8: triage recall is measured, not believed). First-K is
/// deterministic and replayable; a random sample would need a seed and
/// buys nothing at these volumes (ASSUMPTIONS).
#[derive(Debug, Clone)]
pub struct ShadowSampler {
    daily_quota: u32,
    sampled_today: u32,
    day_epoch: i64,
}

impl ShadowSampler {
    pub fn new(daily_quota: u32) -> ShadowSampler {
        ShadowSampler {
            daily_quota,
            sampled_today: 0,
            day_epoch: -1,
        }
    }

    pub fn should_shadow(&mut self, now: UtcTimestamp) -> bool {
        let day = now.epoch_millis().div_euclid(DAY_MS);
        if day != self.day_epoch {
            self.day_epoch = day;
            self.sampled_today = 0;
        }
        if self.sampled_today < self.daily_quota {
            self.sampled_today += 1;
            true
        } else {
            false
        }
    }
}

/// One completed (or declined) cycle's artifacts. The caller persists
/// beliefs (supersession via the ledger), audits the triage verdict, and
/// forwards candidates into sizing + gates. `shadow` runs are scored
/// normally but NEVER trade.
#[derive(Debug)]
pub struct CycleOutcome {
    pub triage: TriageVerdict,
    pub shadow: bool,
    pub beliefs: Vec<BeliefDraft>,
    pub candidates: Vec<EdgeCandidate>,
    pub manifest_hash: String,
    pub cost_cents: i64,
}

/// The per-event decision cycle. Serialization (one in flight per event)
/// and debounce live in the TriggerEngine (T2.2); this struct owns what
/// happens after a trigger FIRES.
pub struct DecisionCycle {
    triage: TriageDecision,
    sampler: ShadowSampler,
    comparator: ComparatorConfig,
    assembler: AssemblerConfig,
}

impl DecisionCycle {
    pub fn new(
        triage: TriageDecision,
        sampler: ShadowSampler,
        comparator: ComparatorConfig,
    ) -> DecisionCycle {
        DecisionCycle {
            triage,
            sampler,
            comparator,
            assembler: AssemblerConfig {
                budget_chars: 100_000,
                anonymize: false,
            },
        }
    }

    /// Run one cycle for a fired trigger on `event_id`. The mind's
    /// beliefs become candidates only on a NON-shadow accepted run.
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &mut self,
        event_id: &str,
        mind: &dyn Mind,
        context_items: &[ContextItem],
        edges: &[EdgeView],
        quotes: &[MarketQuote],
        now: UtcTimestamp,
    ) -> Result<CycleOutcome, CycleError> {
        let triage = self.triage.assess();
        let shadow = match triage {
            TriageVerdict::Accepted => false,
            TriageVerdict::Declined => {
                if !self.sampler.should_shadow(now) {
                    // Plain decline: recorded, no mind call, no cost.
                    return Ok(CycleOutcome {
                        triage,
                        shadow: false,
                        beliefs: Vec::new(),
                        candidates: Vec::new(),
                        manifest_hash: String::new(),
                        cost_cents: 0,
                    });
                }
                true
            }
        };

        let ctx = assemble_context(context_items, now, "decision", &self.assembler)?;
        let output = mind.decide(&ctx).await?;

        // Comparator inputs: the freshly minted beliefs are fresh by
        // construction this tick; calibration (T2.8) adjusts p upstream
        // of this view in the live composition.
        let views: Vec<BeliefView> = output
            .beliefs
            .iter()
            .filter(|b| b.event_id == event_id || event_id.is_empty())
            .map(|b| BeliefView {
                belief_id: format!("draft-{}", b.event_id),
                event_id: b.event_id.clone(),
                p: b.p,
                freshness: Freshness::Fresh,
            })
            .collect();
        let candidates = if shadow {
            Vec::new() // shadow runs are scored, never traded
        } else {
            compare_beliefs_to_markets(&views, edges, quotes, &self.comparator)
        };

        Ok(CycleOutcome {
            triage,
            shadow,
            beliefs: output.beliefs,
            candidates,
            manifest_hash: ctx.manifest_hash,
            cost_cents: output.cost_cents,
        })
    }
}
