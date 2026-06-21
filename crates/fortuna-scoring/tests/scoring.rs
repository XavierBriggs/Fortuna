//! Scoring module tests — written BEFORE implementation per the TDD doctrine.
//!
//! Spec text under test:
//! - `PredictiveDistribution` validation for all three shapes.
//! - `RealizedOutcome` kind parity.
//! - `BrierRule`: exact squared-error vectors; binary-only scope.
//! - `CrpsPinballRule`: TRUE-CRPS vectors (hand-computed); median
//!   degeneracy; realistic funding-rate vector; proper-scoring property.
//! - `ScoringRule` swappability: each rule on its shape, each rule errors on
//!   the wrong shape, kind-mismatched (pred, outcome) errors.
//! - Determinism: same inputs → identical score on repeated calls.

use fortuna_scoring::{
    BrierRule, CategoricalBin, CrpsPinballRule, LogScoreRule, PredictiveDistribution,
    PredictiveKind, Quantile, RealizedOutcome, RpsRule, ScoreError, ScoringRule,
};
use proptest::prelude::*;

// ─── helpers ────────────────────────────────────────────────────────────────

fn binary(p: f64) -> PredictiveDistribution {
    PredictiveDistribution::Binary { p }
}

fn scalar(qs: Vec<(f64, f64)>) -> PredictiveDistribution {
    PredictiveDistribution::Scalar {
        quantiles: qs.into_iter().map(|(q, v)| Quantile { q, v }).collect(),
        unit: "rate".to_string(),
    }
}

fn categorical(bins: Vec<(&str, f64)>) -> PredictiveDistribution {
    PredictiveDistribution::Categorical {
        bins: bins
            .into_iter()
            .map(|(l, p)| CategoricalBin {
                label: l.to_string(),
                p,
            })
            .collect(),
    }
}

/// Exact pinball loss at one quantile level.
fn pinball(q: f64, v: f64, y: f64) -> f64 {
    if y >= v {
        q * (y - v)
    } else {
        (1.0 - q) * (v - y)
    }
}

// ─── PredictiveDistribution::validate — Binary ──────────────────────────────

#[test]
fn binary_valid_midpoint() {
    assert!(binary(0.5).validate().is_ok());
}

#[test]
fn binary_valid_extremes_open_interval() {
    assert!(binary(0.001).validate().is_ok());
    assert!(binary(0.999).validate().is_ok());
}

#[test]
fn binary_p_zero_is_invalid() {
    assert!(binary(0.0).validate().is_err());
}

#[test]
fn binary_p_one_is_invalid() {
    assert!(binary(1.0).validate().is_err());
}

#[test]
fn binary_p_negative_is_invalid() {
    assert!(binary(-0.1).validate().is_err());
}

#[test]
fn binary_p_above_one_is_invalid() {
    assert!(binary(1.5).validate().is_err());
}

#[test]
fn binary_p_nan_is_invalid() {
    assert!(binary(f64::NAN).validate().is_err());
}

#[test]
fn binary_p_inf_is_invalid() {
    assert!(binary(f64::INFINITY).validate().is_err());
}

// ─── PredictiveDistribution::validate — Categorical ─────────────────────────

#[test]
fn categorical_valid_two_bins() {
    assert!(categorical(vec![("yes", 0.7), ("no", 0.3)])
        .validate()
        .is_ok());
}

#[test]
fn categorical_empty_bins_is_invalid() {
    let d = PredictiveDistribution::Categorical { bins: vec![] };
    assert!(d.validate().is_err());
}

#[test]
fn categorical_bins_not_summing_to_one_is_invalid() {
    // Sum = 0.6, outside tolerance.
    assert!(categorical(vec![("a", 0.3), ("b", 0.3)])
        .validate()
        .is_err());
}

#[test]
fn categorical_sum_just_outside_tolerance_is_invalid() {
    // Sum = 1.0 + 2e-9, outside 1e-9 tolerance.
    assert!(categorical(vec![("a", 0.5 + 1e-9), ("b", 0.5 + 1e-9)])
        .validate()
        .is_err());
}

#[test]
fn categorical_sum_within_tolerance_is_valid() {
    // Tiny float rounding: 0.1 + 0.2 + 0.7 may not sum to exactly 1.0
    // but must be within 1e-9.
    assert!(categorical(vec![("a", 0.1), ("b", 0.2), ("c", 0.7)])
        .validate()
        .is_ok());
}

