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
//! loop) is a follow-on (SLICE 3, see GAPS).
//!
//! # SLICE 2 — three more naive baselines + the unified edge gate
//!
//! The §2.6 A2d edge gate is "beats ALL the naive baselines", not just
//! carry-forward. SLICE 2 adds three more, and [`compare_against_baselines`] —
//! the unified entry point scoring all FOUR side-by-side over the SAME resolved
//! window — with `beats_all` (strictly beating every one of the four) as the
//! gate the operator reads before any promotion past Sim (I7, never automatic):
//!
//! 1. **last-realized-rate** — a DEGENERATE scalar point mass at the PREVIOUS
//!    resolved window's realized rate (`last_realized`). A pure persistence
//!    forecast: "next window finalizes where the last one did." Distinct from
//!    carry-forward, which anchors on the venue ESTIMATE of the CURRENT window.
//!
//! 2. **random-walk (estimate-anchored)** — a NON-degenerate fan anchored at the
//!    last observed `estimate`, dispersed by a caller-injected, horizon-scaled
//!    `rw_band` (σ·√(time-remaining); the kernel stays pure + parameter-injected,
//!    it does NOT invent a band). The value at each forecast quantile level `q`
//!    is `estimate + Z(q)·rw_band`, `Z(q)` the standard-normal quantile from the
//!    pinned funding-quantile lookup [`standard_normal_z`].
//!
//! 3. **random-walk (persistence-anchored)** — the SAME fan construction, but
//!    anchored at `last_realized` instead of `estimate`: `last_realized +
//!    Z(q)·rw_band`. This is a true persistence-plus-diffusion forecaster (naive
//!    persistence + drift uncertainty), genuinely DISTINCT from funding_forecast
//!    (which anchors at the new `estimate`). It exists to keep the edge gate
//!    ROBUST — see the honest note below.
//!
//! ## Honest note on the two random-walk baselines (read before trusting them)
//!
//! funding_forecast's OWN dispersion is already `DISPERSION_SCALE·√(remaining/
//! window)` — i.e. ~√(time-remaining)-shaped — and anchored at the same
//! `estimate`. So the ESTIMATE-anchored RW with a √-horizon `rw_band` is a
//! NEAR-TWIN of funding_forecast itself: that leg is the WEAK one — it mostly
//! re-tests "is my dispersion width well-chosen", not "do I have signal." It is
//! kept only for completeness of the §2.6 A2d "beats all naive baselines" claim.
//! The robust gate is carried by the OTHER three legs: carry-forward (THE bar),
//! last-rate (persistence point mass), and the PERSISTENCE-anchored RW
//! (persistence + drift) — the last of which anchors its fan at `last_realized`,
//! making it a genuine persistence-plus-diffusion forecaster rather than an
//! estimate echo, so a forecast cannot pass by merely re-deriving its own
//! estimate-shaped dispersion.

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

/// The pinned standard-normal quantile `Z(q) = Φ⁻¹(q)` for the FIXED funding
/// quantile set `{0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95}` (design §2.6 A2b/A2d).
///
/// These are the SAME rounded multipliers the funding_forecast producer uses
/// (`crates/fortuna-runner/src/funding_forecast.rs`: Z25≈0.674, Z90≈1.282,
/// Z95≈1.645), so the random-walk fan is byte-identical-anchored to the producer's
/// own dispersion grid — a fair side-by-side, not an erf-inverse that could drift
/// across toolchains. Pinned constants (not computed) ⇒ replay determinism (I5).
///
/// Returns `None` for any `q` not in the table: the random-walk baseline is
/// defined ONLY over the standard funding quantile levels, and a forecast on
/// other levels is rejected (mapped to `InvalidPrediction`), never silently
/// scored against a mismatched grid. `0.1`/`0.9` are the same `f64` bit pattern
/// as `0.10`/`0.90`, so they match the same arms (no separate aliasing needed,
/// but called out so the equality is not mistaken for a gap).
fn standard_normal_z(q: f64) -> Option<f64> {
    // Exact f64 equality is intentional: the funding quantile set is a fixed,
    // pinned grid of these exact literals, not a continuous lookup. A `q` that is
    // not bit-identical to one of these is, by definition, off the grid.
    if q == 0.05 {
        Some(-1.645)
    } else if q == 0.10 {
        // 0.10 == 0.1 in f64, so a forecast emitting `0.1` matches here too.
        Some(-1.282)
    } else if q == 0.25 {
        Some(-0.674)
    } else if q == 0.50 {
        Some(0.0)
    } else if q == 0.75 {
        Some(0.674)
    } else if q == 0.90 {
        // 0.90 == 0.9 in f64.
        Some(1.282)
    } else if q == 0.95 {
        Some(1.645)
    } else {
        None
    }
}

