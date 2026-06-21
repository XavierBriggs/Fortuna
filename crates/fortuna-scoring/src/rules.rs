//! Swappable scoring layer for predictive distributions (spec 5.5, 5.15;
//! design: docs/design/perp-strategies-and-scalar-claims.md §1).
//!
//! Two concerns are kept strictly separate:
//!
//! 1. **`PredictiveDistribution`** — the *durable, immutable forecast claim*.
//!    Written once, never mutated.  Supports Binary, Categorical, and Scalar
//!    (quantile-based) shapes.  The `prob_claims/v1` schema discipline applies:
//!    `deny_unknown_fields` on the envelope so unknown wires are a hard error,
//!    not silently dropped data.
//!
//! 2. **`ScoringRule`** — the *swappable scoring function*.  Rules are pure
//!    functions (`score(pred, outcome) → f64`).  Several rules can be run
//!    side-by-side over the same immutable `(pred, outcome)` pair without
//!    touching storage (re-scorability is the point).  Lower is better for all
//!    rules shipped here.
//!
//! These are **cognition-domain `f64` forecast quantities, never money**.
//! When a scalar forecast later informs a perp order, the `f64 → PerpPrice /
//! Cents` conversion happens at the gate/exec boundary with the established
//! rounding-against-us discipline — not here.
//!
//! # Shipped rules
//!
//! - [`BrierRule`] — binary Brier score `(p − o)²` (lower is better).
//! - [`CrpsPinballRule`] — TRUE CRPS via the pinball / quantile-loss
//!   discretization (`2·Σ pinball·Δτ`; equal grid → `(2/K)·Σ`) for scalar
//!   forecasts.  A proper scoring rule; lower is better.
//!
//! Adding a new rule (log-loss, weighted-CRPS, categorical Brier, …) is a new
//! `impl ScoringRule` — no schema change.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

// ─── error type ──────────────────────────────────────────────────────────────

/// Errors returned by scoring operations.
///
/// The three variants cover every failure mode the spec calls out:
/// kind-mismatch, unsupported-kind, and invalid-prediction.  All variants
/// carry a human-readable message so callers can log and audit without
/// inspecting the variant structure.
#[derive(Debug, Error)]
pub enum ScoreError {
    /// The prediction's outcome shape and the realized outcome's shape do not
    /// match — e.g., a Binary prediction paired with a Scalar outcome.
    #[error("kind mismatch: prediction is {pred_kind:?} but outcome is {outcome_kind:?}")]
    KindMismatch {
        pred_kind: PredictiveKind,
        outcome_kind: PredictiveKind,
    },

    /// This scoring rule does not support the given outcome shape — e.g.,
    /// `BrierRule` applied to a Scalar forecast.
    #[error("scoring rule does not support {kind:?} distributions")]
    UnsupportedKind { kind: PredictiveKind },

    /// The prediction failed its own `validate()` check.  Never silently
    /// coerce: schema-invalid output is rejected (spec 5.9).
    #[error("invalid prediction: {reason}")]
    InvalidPrediction { reason: String },
}

// ─── PredictiveKind ───────────────────────────────────────────────────────────

/// Discriminant for the outcome shape.  Used by `ScoringRule::applies_to` and
/// for kind-mismatch checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredictiveKind {
    Binary,
    Categorical,
    Scalar,
}

// ─── sub-types ───────────────────────────────────────────────────────────────

/// One bin in a Categorical distribution: a label and its probability mass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoricalBin {
    /// Non-empty, unique within the distribution.
    pub label: String,
    /// Probability in [0, 1].
    pub p: f64,
}

/// One quantile level in a Scalar distribution.
///
/// `q` is the quantile level, strictly inside (0, 1).
/// `v` is the forecast value at that level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quantile {
    /// Quantile level: must be strictly inside (0, 1) and strictly increasing
    /// across the quantile vector.
    pub q: f64,
    /// Forecast value at level `q`: must be non-decreasing across the vector
    /// (no quantile crossing) and finite.
    pub v: f64,
}

// ─── PredictiveDistribution ───────────────────────────────────────────────────