#[test]
fn categorical_bin_p_below_zero_is_invalid() {
    assert!(categorical(vec![("a", -0.1), ("b", 1.1)])
        .validate()
        .is_err());
}

#[test]
fn categorical_bin_p_above_one_is_invalid() {
    assert!(categorical(vec![("a", 1.5), ("b", -0.5)])
        .validate()
        .is_err());
}

#[test]
fn categorical_bin_non_finite_p_is_invalid() {
    let d = PredictiveDistribution::Categorical {
        bins: vec![
            CategoricalBin {
                label: "a".to_string(),
                p: f64::NAN,
            },
            CategoricalBin {
                label: "b".to_string(),
                p: 1.0,
            },
        ],
    };
    assert!(d.validate().is_err());
}

#[test]
fn categorical_empty_label_is_invalid() {
    assert!(categorical(vec![("", 0.5), ("b", 0.5)]).validate().is_err());
}

#[test]
fn categorical_duplicate_labels_is_invalid() {
    assert!(categorical(vec![("a", 0.5), ("a", 0.5)])
        .validate()
        .is_err());
}

// ─── PredictiveDistribution::validate — Scalar ──────────────────────────────

#[test]
fn scalar_valid_three_quantiles() {
    assert!(scalar(vec![(0.1, -1.0), (0.5, 0.0), (0.9, 1.0)])
        .validate()
        .is_ok());
}

#[test]
fn scalar_fewer_than_two_quantiles_is_invalid() {
    assert!(scalar(vec![(0.5, 0.0)]).validate().is_err());
    assert!(scalar(vec![]).validate().is_err());
}

#[test]
fn scalar_q_at_zero_is_invalid() {
    assert!(scalar(vec![(0.0, 0.0), (0.5, 1.0)]).validate().is_err());
}

#[test]
fn scalar_q_at_one_is_invalid() {
    assert!(scalar(vec![(0.5, 0.0), (1.0, 1.0)]).validate().is_err());
}

#[test]
fn scalar_q_below_zero_is_invalid() {
    assert!(scalar(vec![(-0.1, 0.0), (0.5, 1.0)]).validate().is_err());
}

#[test]
fn scalar_q_above_one_is_invalid() {
    assert!(scalar(vec![(0.5, 0.0), (1.1, 1.0)]).validate().is_err());
}

#[test]
fn scalar_q_not_strictly_increasing_is_invalid() {
    // Equal q values violate "strictly increasing".
    assert!(scalar(vec![(0.3, 0.0), (0.3, 1.0)]).validate().is_err());
}

#[test]
fn scalar_q_decreasing_is_invalid() {
    assert!(scalar(vec![(0.7, 0.0), (0.3, 1.0)]).validate().is_err());
}

#[test]
fn scalar_v_crossing_is_invalid() {
    // v decreases: q=0.3 → v=1.0, q=0.7 → v=0.0 is quantile crossing.
    assert!(scalar(vec![(0.3, 1.0), (0.7, 0.0)]).validate().is_err());
}

#[test]
fn scalar_v_non_decreasing_equal_is_valid() {
    // Equal v at different q is valid (flat region in CDF).
    assert!(scalar(vec![(0.3, 0.0), (0.5, 0.0), (0.9, 1.0)])
        .validate()
        .is_ok());
}

#[test]
fn scalar_q_non_finite_is_invalid() {
    let d = PredictiveDistribution::Scalar {
        quantiles: vec![
            Quantile {
                q: f64::NAN,
                v: 0.0,
            },
            Quantile { q: 0.9, v: 1.0 },
        ],
        unit: "rate".to_string(),
    };
    assert!(d.validate().is_err());
}

#[test]
fn scalar_v_non_finite_is_invalid() {
    let d = PredictiveDistribution::Scalar {
        quantiles: vec![
            Quantile {
                q: 0.1,
                v: f64::INFINITY,
            },
            Quantile { q: 0.9, v: 1.0 },
        ],
        unit: "rate".to_string(),
    };
    assert!(d.validate().is_err());
}

#[test]
fn scalar_empty_unit_is_invalid() {
    let d = PredictiveDistribution::Scalar {
        quantiles: vec![Quantile { q: 0.1, v: -1.0 }, Quantile { q: 0.9, v: 1.0 }],
        unit: "".to_string(),
    };
    assert!(d.validate().is_err());
}

