//! PAV + CORP tests — written BEFORE implementation per the TDD doctrine.
//!
//! Under test (research §3.1, plan Task 4/S3):
//! - `pav`: weighted Pool-Adjacent-Violators isotonic (nondecreasing) fit.
//! - `corp`: CORP decomposition of the Brier score `S̄ = MCB − DSC + UNC`
//!   (all three ≥ 0), the recalibrated reliability curve, and a deterministic
//!   closed-form consistency band per curve point.
//!
//! Assertions are black-box: they pin the decomposition IDENTITY and the
//! nonnegativity/calibration/determinism CONTRACT, never the internal pooling.

use fortuna_scoring::{corp, pav, CalibrationSample};

// ─── helpers ────────────────────────────────────────────────────────────────

fn samples(pairs: &[(f64, bool)]) -> Vec<CalibrationSample> {
    pairs
        .iter()
        .map(|(p, o)| CalibrationSample { p: *p, outcome: *o })
        .collect()
}

/// Independent reference mean-Brier, computed in the test (not via the lib).
fn mean_brier(s: &[CalibrationSample]) -> f64 {
    let n = s.len() as f64;
    s.iter()
        .map(|x| {
            let o = if x.outcome { 1.0 } else { 0.0 };
            (x.p - o) * (x.p - o)
        })
        .sum::<f64>()
        / n
}

fn is_nondecreasing(v: &[f64]) -> bool {
    v.windows(2).all(|w| w[1] >= w[0] - 1e-12)
}

// ─── PAV ─────────────────────────────────────────────────────────────────────

#[test]
fn pav_is_monotone() {
    // Adjacent violators present → output must be nondecreasing.
    let values = [3.0, 1.0, 2.0, 0.0, 4.0];
    let weights = [1.0, 1.0, 1.0, 1.0, 1.0];
    let fit = pav(&values, &weights);
    assert_eq!(fit.len(), values.len());
    assert!(
        is_nondecreasing(&fit),
        "pav output must be nondecreasing: {fit:?}"
    );
}

#[test]
fn pav_already_sorted_is_identity() {
    // Already nondecreasing input → the fit equals the input.
    let values = [0.0, 0.0, 0.25, 0.5, 1.0, 1.0];
    let weights = [1.0; 6];
    let fit = pav(&values, &weights);
    for (a, b) in fit.iter().zip(values.iter()) {
        assert!(
            (a - b).abs() < 1e-12,
            "expected identity on sorted input: {fit:?}"
        );
    }
}

#[test]
fn pav_empty_is_empty() {
    assert!(pav(&[], &[]).is_empty());
}

#[test]
fn pav_mismatched_lengths_no_panic() {
    // Mismatched lengths must NOT panic (contract: return values clone or empty).
    let out = pav(&[1.0, 2.0, 3.0], &[1.0]);
    assert!(out.is_empty() || out.len() == 3);
}

#[test]
fn pav_pools_two_decreasing_to_their_weighted_mean() {
    // [1.0, 0.0] with equal weights pools to the mean 0.5 at both positions.
    let fit = pav(&[1.0, 0.0], &[1.0, 1.0]);
    assert!((fit[0] - 0.5).abs() < 1e-12);
    assert!((fit[1] - 0.5).abs() < 1e-12);
}

// ─── CORP decomposition ───────────────────────────────────────────────────────

#[test]
fn corp_decomposition_equals_mean_brier() {
    // LOAD-BEARING: the CORP identity S̄ = MCB − DSC + UNC, numerically exact.
    let s = samples(&[
        (0.1, false),
        (0.2, false),
        (0.8, true),
        (0.9, true),
        (0.5, true),
        (0.5, false),
        (0.3, true),
        (0.7, false),
    ]);
    let c = corp(&s).expect("non-empty set yields Some");
    let brier = mean_brier(&s);
    assert!(
        (c.mcb - c.dsc + c.unc - brier).abs() < 1e-9,
        "MCB − DSC + UNC ({}) must equal mean Brier ({brier})",
        c.mcb - c.dsc + c.unc
    );
}

#[test]
fn corp_decomposition_identity_second_set() {
    // The identity holds on a second, differently-shaped seeded set.
    let s = samples(&[
        (0.05, false),
        (0.15, true),
        (0.35, false),
        (0.45, true),
        (0.55, false),
        (0.65, true),
        (0.85, true),
        (0.95, false),
        (0.25, true),
        (0.75, true),
    ]);
    let c = corp(&s).expect("non-empty set yields Some");
    let brier = mean_brier(&s);
    assert!((c.mcb - c.dsc + c.unc - brier).abs() < 1e-9);
}