/// The durable predictive claim by outcome shape (`prob_claims/v1`).
///
/// `deny_unknown_fields` is the I6 schema discipline applied to the
/// deserialized claim: unknown fields are a hard error, not silently ignored
/// data.  The `tag` is the discriminant used by the persistent store and the
/// strict mapper from external signal envelopes.
///
/// All three shapes live in the same enum; the active shape determines which
/// `ScoringRule` applies.  Call `validate()` before any scoring operation —
/// `score()` on any `ScoringRule` does this internally.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PredictiveDistribution {
    /// Binary outcome: probability of the event occurring.
    ///
    /// `p` must be finite and strictly inside (0, 1) — a model claiming
    /// certainty is schema-invalid output, rejected (spec 5.9).
    Binary { p: f64 },

    /// Categorical outcome: a finite set of labelled alternatives with
    /// probabilities summing to 1.
    ///
    /// Labels must be non-empty and unique.  Each `p` must be finite and in
    /// [0, 1].  The sum of all `p` must be within 1e-9 of 1.0 (float rounding
    /// tolerance).
    Categorical { bins: Vec<CategoricalBin> },

    /// Scalar outcome: a quantile-based predictive distribution.
    ///
    /// `quantiles` must have at least 2 entries.  `q` values must be strictly
    /// increasing and strictly inside (0, 1).  `v` values must be
    /// non-decreasing (no quantile crossing) and finite.  `unit` must be
    /// non-empty (e.g., "rate", "celsius").
    Scalar {
        quantiles: Vec<Quantile>,
        unit: String,
    },
}

impl PredictiveDistribution {
    /// Returns the discriminant for this distribution.
    pub fn kind(&self) -> PredictiveKind {
        match self {
            PredictiveDistribution::Binary { .. } => PredictiveKind::Binary,
            PredictiveDistribution::Categorical { .. } => PredictiveKind::Categorical,
            PredictiveDistribution::Scalar { .. } => PredictiveKind::Scalar,
        }
    }

    /// Validates the distribution against its shape's invariants.
    ///
    /// Schema-invalid distributions are REJECTED, never silently repaired
    /// (spec 5.9).  Returns `Err(ScoreError::InvalidPrediction { reason })`
    /// with a descriptive message on any violation.
    pub fn validate(&self) -> Result<(), ScoreError> {
        match self {
            PredictiveDistribution::Binary { p } => validate_binary_p(*p),
            PredictiveDistribution::Categorical { bins } => validate_categorical(bins),
            PredictiveDistribution::Scalar { quantiles, unit } => validate_scalar(quantiles, unit),
        }
    }
}

// ─── RealizedOutcome ──────────────────────────────────────────────────────────

/// The realized result when a belief resolves.
///
/// Recorded once, immutably.  Must match the `PredictiveDistribution`'s shape
/// (same `kind()`) before any scoring; mismatches are a `ScoreError::KindMismatch`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RealizedOutcome {
    /// Binary event: did it happen?
    Binary { happened: bool },
    /// Categorical event: which label was realized?
    Categorical { label: String },
    /// Scalar event: what was the realized value (e.g., final funding rate,
    /// realized temperature)?
    Scalar { value: f64 },
}

impl RealizedOutcome {
    /// Returns the kind discriminant for this outcome.
    pub fn kind(&self) -> PredictiveKind {
        match self {
            RealizedOutcome::Binary { .. } => PredictiveKind::Binary,
            RealizedOutcome::Categorical { .. } => PredictiveKind::Categorical,
            RealizedOutcome::Scalar { .. } => PredictiveKind::Scalar,
        }
    }
}

// ─── ScoringRule trait ────────────────────────────────────────────────────────

/// The swappable scoring abstraction.
///
/// Each rule is a pure function: same `(pred, outcome)` → same `f64` on every
/// call.  LOWER IS BETTER for all rules shipped here.
///
/// Rules must:
/// 1. Call `pred.validate()` and surface an `InvalidPrediction` error.
/// 2. Check that `pred.kind() == outcome.kind()` and return `KindMismatch` if
///    not.
/// 3. Check `applies_to(pred.kind())` and return `UnsupportedKind` if not.
///
/// The first two checks ensure callers never get a silent NaN or wrong answer
/// when pairing incompatible types.
pub trait ScoringRule {
    /// Stable identifier, e.g. `"brier"`, `"crps_pinball"`.
    ///
    /// This is stored in the `belief_scores` table as `rule_id` so the row is
    /// self-describing.
    fn id(&self) -> &'static str;

    /// Returns true iff this rule can score a `PredictiveDistribution` of the
    /// given kind.
    fn applies_to(&self, kind: PredictiveKind) -> bool;

    /// Scores one `(prediction, outcome)` pair.  LOWER IS BETTER.
    ///
    /// Returns `Err` for:
    /// - A prediction whose `kind()` does not match the outcome's `kind()`.
    /// - A rule applied to a kind it does not handle.
    /// - A prediction that fails `validate()`.
    fn score(
        &self,
        pred: &PredictiveDistribution,
        outcome: &RealizedOutcome,
    ) -> Result<f64, ScoreError>;
}

