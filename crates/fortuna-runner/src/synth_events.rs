//! synth_events (spec Section 6, item 4): "low-attention, retail-
//! dominated event markets where consensus is weak; pure information-
//! synthesis beliefs. This is the strategy that scales with model
//! improvement; it earns live capital only through the full gate
//! sequence."
//!
//! synth_events IS a `SynthesisStrategy` (the T2.6 decision cycle
//! behind the Strategy trait) with a named configuration: confirmed
//! edges only, a real edge floor, and a PAPER stage cap — "paper-only
//! initially" is the declared ceiling; the EFFECTIVE stage starts at
//! Sim and rises only through operator promotion records
//! (`promotion::effective_stage`, I7). Market selection (low-attention,
//! weak-consensus) is the discovery loop's tradability output feeding
//! the edge list (T3.2); the strategy itself trades whatever confirmed
//! edges the composition hands it.

use crate::synthesis::SynthesisConfig;
use crate::{RunnerError, Stage};
use fortuna_cognition::cycle::{ComparatorConfig, EdgeView, TriageDecision};
use fortuna_cognition::events::EdgeTier;
use fortuna_core::market::StrategyId;

/// The synth_events stage CAP (spec: paper-only initially). Raising
/// this cap is a spec/config change; the effective stage additionally
/// requires operator promotion records.
pub const SYNTH_EVENTS_STAGE_CAP: Stage = Stage::Paper;

/// Minimum comparator edge for synth_events candidates (cents). Weak-
/// consensus markets are wide; a thin edge in a wide market is noise.
pub const SYNTH_EVENTS_MIN_EDGE_CENTS: i64 = 5;

/// Declined-trigger shadow runs per UTC day (triage recall measurement).
pub const SYNTH_EVENTS_SHADOW_QUOTA: u32 = 3;

/// The named synth_events configuration. Edges come from the discovery
/// loop's confirmed output; triage comes from the composition (the
/// cheap-tier mind in live, a scripted decision in tests). Construction
/// is boot-time and fallible like every composition constructor.
pub fn synth_events_config(
    edges: Vec<EdgeView>,
    triage: TriageDecision,
) -> Result<SynthesisConfig, RunnerError> {
    Ok(SynthesisConfig {
        id: StrategyId::new("synth_events").map_err(|e| RunnerError::Config {
            reason: e.to_string(),
        })?,
        edges,
        comparator: ComparatorConfig {
            min_edge_cents: SYNTH_EVENTS_MIN_EDGE_CENTS,
            // Synthesis trades CONFIRMED edges only by default: a wrong
            // equivalence converts conviction into an unhedged position.
            required_tier: EdgeTier::Confirmed,
        },
        triage,
        shadow_quota: SYNTH_EVENTS_SHADOW_QUOTA,
        stage: SYNTH_EVENTS_STAGE_CAP,
    })
}