/// Build the last-realized-rate baseline: a DEGENERATE scalar point mass at
/// `last_realized` over the SAME q-levels as `forecast` (mirrors carry-forward's
/// construction, just a different anchor). q strictly increasing (inherited); v
/// all equal ⇒ non-decreasing; finite iff `last_realized` is — so `validate`
/// (run inside `score`) surfaces a non-finite `last_realized` as
/// `InvalidPrediction`, never a silent NaN.
fn build_last_rate(
    quantiles: &[Quantile],
    unit: &str,
    last_realized: f64,
) -> PredictiveDistribution {
    PredictiveDistribution::Scalar {
        quantiles: quantiles
            .iter()
            .map(|q| Quantile {
                q: q.q,
                v: last_realized,
            })
            .collect(),
        unit: unit.to_string(),
    }
}

/// Build the random-walk baseline: a NON-degenerate fan anchored at `estimate`,
/// with `v(q) = estimate + Z(q)·band` over the forecast's own q-levels, `Z(q)`
/// from [`standard_normal_z`].
///
/// `rw_band` is the caller-injected horizon-scaled dispersion (σ·√(time-
/// remaining)); the kernel stays pure and does NOT invent a band constant. A
/// negative `rw_band` is clamped to 0 defensively (rather than erroring): a
/// non-finite-or-negative dispersion is a caller bug, but the SAFE collapse is a
/// point mass at `estimate` (band 0 ⇒ all v equal ⇒ still `validate`-clean), so
/// the comparison degrades to "RW == carry-forward" instead of failing the whole
/// edge gate on a dispersion-sign slip.
///
/// A NON-FINITE `rw_band` (NaN/±∞) is a different class of caller bug and is
/// REJECTED up front as `InvalidPrediction` — NOT clamped. (Clamping would hide
/// it: `f64::NAN.max(0.0)` returns `0.0`, which would silently collapse a NaN
/// dispersion into a clean point mass and report a misleading "RW ==
/// carry-forward". A NaN band must surface, not vanish.) Only a FINITE negative
/// band is clamped to 0.
///
/// Errors with `InvalidPrediction` if any forecast q-level is OFF the standard
/// funding grid (`standard_normal_z` returns `None`): the RW baseline requires
/// the standard funding quantile levels to place its fan. With `rw_band == 0` the
/// fan collapses to a point mass at `estimate` (identical to carry-forward).
/// Because `Z` is non-decreasing across the grid and `band ≥ 0`, `v` is
/// non-decreasing ⇒ `validate_scalar`-clean by construction (q increasing is
/// inherited from the forecast).
fn build_random_walk(
    quantiles: &[Quantile],
    unit: &str,
    estimate: f64,
    rw_band: f64,
) -> Result<PredictiveDistribution, ScoreError> {
    // Reject a non-finite band explicitly: NaN/∞ is a caller bug that must
    // surface, not be swallowed by `max(0.0)` (which returns 0.0 for NaN). Only a
    // finite negative band is clamped to 0 (the documented safe collapse).
    if !rw_band.is_finite() {
        return Err(ScoreError::InvalidPrediction {
            reason: format!("random-walk baseline rw_band={rw_band} is not finite"),
        });
    }
    let band = rw_band.max(0.0);
    let mut rw_quantiles = Vec::with_capacity(quantiles.len());
    for q in quantiles {
        let z = standard_normal_z(q.q).ok_or_else(|| ScoreError::InvalidPrediction {
            reason: format!(
                "random-walk baseline requires the standard funding quantile levels \
                 {{0.05,0.10,0.25,0.50,0.75,0.90,0.95}}; got q={} (off-grid)",
                q.q
            ),
        })?;
        rw_quantiles.push(Quantile {
            q: q.q,
            v: estimate + z * band,
        });
    }
    Ok(PredictiveDistribution::Scalar {
        quantiles: rw_quantiles,
        unit: unit.to_string(),
    })
}

/// The side-by-side CRPS of funding_forecast vs ALL FOUR naive baselines —
/// carry-forward, last-realized-rate, an estimate-anchored random-walk, and a
/// persistence-anchored random-walk — on one resolved window. CRPS is
/// LOWER-IS-BETTER, so each `beats_*` is the strict `forecast_crps <
/// {baseline}_crps`: a TIE does NOT beat. `beats_all` (the §2.6 A2d edge gate)
/// is the AND of all four — funding_forecast earns its edge only by strictly
/// improving on EVERY naive baseline at once.
///
/// The two random-walk legs are deliberately split: the ESTIMATE-anchored RW is
/// a NEAR-TWIN of funding_forecast (same anchor, same √-horizon dispersion
/// shape) and is therefore the WEAK leg; the PERSISTENCE-anchored RW (anchored
/// at `last_realized`) keeps a genuine naive-persistence + drift forecaster in
/// the comparison, so the gate stays ROBUST rather than rewarding the forecast
/// for merely re-deriving its own estimate-shaped dispersion.
#[derive(Debug, Clone, PartialEq)]
pub struct BaselineComparison {
    /// funding_forecast's CRPS against the realized rate (lower is better).
    pub forecast_crps: f64,
    /// The carry-forward baseline's CRPS (venue estimate projected flat).
    pub carry_forward_crps: f64,
    /// The last-realized-rate baseline's CRPS (point mass at `last_realized`).
    pub last_rate_crps: f64,
    /// The random-walk baseline's CRPS (estimate-anchored, `rw_band`-dispersed fan).
    pub random_walk_crps: f64,
    /// The persistence random-walk baseline's CRPS (`last_realized`-anchored,
    /// `rw_band`-dispersed fan): naive persistence + drift uncertainty.
    pub random_walk_persistence_crps: f64,
    /// `forecast_crps < carry_forward_crps`.
    pub beats_carry_forward: bool,
    /// `forecast_crps < last_rate_crps`.
    pub beats_last_rate: bool,
    /// `forecast_crps < random_walk_crps`.
    pub beats_random_walk: bool,
    /// `forecast_crps < random_walk_persistence_crps`.
    pub beats_random_walk_persistence: bool,
    /// `beats_carry_forward && beats_last_rate && beats_random_walk &&
    /// beats_random_walk_persistence` — strictly beats ALL FOUR. THE §2.6 A2d
    /// edge gate.
    pub beats_all: bool,
}