// ─── BrierRule ────────────────────────────────────────────────────────────────

/// Binary Brier score: `(p − 1{happened})²`.
///
/// Handles `Binary` distributions only.  Lower is better.
///
/// The Brier score is the proper scoring rule for binary events; a fair-coin
/// claim scores 0.25, a perfect forecast scores 0.0.
pub struct BrierRule;

impl ScoringRule for BrierRule {
    fn id(&self) -> &'static str {
        "brier"
    }

    fn applies_to(&self, kind: PredictiveKind) -> bool {
        kind == PredictiveKind::Binary
    }

    fn score(
        &self,
        pred: &PredictiveDistribution,
        outcome: &RealizedOutcome,
    ) -> Result<f64, ScoreError> {
        // 1. Validate prediction first (spec 5.9 — reject, never repair).
        pred.validate()?;

        // 2. Check rule applicability before kind parity so the error message
        //    is about the rule's scope, not a raw mismatch.
        if !self.applies_to(pred.kind()) {
            return Err(ScoreError::UnsupportedKind { kind: pred.kind() });
        }

        // 3. Check kind parity between prediction and outcome.
        if pred.kind() != outcome.kind() {
            return Err(ScoreError::KindMismatch {
                pred_kind: pred.kind(),
                outcome_kind: outcome.kind(),
            });
        }

        // 4. Compute.
        let p = match pred {
            PredictiveDistribution::Binary { p } => *p,
            _ => unreachable!("applies_to guarantees Binary"),
        };
        let o = match outcome {
            RealizedOutcome::Binary { happened } => {
                if *happened {
                    1.0_f64
                } else {
                    0.0_f64
                }
            }
            _ => unreachable!("kind parity check guarantees Binary"),
        };
        Ok((p - o) * (p - o))
    }
}

// ─── CrpsPinballRule ──────────────────────────────────────────────────────────

/// TRUE CRPS via the pinball / quantile-loss discretization for scalar
/// forecasts.
///
/// CRPS as a quantile integral is `CRPS = 2·∫₀¹ pinball_τ dτ`.  Discretized on
/// an **equally-spaced grid** of K quantile levels with spacing `Δτ = 1/K`
/// (this rule's normalization convention), that is `(2/K)·Σ` — i.e. **true
/// CRPS = 2·Σ pinball·Δτ; equal grid → (2/K)·Σ**.  The bare mean pinball
/// (`Σ/K`, no factor 2) is exactly ½·CRPS and is NOT what this rule returns.
///
/// Given K quantile levels `(q_k, v_k)` and realized value `y`:
///
/// ```text
/// pinball_q(y, v) = q·(y − v)       if y ≥ v
///                  (1 − q)·(v − y)   otherwise
///
/// score = (2/K) · Σ_k  pinball_{q_k}(y, v_k)
/// ```
///
/// This is a proper scoring rule; lower is better.  (Mathematical aside, not a
/// reachable path: a lone q=0.5 quantile reduces to `|y − v|`, the median's
/// proper score scaled to CRPS — NOT the Brier squared error; the two rules are
/// distinct `ScoringRule` instances, not reductions of each other.  It is an
/// identity only: `validate()` requires ≥2 quantiles, so K=1 is never a scored
/// input.)
///
/// CRPS is consumed only RELATIVELY (one forecast's CRPS compared against a
/// baseline's on the same realized value), so the factor-2 scale is GO-neutral.
///
/// Handles `Scalar` distributions only.
pub struct CrpsPinballRule;