// ─── PredictiveKind ──────────────────────────────────────────────────────────

#[test]
fn kind_parity_between_distribution_and_outcome() {
    assert_eq!(binary(0.5).kind(), PredictiveKind::Binary);
    assert_eq!(
        categorical(vec![("a", 0.6), ("b", 0.4)]).kind(),
        PredictiveKind::Categorical
    );
    assert_eq!(
        scalar(vec![(0.1, -1.0), (0.9, 1.0)]).kind(),
        PredictiveKind::Scalar
    );

    assert_eq!(
        RealizedOutcome::Binary { happened: true }.kind(),
        PredictiveKind::Binary
    );
    assert_eq!(
        RealizedOutcome::Categorical {
            label: "a".to_string()
        }
        .kind(),
        PredictiveKind::Categorical
    );
    assert_eq!(
        RealizedOutcome::Scalar { value: 0.0 }.kind(),
        PredictiveKind::Scalar
    );
}

// ─── BrierRule: exact vectors ────────────────────────────────────────────────

#[test]
fn brier_07_true_is_009() {
    let score = BrierRule.score(&binary(0.7), &RealizedOutcome::Binary { happened: true });
    assert!(score.is_ok());
    let s = score.unwrap();
    assert!((s - 0.09).abs() < 1e-12, "got {s}");
}

#[test]
fn brier_07_false_is_049() {
    let score = BrierRule.score(&binary(0.7), &RealizedOutcome::Binary { happened: false });
    assert!(score.is_ok());
    let s = score.unwrap();
    assert!((s - 0.49).abs() < 1e-12, "got {s}");
}

#[test]
fn brier_fair_coin_is_025() {
    let score_t = BrierRule
        .score(&binary(0.5), &RealizedOutcome::Binary { happened: true })
        .unwrap();
    let score_f = BrierRule
        .score(&binary(0.5), &RealizedOutcome::Binary { happened: false })
        .unwrap();
    assert!((score_t - 0.25).abs() < 1e-12, "true got {score_t}");
    assert!((score_f - 0.25).abs() < 1e-12, "false got {score_f}");
}

#[test]
fn brier_id_is_brier() {
    assert_eq!(BrierRule.id(), "brier");
}

#[test]
fn brier_applies_to_binary_only() {
    assert!(BrierRule.applies_to(PredictiveKind::Binary));
    assert!(!BrierRule.applies_to(PredictiveKind::Categorical));
    assert!(!BrierRule.applies_to(PredictiveKind::Scalar));
}

#[test]
fn brier_on_scalar_pred_errors() {
    let pred = scalar(vec![(0.1, -1.0), (0.9, 1.0)]);
    let out = RealizedOutcome::Scalar { value: 0.0 };
    let r = BrierRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::UnsupportedKind { .. })),
        "expected UnsupportedKind, got {r:?}"
    );
}

#[test]
fn brier_on_categorical_pred_errors() {
    let pred = categorical(vec![("a", 0.6), ("b", 0.4)]);
    let out = RealizedOutcome::Categorical {
        label: "a".to_string(),
    };
    let r = BrierRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::UnsupportedKind { .. })),
        "expected UnsupportedKind, got {r:?}"
    );
}

// ─── CrpsPinballRule: exact vectors (TRUE CRPS = 2·Σ pinball·Δτ) ──────────────
//
// The rule returns the TRUE CRPS, not the mean pinball: for an equally-spaced
// quantile grid of K levels the convention is Δτ = 1/K, so
//   CRPS = 2·Σ_k pinball_{q_k}(y, v_k)·Δτ = (2/K)·Σ_k pinball_{q_k}(y, v_k).
// (Mean pinball, Σ/K, is exactly ½·CRPS — missing the factor 2.)
//
// Grid: [(0.1, -1.0), (0.5, 0.0), (0.9, 1.0)]  (K = 3)
//
// y = 0:
//   pinball_0.1(-1.0, 0) = 0.1*(0-(-1)) = 0.1
//   pinball_0.5(0.0, 0)  = 0.5*(0-0)   = 0.0
//   pinball_0.9(1.0, 0)  = (1-0.9)*(1-0) = 0.1
//   CRPS = (2/3)*(0.1 + 0.0 + 0.1) ≈ 0.1333...
//
// y = 2:
//   pinball_0.1(-1.0, 2) = 0.1*(2-(-1)) = 0.3
//   pinball_0.5(0.0, 2)  = 0.5*(2-0)   = 1.0
//   pinball_0.9(1.0, 2)  = 0.9*(2-1)   = 0.9
//   CRPS = (2/3)*(0.3 + 1.0 + 0.9) ≈ 1.4666...

