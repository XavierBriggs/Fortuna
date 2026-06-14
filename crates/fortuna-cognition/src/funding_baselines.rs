//! §2.6 A2d — the funding_forecast baseline comparison (the edge / I7-spirit gate).
//!
//! funding_forecast has NO edge unless its CRPS BEATS naive baselines on the same
//! resolved window (design §2.6 A2d). Until it measurably does, it stays
//! DATA-ONLY — no promotion past Sim, the operator's call on the measured result
//! (I7, never automatic). The carry-forward baseline — the venue ESTIMATE
//! projected FLAT to settlement (the §2.3 authoritative input, unchanged) — is
//! THE bar.
//!
//! This is the pure KERNEL (f64 forecast-domain; no money, no DB, no loop). It
//! REUSES the existing `crps_pinball` proper scoring rule — there is no scoring
//! math here. The wiring (persisting `belief_scores` rows + the live resolve/score
//! loop) is a follow-on (SLICE 3, see GAPS); the last-realized-rate and
//! random-walk baselines are SLICE 2.

use crate::scoring::{
    CrpsPinballRule, PredictiveDistribution, PredictiveKind, Quantile, RealizedOutcome, ScoreError,
    ScoringRule,
};

/// The side-by-side CRPS of funding_forecast vs the carry-forward baseline on one
/// resolved window. CRPS is LOWER-IS-BETTER, so `beats_carry_forward` is exactly
/// `forecast_crps < carry_forward_crps`: a TIE does NOT beat — the forecast must
/// strictly improve on carrying the estimate forward to earn its edge.
#[derive(Debug, Clone, PartialEq)]
pub struct CarryForwardComparison {
    /// funding_forecast's CRPS against the realized rate (lower is better).
    pub forecast_crps: f64,
    /// The carry-forward baseline's CRPS against the SAME realized rate.
    pub carry_forward_crps: f64,
    /// `forecast_crps < carry_forward_crps` — the forecast strictly beat the bar.
    pub beats_carry_forward: bool,
}

