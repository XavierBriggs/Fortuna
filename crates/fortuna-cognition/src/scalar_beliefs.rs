//! The SCALAR analog of `beliefs::BeliefDraft` (perp-strategies-and-scalar-
//! claims design §1, §2.5).
//!
//! `BeliefDraft` is binary-only — it carries a REQUIRED `p: f64` validated
//! strictly in (0,1) and `deny_unknown_fields`, so a scalar
//! `PredictiveDistribution` cannot flow through it, and (per the design's
//! "binary path untouched, no track-A collision" constraint) it must NOT be
//! widened. `ScalarBeliefDraft` is the NEW additive shape that carries the
//! scalar claim out through the parallel `Strategy::drain_scalar_beliefs()`
//! egress seam (the must-fix A3 of the design critique).
//!
//! Consumer-agnostic by construction: funding_forecast emits it now;
//! Aeolus weather and track-E personas emit the identical shape later (the
//! "design once" result, design §1.5). The harness stamps `provenance`
//! after the producing cycle, exactly as it does for `BeliefDraft`.

use crate::scoring::PredictiveDistribution;
use fortuna_core::clock::UtcTimestamp;
use serde::{Deserialize, Serialize};

/// What a scalar belief-producer emits (pre-persistence) — the scalar mirror
/// of `beliefs::BeliefDraft`. Unknown fields are REJECTED (I6 schema
/// discipline), matching `BeliefDraft`.
///
/// `predictive` holds a `PredictiveDistribution`; in practice it MUST be the
/// `Scalar` variant (the harness/persist boundary validates that — the
/// `scalar_beliefs` table only stores quantile claims). This type stays a
/// plain carrier so the validation lives in one place, the same split
/// `BeliefDraft::validate` uses for the binary probability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalarBeliefDraft {
    /// The forecast subject (e.g. the funding-rate event key for a market's
    /// next finalized rate). Mirrors `BeliefDraft.event_id`; named per the
    /// design's `scalar_beliefs.event_key` column (§1.4).
    pub event_key: String,
    /// The scalar predictive claim (the quantile fan over the next realized
    /// value). MUST be `PredictiveDistribution::Scalar` in practice; the
    /// harness validates the shape before persisting.
    pub predictive: PredictiveDistribution,
    /// When the belief resolves (the realized value is observed) — for
    /// funding_forecast, `next_funding_time`. Mirrors `BeliefDraft.horizon`.
    pub horizon: UtcTimestamp,
    /// Supporting evidence — DATA (spec 5.11 discipline), never instructions.
    pub evidence: serde_json::Value,
    /// Stamped by the HARNESS after the producing cycle ({producer,
    /// context/manifest detail, ...}) — the producer cannot know its own
    /// provenance, so this is never part of the producer's schema. Mirrors
    /// `BeliefDraft.provenance` exactly (`#[serde(default)]`).
    #[serde(default)]
    pub provenance: serde_json::Value,
}