const GRID: &[(f64, f64)] = &[(0.1, -1.0), (0.5, 0.0), (0.9, 1.0)];

/// TRUE CRPS over an equally-spaced K-level grid: `(2/K)·Σ pinball`.
fn expected_crps(qs: &[(f64, f64)], y: f64) -> f64 {
    let sum: f64 = qs.iter().map(|(q, v)| pinball(*q, *v, y)).sum();
    2.0 * sum / qs.len() as f64
}

#[test]
fn crps_grid_y_zero() {
    let pred = scalar(GRID.to_vec());
    let out = RealizedOutcome::Scalar { value: 0.0 };
    let s = CrpsPinballRule.score(&pred, &out).unwrap();
    let expected = expected_crps(GRID, 0.0);
    assert!((s - expected).abs() < 1e-12, "got {s}, expected {expected}");
}

#[test]
fn crps_grid_y_two() {
    let pred = scalar(GRID.to_vec());
    let out = RealizedOutcome::Scalar { value: 2.0 };
    let s = CrpsPinballRule.score(&pred, &out).unwrap();
    let expected = expected_crps(GRID, 2.0);
    assert!((s - expected).abs() < 1e-12, "got {s}, expected {expected}");
}

#[test]
fn crps_single_quantile_fails_validation_as_required() {
    // A lone q=0.5 quantile is the mathematical |y − v| median case, but the
    // ≥2-quantile invariant makes it UNREACHABLE: score() calls validate()
    // first and MUST reject it. This pins that contract — the |y − v| identity
    // documented on the rule is never a scored input.
    for (v, y) in [(0.0f64, 1.0f64), (0.5, 0.0), (3.0, -1.0)] {
        let pred = PredictiveDistribution::Scalar {
            quantiles: vec![Quantile { q: 0.5, v }],
            unit: "rate".to_string(),
        };
        let out = RealizedOutcome::Scalar { value: y };
        let r = CrpsPinballRule.score(&pred, &out);
        assert!(
            matches!(r, Err(ScoreError::InvalidPrediction { .. })),
            "single quantile should fail validation, got {r:?}"
        );
    }
}

#[test]
fn pinball_at_realized_equals_v_is_zero() {
    // The kink point: at y == v the check loss is exactly 0 for every q (both
    // branches vanish), so the TRUE CRPS (2/K · Σ 0) is also 0. Pins the
    // boundary value as a regression guard and covers the flat-CDF (constant v)
    // case.
    let pred = scalar(vec![(0.1, 0.5), (0.9, 0.5)]);
    let out = RealizedOutcome::Scalar { value: 0.5 };
    let s = CrpsPinballRule.score(&pred, &out).unwrap();
    assert!(s.abs() < 1e-15, "got {s}");
}

/// A realistic recorded Kalshi BTC funding rate: approx 5 hours of quantile
/// forecast vs the final rate. Quantiles were chosen to bracket 0.0001 tightly.
#[test]
fn crps_realistic_funding_rate_vector() {
    // Forecast: quantiles at {0.1, 0.25, 0.5, 0.75, 0.9} basis points.
    let pred = scalar(vec![
        (0.10, -0.0003),
        (0.25, -0.0001),
        (0.50, 0.0001),
        (0.75, 0.0003),
        (0.90, 0.0005),
    ]);
    // Realized rate slightly above the median:
    let y = 0.0002_f64;
    let out = RealizedOutcome::Scalar { value: y };

    let s = CrpsPinballRule.score(&pred, &out).unwrap();
    let expected = expected_crps(
        &[
            (0.10, -0.0003),
            (0.25, -0.0001),
            (0.50, 0.0001),
            (0.75, 0.0003),
            (0.90, 0.0005),
        ],
        y,
    );
    assert!((s - expected).abs() < 1e-14, "got {s}, expected {expected}");
    // Score must be non-negative.
    assert!(s >= 0.0);
}

#[test]
fn crps_id_is_crps_pinball() {
    assert_eq!(CrpsPinballRule.id(), "crps_pinball");
}

