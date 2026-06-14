//! F9: Layer-3 empirical source-reliability scoring (source contract
//! `docs/design/aeolus-fortuna-source-contract.md` §5 Layer 3 — THE LOOP).
//!
//! Aeolus SELF-REPORTS its skill (`skill.*`); FORTUNA INDEPENDENTLY re-scores
//! every Aeolus belief at settlement against the realized temperature (the
//! independent NWS grader, §3.2/§5 — NOT Aeolus, so beliefs are not self-graded,
//! the V4 caution). This module computes that measured reliability per
//! `(model, scope)` — the metric the ROTA Layer-3 scorecard reads:
//!
//! - **Binary Brier** per `ge`/`lt` bracket: the realized integer daily
//!   high/low either satisfies the bracket or not (`ge t` ⟺ `realized ≥ t`); the
//!   belief's own `p` (FORTUNA's μ/σ probability, the F6 helpers) is scored by
//!   `brier_score` against that 0/1 outcome.
//! - **Scalar CRPS** of the μ/σ quantile fan (F8's scalar belief) against the
//!   realized value, via the pinned `CrpsPinballRule`.
//!
//! The realized temperature is an INPUT here: extracting the official daily
//! high/low from the NWS CLI product (`productText` → °F, the F2 grader) is a
//! source-side concern not yet in cognition (ledgered in GAPS as a seam); F9
//! takes the graded value and the e2e supplies a RECORDED one. Pure +
//! deterministic; no `Clock::now`, no panic (a scoring error degrades to `None`).

use crate::aeolus_beliefs::emit_aeolus_beliefs;
use crate::aeolus_forecast::{
    bracket_prob_ge, bracket_prob_lt, AeolusForecast, Comparison, Variable,
};
use crate::beliefs::brier_score;
use crate::scoring::{CrpsPinballRule, RealizedOutcome, ScoringRule};

/// The Layer-3 scoring scope — the `(model, …)` key the ROTA scorecard groups by.
/// Mirrors the belief provenance F8 stamps (`model_id`/`model_version`/station/
/// variable), so a measured score attributes to the exact forecast model.
#[derive(Debug, Clone, PartialEq)]
pub struct ReliabilityScope {
    pub model_id: String,
    pub model_version: String,
    pub station: String,
    pub variable: Variable,
    pub target_date: String,
}

/// One bracket's realized score.
#[derive(Debug, Clone, PartialEq)]
pub struct BracketScore {
    pub event_id: String,
    pub threshold_f: i64,
    pub comparison: Comparison,
    /// FORTUNA's own μ/σ probability for the bracket (the belief's `p`).
    pub p_fortuna: f64,
    /// Whether the realized integer high/low satisfied the bracket.
    pub outcome: bool,
    /// `(p − outcome)²` — lower is better.
    pub brier: f64,
}

/// The measured reliability of one Aeolus forecast against its realized outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct AeolusReliability {
    pub scope: ReliabilityScope,
    /// The official realized daily high/low (°F) the beliefs were graded against.
    pub realized_f: f64,
    pub n_brackets: usize,
    /// Mean Brier over the scored `ge`/`lt` brackets (`0.0` when none scored).
    pub brier_mean: f64,
    /// Scalar CRPS of the μ/σ fan vs `realized_f`; `None` only on a (post-parse
    /// impossible) scoring error — never a panic.
    pub crps: Option<f64>,
    pub per_bracket: Vec<BracketScore>,
}

/// Whether the realized integer high/low satisfies a bracket. `ge t ⟺ realized ≥
/// t`, `lt t ⟺ realized < t`. `in_bracket` needs a floor+cap pair a single
/// threshold can't supply → `None` (skipped, mirroring F8).
///
/// `pub` so the resolution bridge (`aeolus_resolve::score_bracket`) scores open
/// beliefs by the SAME outcome rule — one source of truth for `ge`/`lt`/
/// `in_bracket`, so the live resolver and this scorecard can never drift.
pub fn bracket_outcome(threshold_f: i64, comparison: Comparison, realized_f: f64) -> Option<bool> {
    let t = threshold_f as f64;
    match comparison {
        Comparison::Ge => Some(realized_f >= t),
        Comparison::Lt => Some(realized_f < t),
        Comparison::InBracket => None,
    }
}

/// Score one Aeolus forecast against its realized temperature: Brier per `ge`/`lt`
/// bracket (the belief's own μ/σ probability vs the realized 0/1 outcome) + CRPS
/// of the scalar μ/σ fan vs the realized value. Deterministic; never panics.
pub fn score_reliability(fc: &AeolusForecast, realized_f: f64) -> AeolusReliability {
    let mu = fc.mu();
    let sigma = fc.sigma();

    let mut per_bracket = Vec::with_capacity(fc.brackets().len());
    for bracket in fc.brackets() {
        let p = match bracket.comparison {
            Comparison::Ge => bracket_prob_ge(bracket.threshold_f, mu, sigma),
            Comparison::Lt => bracket_prob_lt(bracket.threshold_f, mu, sigma),
            Comparison::InBracket => None,
        };
        // `ge`/`lt` with σ>0 (a parse invariant) ⇒ both `Some`; an `in_bracket`
        // bracket (or a degenerate σ that parse already rejected) is skipped.
        let (Some(p), Some(outcome)) = (
            p,
            bracket_outcome(bracket.threshold_f, bracket.comparison, realized_f),
        ) else {
            continue;
        };
        per_bracket.push(BracketScore {
            event_id: format!("aeolus:{}", bracket.event_hint),
            threshold_f: bracket.threshold_f,
            comparison: bracket.comparison,
            p_fortuna: p,
            outcome,
            brier: brier_score(p, outcome),
        });
    }

    let n_brackets = per_bracket.len();
    let brier_mean = if n_brackets == 0 {
        0.0
    } else {
        per_bracket.iter().map(|b| b.brier).sum::<f64>() / n_brackets as f64
    };

    // CRPS of F8's scalar μ/σ fan vs the realized value (the SAME fan F8 emits, so
    // the scored object is exactly the persisted scalar belief). A scoring error
    // (impossible for a validated scalar + finite realized) degrades to None.
    let scalar = emit_aeolus_beliefs(fc).scalar;
    let crps = CrpsPinballRule
        .score(
            &scalar.predictive,
            &RealizedOutcome::Scalar { value: realized_f },
        )
        .ok();

    AeolusReliability {
        scope: ReliabilityScope {
            model_id: "aeolus".to_string(),
            model_version: fc.distribution().model_version.clone(),
            station: fc.station().to_string(),
            variable: fc.variable(),
            target_date: fc.target_date().to_string(),
        },
        realized_f,
        n_brackets,
        brier_mean,
        crps,
        per_bracket,
    }
}