/// Compare funding_forecast's scalar `forecast` against all FOUR naive baselines
/// on the SAME `realized` funding rate, every leg scored by `crps_pinball`.
///
/// Anchors:
/// - `estimate` — the venue estimate of the CURRENT window: the carry-forward
///   point anchor AND the estimate-anchored random-walk fan anchor.
/// - `last_realized` — the PREVIOUS resolved window's realized rate: the
///   last-rate point anchor AND the persistence-anchored random-walk fan anchor.
/// - `rw_band` — the caller-computed horizon-scaled RW dispersion (σ·√horizon),
///   shared by BOTH random-walk legs. Injected, not invented (the kernel is
///   pure). Clamped to ≥0 in [`build_random_walk`].
///
/// All baselines reuse the forecast's OWN q-levels, so every CRPS is
/// apples-to-apples on the same discretization. Pure + deterministic: same
/// inputs always yield the same result, no clock/IO.
///
/// Errors (never a silent NaN):
/// - a non-Scalar `forecast` is a `KindMismatch` (funding_forecast is always
///   Scalar) — the FIRST check, mirroring [`compare_against_carry_forward`];
/// - a `forecast`/`estimate`/`last_realized`/`rw_band` that produces a fan
///   failing `validate()` (e.g. a non-finite anchor, or a NaN `rw_band`) is an
///   `InvalidPrediction`;
/// - a forecast on q-levels OUTSIDE the standard funding grid is an
///   `InvalidPrediction` (either random-walk leg cannot place its fan).
///
/// (Re the honest-twin note in the module docs: with the production
/// funding_forecast producer the ESTIMATE-anchored RW leg is the weakest
/// discriminator — a near-twin of the forecast's own dispersion. The robust gate
/// is carried by the PERSISTENCE-anchored RW (`last_realized` anchor), the
/// carry-forward bar, and the last-rate persistence point mass.)
pub fn compare_against_baselines(
    forecast: &PredictiveDistribution,
    estimate: f64,
    last_realized: f64,
    rw_band: f64,
    realized: f64,
) -> Result<BaselineComparison, ScoreError> {
    // Scalar-only by design: extract the q-levels up front so every baseline
    // shares them. A non-Scalar forecast is a kind mismatch (checked FIRST, like
    // the carry-forward entry point) — never a silent NaN comparison.
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

    // Carry-forward: estimate projected flat (a point mass at `estimate`).
    let carry_forward = build_last_rate(quantiles, unit, estimate);
    let carry_forward_crps = rule.score(&carry_forward, &outcome)?;

    // Last-realized-rate: a point mass at the previous window's realized rate.
    let last_rate = build_last_rate(quantiles, unit, last_realized);
    let last_rate_crps = rule.score(&last_rate, &outcome)?;

    // Random-walk (estimate-anchored): fan around the venue estimate, `rw_band`-
    // dispersed over the grid. The near-twin of funding_forecast (weak leg).
    let random_walk = build_random_walk(quantiles, unit, estimate, rw_band)?;
    let random_walk_crps = rule.score(&random_walk, &outcome)?;

    // Random-walk (persistence-anchored): the SAME fan construction reusing the
    // anchor-agnostic helper, but anchored at `last_realized` — a genuine
    // persistence-plus-diffusion forecaster, DISTINCT from the forecast. This is
    // the robust RW leg.
    let random_walk_persistence = build_random_walk(quantiles, unit, last_realized, rw_band)?;
    let random_walk_persistence_crps = rule.score(&random_walk_persistence, &outcome)?;

    let beats_carry_forward = forecast_crps < carry_forward_crps;
    let beats_last_rate = forecast_crps < last_rate_crps;
    let beats_random_walk = forecast_crps < random_walk_crps;
    let beats_random_walk_persistence = forecast_crps < random_walk_persistence_crps;

    Ok(BaselineComparison {
        forecast_crps,
        carry_forward_crps,
        last_rate_crps,
        random_walk_crps,
        random_walk_persistence_crps,
        beats_carry_forward,
        beats_last_rate,
        beats_random_walk,
        beats_random_walk_persistence,
        beats_all: beats_carry_forward
            && beats_last_rate
            && beats_random_walk
            && beats_random_walk_persistence,
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

    // ── SLICE 2: last-rate + random-walk baselines + the unified entry point ──

    /// The standard funding quantile levels (design §2.6 A2b/A2d) — the grid the
    /// random-walk baseline requires. `Z(q)` from `standard_normal_z`.
    const FUNDING_QS: [f64; 7] = [0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95];

    /// A funding-domain forecast fan on the FULL standard grid: `v = center +
    /// Z(q)·width` (so it is a valid, non-crossing fan for any `width ≥ 0`). Used
    /// where the random-walk leg needs on-grid q-levels.
    fn funding_scalar(center: f64, width: f64) -> PredictiveDistribution {
        let qvs: Vec<(f64, f64)> = FUNDING_QS
            .iter()
            .map(|&q| (q, center + standard_normal_z(q).unwrap() * width))
            .collect();
        scalar(&qvs)
    }

    #[test]
    fn standard_normal_z_matches_the_pinned_funding_grid_and_rejects_off_grid() {
        // The lookup mirrors the funding_forecast producer's rounded multipliers
        // EXACTLY, is monotone non-decreasing across the grid (so v stays
        // non-decreasing for band ≥ 0), and returns None off-grid.
        assert_eq!(standard_normal_z(0.05), Some(-1.645));
        assert_eq!(standard_normal_z(0.10), Some(-1.282));
        assert_eq!(standard_normal_z(0.25), Some(-0.674));
        assert_eq!(standard_normal_z(0.50), Some(0.0));
        assert_eq!(standard_normal_z(0.75), Some(0.674));
        assert_eq!(standard_normal_z(0.90), Some(1.282));
        assert_eq!(standard_normal_z(0.95), Some(1.645));
        // 0.1 / 0.9 are the same f64 as 0.10 / 0.90 — match the same arms.
        assert_eq!(standard_normal_z(0.1), standard_normal_z(0.10));
        assert_eq!(standard_normal_z(0.9), standard_normal_z(0.90));
        // Monotone non-decreasing (the v-non-decreasing guarantee for band ≥ 0).
        let zs: Vec<f64> = FUNDING_QS
            .iter()
            .map(|&q| standard_normal_z(q).unwrap())
            .collect();
        assert!(
            zs.windows(2).all(|w| w[0] <= w[1]),
            "Z must be non-decreasing: {zs:?}"
        );
        // Off-grid ⇒ None.
        assert_eq!(standard_normal_z(0.30), None);
        assert_eq!(standard_normal_z(0.50001), None);
        assert_eq!(standard_normal_z(0.0), None);
        assert_eq!(standard_normal_z(1.0), None);
    }

    #[test]
    fn a_forecast_centered_on_realized_beats_all_four_baselines() {
        // funding_forecast is a tight fan AT the realized rate; all four naive
        // baselines (estimate-anchored carry-forward, last-window persistence,
        // estimate-anchored RW, and the persistence-anchored RW) are anchored FAR
        // from realized. The forecast earns its edge against every one ⇒ beats_all.
        // THE §2.6 A2d pass case.
        let realized = 0.0010;
        let forecast = funding_scalar(realized, 0.0001); // tight, on-target
        let estimate = 0.0060; // far carry-forward AND far estimate-RW anchor
        let last_realized = 0.0050; // far last-rate AND far persistence-RW anchor
        let rw_band = 0.0005; // a real, non-degenerate RW fan (both legs)
        let c = compare_against_baselines(&forecast, estimate, last_realized, rw_band, realized)
            .unwrap();
        assert!(c.forecast_crps < c.carry_forward_crps, "{c:?}");
        assert!(c.forecast_crps < c.last_rate_crps, "{c:?}");
        assert!(c.forecast_crps < c.random_walk_crps, "{c:?}");
        assert!(c.forecast_crps < c.random_walk_persistence_crps, "{c:?}");
        assert!(
            c.beats_carry_forward
                && c.beats_last_rate
                && c.beats_random_walk
                && c.beats_random_walk_persistence,
            "{c:?}"
        );
        assert!(
            c.beats_all,
            "a centered forecast strictly beats all four: {c:?}"
        );
    }

    #[test]
    fn a_perfect_carry_forward_is_not_beaten_so_beats_all_is_false() {
        // estimate == realized ⇒ carry-forward is a perfect point mass (CRPS 0),
        // unbeatable by a strict `<`. A wild forecast cannot beat it ⇒
        // beats_carry_forward false ⇒ beats_all false, regardless of the other legs.
        let realized = 0.0010;
        let forecast = funding_scalar(0.0060, 0.0002); // wild, off-target
        let estimate = realized; // perfect carry-forward
        let last_realized = 0.0055; // far persistence (forecast may beat this)
        let rw_band = 0.0004;
        let c = compare_against_baselines(&forecast, estimate, last_realized, rw_band, realized)
            .unwrap();
        assert_eq!(
            c.carry_forward_crps, 0.0,
            "estimate==realized ⇒ CRPS 0: {c:?}"
        );
        assert!(
            !c.beats_carry_forward,
            "cannot strictly beat a perfect carry-forward: {c:?}"
        );
        assert!(
            !c.beats_all,
            "a failed carry-forward leg forces beats_all=false: {c:?}"
        );
    }

    #[test]
    fn a_perfect_last_rate_is_not_beaten_so_beats_last_rate_is_false() {
        // last_realized == realized ⇒ the last-rate point mass is perfect (CRPS 0),
        // so beats_last_rate is false (a tie/loss against 0, strict `<`), which
        // alone forces beats_all=false even if the forecast beats the other three.
        //
        // NOTE the persistence-RW is anchored at last_realized == realized too, but
        // with rw_band > 0 it is a DIFFUSE on-target fan, NOT a point mass — so the
        // tighter on-target forecast still strictly beats it. The last-rate point
        // mass stays the LONE blocker.
        let realized = 0.0010;
        let forecast = funding_scalar(realized, 0.0001); // on-target, tighter than the RW band
        let estimate = 0.0050; // far carry-forward + estimate-RW anchor (forecast beats these)
        let last_realized = realized; // perfect persistence POINT mass
        let rw_band = 0.0005; // persistence-RW fan is on-target but diffuse (beatable)
        let c = compare_against_baselines(&forecast, estimate, last_realized, rw_band, realized)
            .unwrap();
        assert_eq!(
            c.last_rate_crps, 0.0,
            "last_realized==realized ⇒ CRPS 0: {c:?}"
        );
        assert!(
            !c.beats_last_rate,
            "cannot strictly beat a perfect last-rate: {c:?}"
        );
        // It DID beat the other THREE — proving the last-rate leg is the lone blocker.
        assert!(c.beats_carry_forward, "{c:?}");
        assert!(c.beats_random_walk, "{c:?}");
        assert!(
            c.beats_random_walk_persistence,
            "tighter on-target forecast beats the diffuse persistence-RW: {c:?}"
        );
        assert!(
            !c.beats_all,
            "a perfect last-rate alone forces beats_all=false: {c:?}"
        );
    }

    #[test]
    fn random_walk_with_zero_band_collapses_to_the_carry_forward_point_mass() {
        // rw_band == 0 ⇒ v(q) = estimate + Z(q)·0 = estimate at every level ⇒ the
        // RW fan IS the carry-forward point mass. So random_walk_crps ==
        // carry_forward_crps exactly, and beats_random_walk == beats_carry_forward.
        let realized = 0.0010;
        let forecast = funding_scalar(0.0030, 0.0002);
        let estimate = 0.0040;
        let last_realized = 0.0020;
        let c =
            compare_against_baselines(&forecast, estimate, last_realized, 0.0, realized).unwrap();
        assert_eq!(
            c.random_walk_crps, c.carry_forward_crps,
            "rw_band=0 collapses RW to carry-forward: {c:?}"
        );
        assert_eq!(c.beats_random_walk, c.beats_carry_forward, "{c:?}");
    }

    #[test]
    fn random_walk_with_positive_band_is_a_nondegenerate_validate_clean_fan() {
        // rw_band > 0 ⇒ a real fan: strictly wider than a point mass, and it must
        // pass validate() by construction (q increasing inherited; v non-decreasing
        // because Z is non-decreasing and band ≥ 0). We rebuild it via the same
        // helper and assert it validates and is non-degenerate.
        let estimate = 0.0010;
        let band = 0.0005;
        let forecast = funding_scalar(estimate, 0.0001);
        let PredictiveDistribution::Scalar { quantiles, unit } = &forecast else {
            panic!("forecast is scalar");
        };
        let rw = build_random_walk(quantiles, unit, estimate, band).unwrap();
        rw.validate()
            .expect("RW fan with band>0 must be validate()-clean");
        let PredictiveDistribution::Scalar { quantiles: rq, .. } = &rw else {
            panic!("rw is scalar");
        };
        // Non-degenerate: the tails differ (0.05 strictly below 0.95).
        assert!(
            rq.first().unwrap().v < rq.last().unwrap().v,
            "RW fan must be non-degenerate: {rq:?}"
        );
        // Anchored & symmetric about the estimate at the median (Z(0.50)=0).
        let median = rq.iter().find(|q| q.q == 0.50).unwrap();
        assert_eq!(
            median.v, estimate,
            "RW median sits at the estimate anchor: {rq:?}"
        );
    }

    #[test]
    fn a_wide_random_walk_loses_to_a_tight_on_target_forecast() {
        // A tight forecast AT realized vs a WIDE estimate-anchored RW that is also
        // off-center: the forecast strictly beats the RW (beats_random_walk=true).
        let realized = 0.0010;
        let forecast = funding_scalar(realized, 0.0001); // tight, on-target
        let estimate = 0.0035; // RW anchor off realized
        let last_realized = 0.0010;
        let rw_band = 0.0020; // wide, diffuse RW
        let c = compare_against_baselines(&forecast, estimate, last_realized, rw_band, realized)
            .unwrap();
        assert!(c.forecast_crps < c.random_walk_crps, "{c:?}");
        assert!(
            c.beats_random_walk,
            "tight on-target forecast beats a wide RW: {c:?}"
        );
    }

    #[test]
    fn a_sharp_on_target_random_walk_is_not_beaten_by_a_diffuse_forecast() {
        // The RW anchor (estimate) IS realized and the band is tiny ⇒ a sharp,
        // on-target RW. A diffuse forecast off-target does NOT beat it
        // (beats_random_walk=false) — the RW leg can be the discriminating one.
        let realized = 0.0010;
        let forecast = funding_scalar(0.0030, 0.0010); // diffuse, off-target
        let estimate = realized; // RW anchored AT realized
        let last_realized = 0.0010;
        let rw_band = 0.00002; // very sharp RW
        let c = compare_against_baselines(&forecast, estimate, last_realized, rw_band, realized)
            .unwrap();
        assert!(c.random_walk_crps < c.forecast_crps, "{c:?}");
        assert!(
            !c.beats_random_walk,
            "a diffuse forecast does NOT beat a sharp on-target RW: {c:?}"
        );
    }

    #[test]
    fn beats_all_is_exactly_the_and_of_the_four_legs() {
        // Exhaustively assert the boolean identity on a case that beats all four.
        let realized = 0.0010;
        let forecast = funding_scalar(realized, 0.0001);
        let c = compare_against_baselines(&forecast, 0.0060, 0.0050, 0.0005, realized).unwrap();
        assert_eq!(
            c.beats_all,
            c.beats_carry_forward
                && c.beats_last_rate
                && c.beats_random_walk
                && c.beats_random_walk_persistence,
            "beats_all must be the AND of the four legs: {c:?}"
        );
        assert!(c.beats_all, "{c:?}");

        // And a case that fails exactly one leg (perfect last-rate point mass) still
        // satisfies the identity with beats_all=false. The persistence-RW shares the
        // last_realized anchor but, being a diffuse fan (band>0), is still beaten —
        // so the last-rate point mass is the SOLE failing leg.
        let c2 = compare_against_baselines(&forecast, 0.0060, realized, 0.0005, realized).unwrap();
        assert_eq!(
            c2.beats_all,
            c2.beats_carry_forward
                && c2.beats_last_rate
                && c2.beats_random_walk
                && c2.beats_random_walk_persistence,
            "{c2:?}"
        );
        assert!(!c2.beats_all, "{c2:?}");
        // Confirm last-rate is the lone failing leg in this case.
        assert!(
            c2.beats_carry_forward
                && !c2.beats_last_rate
                && c2.beats_random_walk
                && c2.beats_random_walk_persistence,
            "only the last-rate leg fails: {c2:?}"
        );
    }

    // ── persistence-anchored random-walk: the 4th, robust leg ──

    #[test]
    fn persistence_rw_is_distinct_from_estimate_rw_when_anchors_differ() {
        // The whole point of the 4th leg: when estimate != last_realized the
        // persistence-RW (anchored at last_realized) is a GENUINELY different fan
        // from the estimate-RW (anchored at estimate), so it scores a DIFFERENT
        // CRPS against the same realized. (A near-twin of the forecast it is not.)
        let realized = 0.0010;
        let forecast = funding_scalar(realized, 0.0001);
        let estimate = 0.0050; // estimate-RW centered here (far from realized)
        let last_realized = 0.0020; // persistence-RW centered here (nearer realized)
        let rw_band = 0.0005; // identical band for both legs ⇒ only the anchor differs
        let c = compare_against_baselines(&forecast, estimate, last_realized, rw_band, realized)
            .unwrap();
        assert_ne!(
            c.random_walk_crps, c.random_walk_persistence_crps,
            "different anchors ⇒ the two RW legs must score differently: {c:?}"
        );
        // The persistence-RW is centered nearer realized here, so it scores BETTER
        // (lower CRPS) than the far estimate-RW — confirming it is the distinct leg,
        // not a relabel of the estimate-RW.
        assert!(
            c.random_walk_persistence_crps < c.random_walk_crps,
            "persistence-RW centered nearer realized must beat the far estimate-RW: {c:?}"
        );
    }

    #[test]
    fn persistence_rw_can_be_the_lone_blocker_of_beats_all() {
        // The robustness payoff: a case where the forecast beats the OTHER THREE
        // baselines but is stopped by the persistence-RW alone ⇒ beats_all=false.
        //
        // Construction: last_realized is OFF realized, but the persistence-RW band
        // is WIDE enough that the fan's spread covers realized well (low CRPS) —
        // strictly better than both the off-target last-rate POINT mass at the same
        // anchor and a diffuse off-target forecast. The far estimate drives the
        // carry-forward and estimate-RW legs, which the forecast still beats.
        //
        // (Note: an EXACT `last_realized == realized` cannot isolate this leg — it
        // would make the last-rate point mass a perfect CRPS-0 co-blocker. So the
        // persistence-RW is made on-target via its FAN COVERAGE, not its anchor.)
        //
        // With these inputs (verified): forecast_crps≈0.0010126,
        // last_rate_crps=0.0015 (beaten by a clear margin), and
        // random_walk_persistence_crps≈0.0006788 (NOT beaten, the lone blocker);
        // carry-forward & estimate-RW sit at estimate=0.0120 (far, beaten).
        let realized = 0.0010;
        let forecast = funding_scalar(0.0040, 0.0012); // diffuse, off-target
        let estimate = 0.0120; // far carry-forward + far estimate-RW (forecast beats both)
        let last_realized = 0.0040; // last-rate point mass off realized (forecast beats it)
        let rw_band = 0.0030; // WIDE persistence-RW fan: spread covers realized
        let c = compare_against_baselines(&forecast, estimate, last_realized, rw_band, realized)
            .unwrap();
        // Persistence-RW is NOT beaten (the lone blocker) — strict, not a tie…
        assert!(
            c.random_walk_persistence_crps < c.forecast_crps,
            "persistence-RW must not be beaten here: {c:?}"
        );
        assert!(
            !c.beats_random_walk_persistence,
            "persistence-RW is the lone blocker ⇒ its beats flag is false: {c:?}"
        );
        // …while the forecast beats the OTHER three.
        assert!(
            c.beats_carry_forward,
            "forecast must beat far carry-forward: {c:?}"
        );
        assert!(
            c.beats_last_rate,
            "forecast must beat the off-target last-rate: {c:?}"
        );
        assert!(
            c.beats_random_walk,
            "forecast must beat the far estimate-RW: {c:?}"
        );
        // ⇒ the persistence-RW alone forces beats_all=false.
        assert!(
            !c.beats_all,
            "persistence-RW alone forces beats_all=false: {c:?}"
        );
    }

    #[test]
    fn beats_all_and_identity_holds_with_persistence_rw_as_the_failed_leg() {
        // Complements `beats_all_is_exactly_the_and_of_the_four_legs` (which fails on
        // the last-rate leg): here the SAME boolean identity is checked on a case
        // whose SOLE failing leg is the persistence-RW. Proves the new leg is wired
        // into the AND, not merely present on the struct.
        let realized = 0.0010;
        let forecast = funding_scalar(0.0040, 0.0012); // diffuse, off-target
                                                       // Same inputs as `persistence_rw_can_be_the_lone_blocker_of_beats_all`.
        let c = compare_against_baselines(&forecast, 0.0120, 0.0040, 0.0030, realized).unwrap();
        assert_eq!(
            c.beats_all,
            c.beats_carry_forward
                && c.beats_last_rate
                && c.beats_random_walk
                && c.beats_random_walk_persistence,
            "beats_all must be the AND of the four legs (persistence-fail case): {c:?}"
        );
        assert!(
            c.beats_carry_forward
                && c.beats_last_rate
                && c.beats_random_walk
                && !c.beats_random_walk_persistence,
            "only the persistence-RW leg fails: {c:?}"
        );
        assert!(!c.beats_all, "{c:?}");
    }

    #[test]
    fn unified_carry_forward_leg_agrees_with_the_slice_1_entry_point() {
        // The unified fn's carry-forward leg must produce the SAME numbers as the
        // standalone SLICE-1 `compare_against_carry_forward` (it reuses the same
        // construction). Lock that equivalence so the public APIs never diverge.
        let realized = 0.0012;
        let forecast = funding_scalar(0.0011, 0.0003);
        let estimate = 0.0040;
        let slice1 = compare_against_carry_forward(&forecast, estimate, realized).unwrap();
        let unified =
            compare_against_baselines(&forecast, estimate, 0.0020, 0.0005, realized).unwrap();
        assert_eq!(unified.forecast_crps, slice1.forecast_crps, "{unified:?}");
        assert_eq!(
            unified.carry_forward_crps, slice1.carry_forward_crps,
            "{unified:?}"
        );
        assert_eq!(
            unified.beats_carry_forward, slice1.beats_carry_forward,
            "{unified:?}"
        );
    }

    #[test]
    fn unified_all_crps_are_finite_and_nonnegative() {
        // Every leg is a proper-rule loss: finite and ≥ 0 (non-vacuity of the
        // unified comparison — all FIVE CRPS, including both RW legs, were actually
        // computed).
        let forecast = funding_scalar(0.0005, 0.0002);
        let c = compare_against_baselines(&forecast, 0.0008, 0.0003, 0.0004, 0.0006).unwrap();
        for v in [
            c.forecast_crps,
            c.carry_forward_crps,
            c.last_rate_crps,
            c.random_walk_crps,
            c.random_walk_persistence_crps,
        ] {
            assert!(
                v.is_finite() && v >= 0.0,
                "CRPS must be finite & non-negative: {c:?}"
            );
        }
    }

    #[test]
    fn unified_off_grid_quantiles_are_an_invalid_prediction_via_the_rw_leg() {
        // A forecast on q-levels OUTSIDE the standard funding grid cannot place the
        // RW fan ⇒ InvalidPrediction (NOT a silent mismatched-grid score). Here
        // 0.5 is on-grid but 0.25→0.3 and 0.75→0.7 are off-grid.
        let forecast = scalar(&[(0.3, 0.0009), (0.5, 0.0010), (0.7, 0.0011)]);
        let err = compare_against_baselines(&forecast, 0.0010, 0.0010, 0.0005, 0.0010).unwrap_err();
        assert!(
            matches!(err, ScoreError::InvalidPrediction { .. }),
            "off-grid q must be InvalidPrediction (RW leg): got {err:?}"
        );
    }

    #[test]
    fn unified_negative_rw_band_is_clamped_to_a_point_mass_not_an_error() {
        // A negative rw_band is a caller bug, but the documented SAFE behavior is to
        // clamp to 0 (RW collapses to the estimate point mass == carry-forward),
        // NOT to fail the whole edge gate. Confirm: no error, and RW == carry-forward.
        let realized = 0.0010;
        let forecast = funding_scalar(0.0030, 0.0002);
        let estimate = 0.0040;
        let c = compare_against_baselines(&forecast, estimate, 0.0020, -0.0005, realized).unwrap();
        assert_eq!(
            c.random_walk_crps, c.carry_forward_crps,
            "a negative rw_band clamps to the estimate point mass: {c:?}"
        );
    }

    #[test]
    fn unified_nan_rw_band_is_an_invalid_prediction() {
        // A NaN rw_band must NOT be swallowed by the clamp (`f64::NAN.max(0.0)`
        // returns 0.0, which would silently collapse it to a clean point mass and
        // report a misleading "RW == carry-forward"). `build_random_walk` rejects a
        // non-finite band UP FRONT ⇒ InvalidPrediction, never a silent point mass.
        let forecast = funding_scalar(0.0010, 0.0002);
        let err =
            compare_against_baselines(&forecast, 0.0010, 0.0010, f64::NAN, 0.0010).unwrap_err();
        assert!(
            matches!(err, ScoreError::InvalidPrediction { .. }),
            "a NaN rw_band must surface as InvalidPrediction: got {err:?}"
        );
    }

    #[test]
    fn unified_infinite_rw_band_is_an_invalid_prediction() {
        // ±∞ is the other non-finite band class — also rejected up front (an
        // infinite dispersion is a caller bug, not a usable fan width).
        let forecast = funding_scalar(0.0010, 0.0002);
        let err = compare_against_baselines(&forecast, 0.0010, 0.0010, f64::INFINITY, 0.0010)
            .unwrap_err();
        assert!(
            matches!(err, ScoreError::InvalidPrediction { .. }),
            "an infinite rw_band must surface as InvalidPrediction: got {err:?}"
        );
    }

    #[test]
    fn unified_non_finite_anchor_is_an_invalid_prediction() {
        // A non-finite estimate (carry-forward AND RW anchor) yields a non-finite
        // baseline value ⇒ InvalidPrediction from validate(), not a silent NaN.
        let forecast = funding_scalar(0.0010, 0.0002);
        let err = compare_against_baselines(&forecast, f64::INFINITY, 0.0010, 0.0005, 0.0010)
            .unwrap_err();
        assert!(
            matches!(err, ScoreError::InvalidPrediction { .. }),
            "a non-finite anchor must surface as InvalidPrediction: got {err:?}"
        );
    }

    #[test]
    fn unified_a_non_scalar_forecast_is_a_kind_mismatch() {
        // Mirror SLICE 1: the unified fn is scalar-only; a Categorical forecast is a
        // KindMismatch (checked FIRST), never a silent NaN comparison.
        let forecast = PredictiveDistribution::Categorical {
            bins: vec![
                crate::scoring::CategoricalBin {
                    label: "a".to_string(),
                    p: 0.5,
                },
                crate::scoring::CategoricalBin {
                    label: "b".to_string(),
                    p: 0.5,
                },
            ],
        };
        let err = compare_against_baselines(&forecast, 0.0010, 0.0010, 0.0005, 0.0010).unwrap_err();
        assert!(
            matches!(err, ScoreError::KindMismatch { .. }),
            "a non-scalar forecast is a KindMismatch: got {err:?}"
        );
    }
}