#[test]
fn crps_applies_to_scalar_only() {
    assert!(CrpsPinballRule.applies_to(PredictiveKind::Scalar));
    assert!(!CrpsPinballRule.applies_to(PredictiveKind::Binary));
    assert!(!CrpsPinballRule.applies_to(PredictiveKind::Categorical));
}

#[test]
fn crps_on_binary_pred_errors() {
    let pred = binary(0.7);
    let out = RealizedOutcome::Binary { happened: true };
    let r = CrpsPinballRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::UnsupportedKind { .. })),
        "expected UnsupportedKind, got {r:?}"
    );
}

// ─── Kind mismatch between pred and outcome ──────────────────────────────────

#[test]
fn brier_kind_mismatch_pred_binary_outcome_scalar_errors() {
    let pred = binary(0.5);
    let out = RealizedOutcome::Scalar { value: 1.0 };
    let r = BrierRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::KindMismatch { .. })),
        "expected KindMismatch, got {r:?}"
    );
}

#[test]
fn crps_kind_mismatch_pred_scalar_outcome_binary_errors() {
    let pred = scalar(vec![(0.1, -1.0), (0.9, 1.0)]);
    let out = RealizedOutcome::Binary { happened: false };
    let r = CrpsPinballRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::KindMismatch { .. })),
        "expected KindMismatch, got {r:?}"
    );
}

// ─── Invalid prediction is caught by score() ────────────────────────────────

#[test]
fn brier_invalid_pred_errors_with_invalid_prediction() {
    let pred = binary(1.5); // p outside [0,1]
    let out = RealizedOutcome::Binary { happened: true };
    let r = BrierRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::InvalidPrediction { .. })),
        "expected InvalidPrediction, got {r:?}"
    );
}

#[test]
fn crps_invalid_pred_quantile_crossing_errors() {
    let pred = scalar(vec![(0.3, 1.0), (0.7, 0.0)]); // crossing v
    let out = RealizedOutcome::Scalar { value: 0.5 };
    let r = CrpsPinballRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::InvalidPrediction { .. })),
        "expected InvalidPrediction, got {r:?}"
    );
}

// ─── Determinism ─────────────────────────────────────────────────────────────

#[test]
fn brier_deterministic_on_repeated_calls() {
    let pred = binary(0.7);
    let out = RealizedOutcome::Binary { happened: true };
    let s1 = BrierRule.score(&pred, &out).unwrap();
    let s2 = BrierRule.score(&pred, &out).unwrap();
    // Identical IEEE bits on repeated evaluation (pure function, no randomness).
    assert_eq!(s1.to_bits(), s2.to_bits());
}

#[test]
fn crps_deterministic_on_repeated_calls() {
    let pred = scalar(GRID.to_vec());
    let out = RealizedOutcome::Scalar { value: 0.5 };
    let s1 = CrpsPinballRule.score(&pred, &out).unwrap();
    let s2 = CrpsPinballRule.score(&pred, &out).unwrap();
    assert_eq!(s1.to_bits(), s2.to_bits());
}

// ─── Proper-scoring property (proptest) ──────────────────────────────────────
//
// A properly calibrated forecast scores at least as well as a shifted one.
// Concretely: for scalar [q1,q2,q3] whose MEDIAN (q=0.5) equals the
// realized value, the score is no worse than a forecast shifted ±δ.
// We test the median-is-optimal property: when v_median = y, no shift
// can improve the score. (The constant factor 2 in TRUE CRPS preserves the
// ordering, so this property is unchanged by the scale fix.)

proptest! {
    #[test]
    fn proper_scoring_median_optimal(
        // Central value in a narrow range
        center in -1.0_f64..1.0_f64,
        // Shift away from truth (strictly positive for the test to be meaningful)
        delta in 0.001_f64..0.5_f64,
    ) {
        // Forecast centered at `center` (median = center).
        let spread = 0.2_f64;
        let pred_true = PredictiveDistribution::Scalar {
            quantiles: vec![
                Quantile { q: 0.1, v: center - spread },
                Quantile { q: 0.5, v: center },
                Quantile { q: 0.9, v: center + spread },
            ],
            unit: "rate".to_string(),
        };
        // Shifted forecast: median is center + delta.
        let pred_shifted = PredictiveDistribution::Scalar {
            quantiles: vec![
                Quantile { q: 0.1, v: center + delta - spread },
                Quantile { q: 0.5, v: center + delta },
                Quantile { q: 0.9, v: center + delta + spread },
            ],
            unit: "rate".to_string(),
        };

        let y = center; // realized value = true median
        let out = RealizedOutcome::Scalar { value: y };

        let s_true = CrpsPinballRule.score(&pred_true, &out).unwrap();
        let s_shifted = CrpsPinballRule.score(&pred_shifted, &out).unwrap();

        // The truth-centered forecast must score at most as high as the shifted one.
        prop_assert!(
            s_true <= s_shifted + 1e-12,
            "s_true={s_true} should be ≤ s_shifted={s_shifted} (delta={delta})"
        );
    }
}