/// Compare funding_forecast's scalar `forecast` against the carry-forward baseline
/// (the venue `estimate` projected FLAT to settlement) on the SAME `realized`
/// funding rate, both scored by `crps_pinball`.
///
/// The carry-forward baseline is a DEGENERATE scalar — every quantile value equals
/// `estimate` — over the SAME q-levels as `forecast`, so the discretized CRPS is
/// apples-to-apples. Pure + deterministic: same inputs always yield the same
/// result, no clock/IO.
///
/// Errors (never a silent NaN): a non-Scalar `forecast` is a `KindMismatch`
/// (funding_forecast is always Scalar); a `forecast` or `estimate` that fails
/// `validate()` (e.g. a non-finite `estimate`) is an `InvalidPrediction`.
pub fn compare_against_carry_forward(
    forecast: &PredictiveDistribution,
    estimate: f64,
    realized: f64,
) -> Result<CarryForwardComparison, ScoreError> {
    // Scalar-only by design: extract the q-levels up front so the baseline shares
    // them. A non-Scalar forecast paired with a scalar realized is a kind mismatch.
    let PredictiveDistribution::Scalar { quantiles, unit } = forecast else {
        return Err(ScoreError::KindMismatch {
            pred_kind: forecast.kind(),
            outcome_kind: PredictiveKind::Scalar,
        });
    };

    let rule = CrpsPinballRule;
    let outcome = RealizedOutcome::Scalar { value: realized };

    // `score` validates each prediction internally (rejects, never repairs).
    let forecast_crps = rule.score(forecast, &outcome)?;

    // Carry-forward: the estimate projected flat — a point mass at `estimate` over
    // the forecast's own q-levels. q strictly increasing (inherited); v all equal
    // => non-decreasing; finite iff `estimate` is. `validate` (run inside `score`)
    // surfaces a non-finite `estimate` as InvalidPrediction.
    let carry_forward = PredictiveDistribution::Scalar {
        quantiles: quantiles
            .iter()
            .map(|q| Quantile {
                q: q.q,
                v: estimate,
            })
            .collect(),
        unit: unit.clone(),
    };
    let carry_forward_crps = rule.score(&carry_forward, &outcome)?;

    Ok(CarryForwardComparison {
        forecast_crps,
        carry_forward_crps,
        beats_carry_forward: forecast_crps < carry_forward_crps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A valid scalar forecast from (q, v) pairs (unit "rate", the funding domain).
    fn scalar(qvs: &[(f64, f64)]) -> PredictiveDistribution {
        PredictiveDistribution::Scalar {
            quantiles: qvs.iter().map(|&(q, v)| Quantile { q, v }).collect(),
            unit: "rate".to_string(),
        }
    }

    #[test]
    fn a_forecast_centered_on_realized_beats_a_far_carry_forward() {
        // funding_forecast nails the realized rate (a tight fan around it); the
        // venue estimate (carry-forward) is far off. The forecast EARNS its edge.
        let realized = 0.0010;
        let forecast = scalar(&[(0.25, 0.0009), (0.5, 0.0010), (0.75, 0.0011)]);
        let estimate = 0.0050; // far above realized
        let c = compare_against_carry_forward(&forecast, estimate, realized).unwrap();
        assert!(c.forecast_crps < c.carry_forward_crps, "{c:?}");
        assert!(
            c.beats_carry_forward,
            "a centered forecast beats a far carry-forward: {c:?}"
        );
    }

    #[test]
    fn a_carry_forward_on_target_beats_a_wild_forecast() {
        // The venue estimate IS the realized rate (carry-forward perfect, CRPS 0);
        // the forecast fan is wild/off. Carry-forward wins — funding_forecast has
        // NO edge here, so it stays DATA-ONLY.
        let realized = 0.0010;
        let forecast = scalar(&[(0.25, 0.0040), (0.5, 0.0050), (0.75, 0.0060)]);
        let estimate = 0.0010; // exactly realized
        let c = compare_against_carry_forward(&forecast, estimate, realized).unwrap();
        assert!(c.carry_forward_crps < c.forecast_crps, "{c:?}");
        assert!(
            !c.beats_carry_forward,
            "a wild forecast does NOT beat an on-target carry-forward: {c:?}"
        );
    }

    #[test]
    fn the_comparison_is_computed_both_crps_finite_and_nonnegative() {
        // Both legs scored: the comparison is COMPUTED (non-vacuous); CRPS is the
        // proper rule's finite, non-negative loss on each side.
        let forecast = scalar(&[(0.1, -0.001), (0.5, 0.0), (0.9, 0.001)]);
        let c = compare_against_carry_forward(&forecast, 0.0005, 0.0002).unwrap();
        assert!(
            c.forecast_crps.is_finite() && c.forecast_crps >= 0.0,
            "{c:?}"
        );
        assert!(
            c.carry_forward_crps.is_finite() && c.carry_forward_crps >= 0.0,
            "{c:?}"
        );
    }

    #[test]
    fn a_perfect_carry_forward_scores_zero_and_a_tie_does_not_beat() {
        // A degenerate carry-forward AT the realized value scores CRPS 0 (a perfect
        // point mass). A forecast that is ALSO a point mass at realized ties — and
        // a tie does NOT beat (the strict `<` guard).
        let realized = 0.0010;
        let forecast = scalar(&[(0.25, 0.0010), (0.5, 0.0010), (0.75, 0.0010)]);
        let c = compare_against_carry_forward(&forecast, realized, realized).unwrap();
        assert_eq!(
            c.carry_forward_crps, 0.0,
            "a point mass at realized scores 0: {c:?}"
        );
        assert!(
            !c.beats_carry_forward,
            "a tie does not beat the bar (strict <): {c:?}"
        );
    }

    #[test]
    fn a_non_scalar_forecast_is_a_kind_mismatch() {
        // The kernel is scalar-only (funding_forecast is always Scalar); a Binary
        // forecast is a KindMismatch, never a silent NaN comparison.
        let forecast = PredictiveDistribution::Binary { p: 0.5 };
        let err = compare_against_carry_forward(&forecast, 0.001, 0.001).unwrap_err();
        assert!(
            matches!(err, ScoreError::KindMismatch { .. }),
            "got {err:?}"
        );
    }
}