impl ScoringRule for CrpsPinballRule {
    fn id(&self) -> &'static str {
        "crps_pinball"
    }

    fn applies_to(&self, kind: PredictiveKind) -> bool {
        kind == PredictiveKind::Scalar
    }

    fn score(
        &self,
        pred: &PredictiveDistribution,
        outcome: &RealizedOutcome,
    ) -> Result<f64, ScoreError> {
        // 1. Validate prediction (spec 5.9 — reject, never repair).
        pred.validate()?;

        // 2. Check rule applicability.
        if !self.applies_to(pred.kind()) {
            return Err(ScoreError::UnsupportedKind { kind: pred.kind() });
        }

        // 3. Kind parity.
        if pred.kind() != outcome.kind() {
            return Err(ScoreError::KindMismatch {
                pred_kind: pred.kind(),
                outcome_kind: outcome.kind(),
            });
        }

        // 4. Compute the TRUE CRPS = 2·Σ pinball·Δτ.
        let quantiles = match pred {
            PredictiveDistribution::Scalar { quantiles, .. } => quantiles,
            _ => unreachable!("applies_to guarantees Scalar"),
        };
        let y = match outcome {
            RealizedOutcome::Scalar { value } => *value,
            _ => unreachable!("kind parity check guarantees Scalar"),
        };

        let sum: f64 = quantiles
            .iter()
            .map(|Quantile { q, v }| pinball_loss(*q, *v, y))
            .sum();

        // Equally-spaced grid of K levels ⇒ Δτ = 1/K, so
        // CRPS = 2·Σ pinball·Δτ = (2/K)·Σ. The old `Σ/K` was mean pinball
        // (= ½·CRPS, missing the factor 2).
        Ok(2.0 * sum / quantiles.len() as f64)
    }
}

/// Pinball / check loss at one quantile level.
///
/// `pinball_q(y, v) = q·(y − v)` if `y ≥ v`, else `(1 − q)·(v − y)`.
#[inline]
fn pinball_loss(q: f64, v: f64, y: f64) -> f64 {
    if y >= v {
        q * (y - v)
    } else {
        (1.0 - q) * (v - y)
    }
}

// ─── validation helpers ───────────────────────────────────────────────────────

fn invalid(reason: impl Into<String>) -> ScoreError {
    ScoreError::InvalidPrediction {
        reason: reason.into(),
    }
}

fn validate_binary_p(p: f64) -> Result<(), ScoreError> {
    if !p.is_finite() || !(0.0..=1.0).contains(&p) {
        return Err(invalid(format!("binary p={p} is not finite in [0,1]")));
    }
    // Strictly inside (0,1): certainty is schema-invalid (spec 5.9).
    if p == 0.0 || p == 1.0 {
        return Err(invalid(format!(
            "binary p={p} must be strictly inside (0,1)"
        )));
    }
    Ok(())
}

fn validate_categorical(bins: &[CategoricalBin]) -> Result<(), ScoreError> {
    if bins.is_empty() {
        return Err(invalid("categorical bins must be non-empty"));
    }
    let mut labels = HashSet::with_capacity(bins.len());
    let mut sum = 0.0_f64;
    for b in bins {
        if b.label.is_empty() {
            return Err(invalid("categorical bin label must be non-empty"));
        }
        if !labels.insert(b.label.as_str()) {
            return Err(invalid(format!(
                "categorical bin label {:?} is duplicated",
                b.label
            )));
        }
        if !b.p.is_finite() || !(0.0..=1.0).contains(&b.p) {
            return Err(invalid(format!(
                "categorical bin {:?} p={} is not finite in [0,1]",
                b.label, b.p
            )));
        }
        sum += b.p;
    }
    if (sum - 1.0_f64).abs() > 1e-9 {
        return Err(invalid(format!(
            "categorical bin probabilities sum to {sum} (must be within 1e-9 of 1.0)"
        )));
    }
    Ok(())
}

fn validate_scalar(quantiles: &[Quantile], unit: &str) -> Result<(), ScoreError> {
    if quantiles.len() < 2 {
        return Err(invalid(format!(
            "scalar requires at least 2 quantiles, got {}",
            quantiles.len()
        )));
    }
    if unit.is_empty() {
        return Err(invalid("scalar unit must be non-empty"));
    }
    let mut prev_q = f64::NEG_INFINITY;
    let mut prev_v = f64::NEG_INFINITY;
    for (i, Quantile { q, v }) in quantiles.iter().enumerate() {
        // q finite and strictly inside (0,1).
        if !q.is_finite() {
            return Err(invalid(format!("quantile[{i}].q is not finite")));
        }
        if *q <= 0.0 || *q >= 1.0 {
            return Err(invalid(format!(
                "quantile[{i}].q={q} must be strictly inside (0,1)"
            )));
        }
        // q strictly increasing.
        if *q <= prev_q {
            return Err(invalid(format!(
                "quantile[{i}].q={q} is not strictly greater than previous q={prev_q}"
            )));
        }
        // v finite.
        if !v.is_finite() {
            return Err(invalid(format!("quantile[{i}].v is not finite")));
        }
        // v non-decreasing (no quantile crossing).
        if *v < prev_v {
            return Err(invalid(format!(
                "quantile[{i}].v={v} < previous v={prev_v} (quantile crossing)"
            )));
        }
        prev_q = *q;
        prev_v = *v;
    }
    Ok(())
}