// ─── LogScoreRule (S2): logarithmic / ignorance score for binary events ───────
//
// Log score S(p, o) = −ln(p) if the event happened, −ln(1−p) otherwise.
// Lower is better. p is clamped to [1e-15, 1−1e-15] so a near-certain miss is
// finite rather than +∞. Guard ORDER mirrors BrierRule: validate → applies_to
// (UnsupportedKind) → kind parity (KindMismatch).

#[test]
fn log_score_at_half_is_ln2() {
    // p = 0.5, event happened ⇒ −ln(0.5) = ln(2).
    let s = LogScoreRule
        .score(&binary(0.5), &RealizedOutcome::Binary { happened: true })
        .unwrap();
    assert!(
        (s - std::f64::consts::LN_2).abs() < 1e-12,
        "got {s}, expected ln(2)={}",
        std::f64::consts::LN_2
    );
}

#[test]
fn log_score_at_half_miss_is_ln2() {
    // Symmetry at p=0.5: a miss also scores ln(2) since −ln(1−0.5) = ln(2).
    let s = LogScoreRule
        .score(&binary(0.5), &RealizedOutcome::Binary { happened: false })
        .unwrap();
    assert!((s - std::f64::consts::LN_2).abs() < 1e-12, "got {s}");
}

#[test]
fn log_floor_keeps_finite() {
    // p ≈ 1, event did NOT happen ⇒ −ln(1−p) would diverge; the 1e-15 clamp
    // keeps it finite and large (> 30, i.e. about −ln(1e-15) ≈ 34.5).
    let s = LogScoreRule
        .score(
            &binary(0.999999999999999),
            &RealizedOutcome::Binary { happened: false },
        )
        .unwrap();
    assert!(s.is_finite(), "score must be finite, got {s}");
    assert!(s > 30.0, "clamped tail score should exceed 30, got {s}");
}

#[test]
fn log_perfect_forecast_scores_near_zero() {
    // A confident, correct forecast (p≈1, happened) scores ≈ 0 (−ln(1) = 0),
    // but is floored just above 0 by the clamp (−ln(1−1e-15) ≈ 1e-15).
    let s = LogScoreRule
        .score(
            &binary(0.999999999999999),
            &RealizedOutcome::Binary { happened: true },
        )
        .unwrap();
    assert!((0.0..1e-6).contains(&s), "near-perfect forecast got {s}");
}

#[test]
fn log_rejects_scalar() {
    // Scalar pred + Scalar outcome: the rule is Binary-only ⇒ UnsupportedKind
    // (raised before kind parity, matching BrierRule's guard order).
    let pred = scalar(vec![(0.1, -1.0), (0.9, 1.0)]);
    let out = RealizedOutcome::Scalar { value: 0.0 };
    let r = LogScoreRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::UnsupportedKind { .. })),
        "expected UnsupportedKind, got {r:?}"
    );
}

#[test]
fn log_rejects_categorical() {
    let pred = categorical(vec![("a", 0.6), ("b", 0.4)]);
    let out = RealizedOutcome::Categorical {
        label: "a".to_string(),
    };
    let r = LogScoreRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::UnsupportedKind { .. })),
        "expected UnsupportedKind, got {r:?}"
    );
}

#[test]
fn log_kind_mismatch_pred_binary_outcome_scalar_errors() {
    // Binary pred (applies_to passes) paired with a Scalar outcome ⇒ the kind
    // parity guard fires AFTER applies_to: KindMismatch.
    let pred = binary(0.5);
    let out = RealizedOutcome::Scalar { value: 1.0 };
    let r = LogScoreRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::KindMismatch { .. })),
        "expected KindMismatch, got {r:?}"
    );
}