#[test]
fn corp_terms_nonnegative() {
    let s = samples(&[
        (0.1, false),
        (0.2, false),
        (0.8, true),
        (0.9, true),
        (0.5, true),
        (0.5, false),
        (0.3, true),
        (0.7, false),
    ]);
    let c = corp(&s).unwrap();
    assert!(c.mcb >= -1e-12, "mcb >= 0, got {}", c.mcb);
    assert!(c.dsc >= -1e-12, "dsc >= 0, got {}", c.dsc);
    assert!(c.unc >= -1e-12, "unc >= 0, got {}", c.unc);
}

#[test]
fn corp_calibrated_set_has_near_zero_mcb() {
    // A synthetic well-calibrated set: at each forecast level p, the realized
    // event rate ≈ p. Build groups so the empirical rate matches the forecast,
    // already sorted by p → PAV leaves it ~unchanged → MCB ≈ 0.
    let mut s = Vec::new();
    // p=0.2: 1 of 5 true
    for i in 0..5 {
        s.push(CalibrationSample {
            p: 0.2,
            outcome: i < 1,
        });
    }
    // p=0.5: 5 of 10 true
    for i in 0..10 {
        s.push(CalibrationSample {
            p: 0.5,
            outcome: i < 5,
        });
    }
    // p=0.8: 4 of 5 true
    for i in 0..5 {
        s.push(CalibrationSample {
            p: 0.8,
            outcome: i < 4,
        });
    }
    let c = corp(&s).unwrap();
    assert!(
        c.mcb < 1e-6,
        "well-calibrated set should have mcb≈0, got {}",
        c.mcb
    );
}

#[test]
fn corp_empty_is_none() {
    assert!(corp(&[]).is_none());
}

#[test]
fn corp_curve_recalibrated_is_monotone_and_counts_sum() {
    let s = samples(&[
        (0.1, false),
        (0.2, true),
        (0.2, false),
        (0.8, true),
        (0.9, true),
        (0.9, false),
    ]);
    let c = corp(&s).unwrap();
    let recals: Vec<f64> = c.curve.iter().map(|pt| pt.recalibrated).collect();
    assert!(
        is_nondecreasing(&recals),
        "recalibrated curve must be nondecreasing"
    );
    let total: usize = c.curve.iter().map(|pt| pt.count).sum();
    assert_eq!(total, s.len(), "curve counts must sum to N");
}

// ─── Bands ─────────────────────────────────────────────────────────────────

#[test]
fn corp_bands_deterministic() {
    // No RNG anywhere → two calls on identical input give identical bands.
    let s = samples(&[
        (0.1, false),
        (0.2, false),
        (0.8, true),
        (0.9, true),
        (0.5, true),
        (0.5, false),
        (0.3, true),
        (0.7, false),
    ]);
    let c1 = corp(&s).unwrap();
    let c2 = corp(&s).unwrap();
    assert_eq!(c1.band_lo, c2.band_lo, "band_lo must be deterministic");
    assert_eq!(c1.band_hi, c2.band_hi, "band_hi must be deterministic");
}

#[test]
fn corp_bands_bracket_curve_and_clamp() {
    let s = samples(&[
        (0.1, false),
        (0.2, false),
        (0.8, true),
        (0.9, true),
        (0.5, true),
        (0.5, false),
        (0.3, true),
        (0.7, false),
    ]);
    let c = corp(&s).unwrap();
    assert_eq!(c.band_lo.len(), c.curve.len(), "band_lo aligns to curve");
    assert_eq!(c.band_hi.len(), c.curve.len(), "band_hi aligns to curve");
    for (i, pt) in c.curve.iter().enumerate() {
        let lo = c.band_lo[i];
        let hi = c.band_hi[i];
        assert!((0.0..=1.0).contains(&lo), "band_lo clamped to [0,1]: {lo}");
        assert!((0.0..=1.0).contains(&hi), "band_hi clamped to [0,1]: {hi}");
        assert!(lo <= pt.recalibrated + 1e-12, "band_lo ≤ recalibrated");
        assert!(hi >= pt.recalibrated - 1e-12, "band_hi ≥ recalibrated");
    }
}

#[test]
fn corp_serialize_round_trips() {
    let s = samples(&[(0.2, false), (0.8, true), (0.5, true)]);
    let c = corp(&s).unwrap();
    let json = serde_json::to_string(&c).expect("Corp serializes");
    assert!(json.contains("mcb"));
    assert!(json.contains("curve"));
    assert!(json.contains("band_lo"));
}