#[test]
fn log_invalid_pred_errors_with_invalid_prediction() {
    // validate() runs first: p outside [0,1] is rejected before any guard.
    let pred = binary(1.5);
    let out = RealizedOutcome::Binary { happened: true };
    let r = LogScoreRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::InvalidPrediction { .. })),
        "expected InvalidPrediction, got {r:?}"
    );
}

#[test]
fn log_id_is_log() {
    assert_eq!(LogScoreRule.id(), "log");
}

#[test]
fn log_applies_to_binary_only() {
    assert!(LogScoreRule.applies_to(PredictiveKind::Binary));
    assert!(!LogScoreRule.applies_to(PredictiveKind::Categorical));
    assert!(!LogScoreRule.applies_to(PredictiveKind::Scalar));
}

#[test]
fn log_monotone_worse_scores_higher() {
    // Event happened: a forecast that assigned LESS probability to the truth
    // (p=0.4) must score strictly WORSE (higher) than one that assigned more
    // (p=0.8). Lower-is-better ordering for the log score.
    let better = LogScoreRule
        .score(&binary(0.8), &RealizedOutcome::Binary { happened: true })
        .unwrap();
    let worse = LogScoreRule
        .score(&binary(0.4), &RealizedOutcome::Binary { happened: true })
        .unwrap();
    assert!(
        worse > better,
        "worse (p=0.4) {worse} should exceed better (p=0.8) {better}"
    );
}

#[test]
fn log_deterministic_on_repeated_calls() {
    let pred = binary(0.7);
    let out = RealizedOutcome::Binary { happened: true };
    let s1 = LogScoreRule.score(&pred, &out).unwrap();
    let s2 = LogScoreRule.score(&pred, &out).unwrap();
    assert_eq!(s1.to_bits(), s2.to_bits());
}

// ─── RpsRule (S2): Ranked Probability Score for ordered categorical ───────────
//
// RPS over an ordered K-bin ladder is the sum of squared distances between the
// predicted cumulative distribution and the observed (step) cumulative
// distribution, over the first K−1 cut points:
//
//   RPS = Σ_{i=1}^{K-1} (P_i − O_i)²
//
// where P_i = Σ_{j≤i} p_j and O_i steps 0→1 at the realized label's index.
// The Vec order of `bins` IS the ladder order. Lower is better.

/// Build an RPS pred from ordered (label, mass) pairs and a Categorical outcome.
fn cat_outcome(label: &str) -> RealizedOutcome {
    RealizedOutcome::Categorical {
        label: label.to_string(),
    }
}

#[test]
fn rps_known_value_three_bins() {
    // Ladder masses [lo:0.2, mid:0.5, hi:0.3]; realized = "mid" (index 1).
    // Predicted CDF: [0.2, 0.7, 1.0]; observed CDF: [0, 1, 1].
    // RPS = Σ_{i=1..2} (P_i − O_i)²
    //     = (0.2 − 0)² + (0.7 − 1)²
    //     = 0.04 + 0.09 = 0.13.
    let pred = categorical(vec![("lo", 0.2), ("mid", 0.5), ("hi", 0.3)]);
    let s = RpsRule.score(&pred, &cat_outcome("mid")).unwrap();
    assert!((s - 0.13).abs() < 1e-12, "got {s}, expected 0.13");
}

#[test]
fn rps_perfect_forecast_is_zero() {
    // All mass on the realized label ⇒ predicted CDF equals the observed step
    // CDF exactly ⇒ RPS = 0.
    let pred = categorical(vec![("lo", 0.0), ("mid", 1.0), ("hi", 0.0)]);
    let s = RpsRule.score(&pred, &cat_outcome("mid")).unwrap();
    assert!(s.abs() < 1e-15, "perfect forecast should score 0, got {s}");
}

#[test]
fn rps_realized_at_first_label() {
    // Realized = "lo" (index 0). Observed CDF: [1, 1].
    // Predicted CDF: [0.2, 0.7]. RPS = (0.2−1)² + (0.7−1)² = 0.64 + 0.09 = 0.73.
    let pred = categorical(vec![("lo", 0.2), ("mid", 0.5), ("hi", 0.3)]);
    let s = RpsRule.score(&pred, &cat_outcome("lo")).unwrap();
    assert!((s - 0.73).abs() < 1e-12, "got {s}, expected 0.73");
}

#[test]
fn rps_realized_at_last_label() {
    // Realized = "hi" (index 2, the last cut). Observed CDF over cuts 1..2:
    // [0, 0]. Predicted CDF: [0.2, 0.7]. RPS = 0.04 + 0.49 = 0.53.
    let pred = categorical(vec![("lo", 0.2), ("mid", 0.5), ("hi", 0.3)]);
    let s = RpsRule.score(&pred, &cat_outcome("hi")).unwrap();
    assert!((s - 0.53).abs() < 1e-12, "got {s}, expected 0.53");
}

#[test]
fn rps_rejects_binary() {
    // Binary pred + Binary outcome: the rule is Categorical-only ⇒
    // UnsupportedKind (raised before kind parity).
    let pred = binary(0.7);
    let out = RealizedOutcome::Binary { happened: true };
    let r = RpsRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::UnsupportedKind { .. })),
        "expected UnsupportedKind, got {r:?}"
    );
}

#[test]
fn rps_rejects_scalar() {
    let pred = scalar(vec![(0.1, -1.0), (0.9, 1.0)]);
    let out = RealizedOutcome::Scalar { value: 0.0 };
    let r = RpsRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::UnsupportedKind { .. })),
        "expected UnsupportedKind, got {r:?}"
    );
}

#[test]
fn rps_unknown_label_invalid() {
    // The realized label is absent from the bins ⇒ no step can be placed on the
    // ladder ⇒ InvalidPrediction (never silently treated as "never happened").
    let pred = categorical(vec![("lo", 0.2), ("mid", 0.5), ("hi", 0.3)]);
    let r = RpsRule.score(&pred, &cat_outcome("nope"));
    assert!(
        matches!(r, Err(ScoreError::InvalidPrediction { .. })),
        "expected InvalidPrediction, got {r:?}"
    );
}

#[test]
fn rps_kind_mismatch_pred_categorical_outcome_scalar_errors() {
    // Categorical pred (applies_to passes) paired with a Scalar outcome ⇒ the
    // kind parity guard fires AFTER applies_to: KindMismatch.
    let pred = categorical(vec![("a", 0.6), ("b", 0.4)]);
    let out = RealizedOutcome::Scalar { value: 1.0 };
    let r = RpsRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::KindMismatch { .. })),
        "expected KindMismatch, got {r:?}"
    );
}

#[test]
fn rps_invalid_pred_errors_with_invalid_prediction() {
    // validate() runs first: masses that don't sum to 1 are rejected.
    let pred = categorical(vec![("a", 0.3), ("b", 0.3)]);
    let out = cat_outcome("a");
    let r = RpsRule.score(&pred, &out);
    assert!(
        matches!(r, Err(ScoreError::InvalidPrediction { .. })),
        "expected InvalidPrediction, got {r:?}"
    );
}

#[test]
fn rps_id_is_rps() {
    assert_eq!(RpsRule.id(), "rps");
}

#[test]
fn rps_applies_to_categorical_only() {
    assert!(RpsRule.applies_to(PredictiveKind::Categorical));
    assert!(!RpsRule.applies_to(PredictiveKind::Binary));
    assert!(!RpsRule.applies_to(PredictiveKind::Scalar));
}

#[test]
fn rps_monotone_distance_aware_beats_distant() {
    // RPS rewards being CLOSE on an ordered ladder, not just right. Realized =
    // "hi" (index 2). A forecast that puts its errant mass on the ADJACENT bin
    // "mid" must score better (lower) than one that puts it on the FAR bin "lo".
    let near = categorical(vec![("lo", 0.0), ("mid", 0.3), ("hi", 0.7)]);
    let far = categorical(vec![("lo", 0.3), ("mid", 0.0), ("hi", 0.7)]);
    let s_near = RpsRule.score(&near, &cat_outcome("hi")).unwrap();
    let s_far = RpsRule.score(&far, &cat_outcome("hi")).unwrap();
    assert!(
        s_near < s_far,
        "near-miss {s_near} should beat far-miss {s_far}"
    );
}

#[test]
fn rps_deterministic_on_repeated_calls() {
    let pred = categorical(vec![("lo", 0.2), ("mid", 0.5), ("hi", 0.3)]);
    let out = cat_outcome("mid");
    let s1 = RpsRule.score(&pred, &out).unwrap();
    let s2 = RpsRule.score(&pred, &out).unwrap();
    assert_eq!(s1.to_bits(), s2.to_bits());
}
