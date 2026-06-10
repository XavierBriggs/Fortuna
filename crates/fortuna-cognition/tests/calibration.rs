//! T2.8: the calibration layer (spec 5.10) — Platt scaling, isotonic
//! regression, shrinkage-toward-market prior, extremization, versioned
//! parameters, and the quality factor feeding the T2.6 sizing haircut.
//!
//! Doctrine under test:
//! - Fits use the FORWARD record only (resolved beliefs handed in by the
//!   caller — nothing here reads history).
//! - Below N = 50 resolved beliefs in a category, a conservative
//!   shrinkage-toward-market prior applies (low-data categories get
//!   little autonomous weight); at and above 50 the fitted method runs.
//! - Everything is deterministic code with VERSIONED parameters; the
//!   same samples always fit the same parameters.
//! - Calibrated outputs always stay strictly inside (0,1).
//!
//! Written BEFORE src/calibration.rs per the repository TDD doctrine.

use fortuna_cognition::beliefs::calibration_curve;
use fortuna_cognition::calibration::{
    calibrate, calibration_quality, extremize, fit_isotonic, fit_platt, shrink_toward_market,
    CalibrationMethod, CalibrationParams,
};

// ----------------------------------------------------------------- platt

#[test]
fn platt_tempers_overconfident_forecasts() {
    // Overconfident synthetic record: claims at 0.9/0.1 that resolve at
    // ~0.7/0.3 frequency.
    let mut samples = Vec::new();
    for i in 0..40 {
        samples.push((0.9, i % 10 < 7)); // 70% hit at claimed 90%
        samples.push((0.1, i % 10 < 3)); // 30% hit at claimed 10%
    }
    let platt = fit_platt(&samples).unwrap();
    let calibrated_hi = platt.apply(0.9);
    let calibrated_lo = platt.apply(0.1);
    assert!(
        calibrated_hi < 0.85 && calibrated_hi > 0.55,
        "overconfident 0.9 tempers toward 0.7, got {calibrated_hi}"
    );
    assert!(
        calibrated_lo > 0.15 && calibrated_lo < 0.45,
        "overconfident 0.1 tempers toward 0.3, got {calibrated_lo}"
    );
    // Monotone: ordering preserved.
    assert!(platt.apply(0.8) < platt.apply(0.9));
    // Deterministic: same samples, same fit.
    let again = fit_platt(&samples).unwrap();
    assert_eq!(platt, again);
}

#[test]
fn platt_on_well_calibrated_data_is_near_identity() {
    let mut samples = Vec::new();
    for i in 0..100 {
        samples.push((0.7, i % 10 < 7));
        samples.push((0.3, i % 10 < 3));
        samples.push((0.5, i % 2 == 0));
    }
    let platt = fit_platt(&samples).unwrap();
    assert!((platt.apply(0.7) - 0.7).abs() < 0.08);
    assert!((platt.apply(0.3) - 0.3).abs() < 0.08);
}

#[test]
fn platt_refuses_degenerate_records() {
    // All-one-outcome data cannot fit a logistic; identity would lie.
    let all_yes: Vec<(f64, bool)> = (0..20).map(|_| (0.6, true)).collect();
    assert!(fit_platt(&all_yes).is_err());
    assert!(fit_platt(&[]).is_err());
}

// -------------------------------------------------------------- isotonic

#[test]
fn isotonic_pools_adjacent_violators_and_stays_monotone() {
    // Violations: 0.2->1, 0.4->0 must pool.
    let samples = vec![
        (0.1, false),
        (0.2, true),
        (0.4, false),
        (0.6, true),
        (0.8, true),
        (0.9, true),
    ];
    let iso = fit_isotonic(&samples).unwrap();
    // Applied outputs are non-decreasing in the input.
    let xs = [0.05, 0.15, 0.3, 0.5, 0.7, 0.85, 0.95];
    let ys: Vec<f64> = xs.iter().map(|x| iso.apply(*x)).collect();
    for w in ys.windows(2) {
        assert!(w[0] <= w[1] + 1e-12, "monotone violated: {ys:?}");
    }
    // Outputs stay inside [0,1] and the high end is high.
    assert!(ys.iter().all(|y| (0.0..=1.0).contains(y)));
    assert!(iso.apply(0.9) > 0.7);
    assert!(fit_isotonic(&[]).is_err());
}

// ------------------------------------------------------------- shrinkage

#[test]
fn shrinkage_ramps_autonomous_weight_with_resolved_count() {
    // n = 0: pure market prior.
    assert!((shrink_toward_market(0.9, 0.5, 0) - 0.5).abs() < 1e-12);
    // n = 25: w = 0.5 -> halfway.
    assert!((shrink_toward_market(0.9, 0.5, 25) - 0.7).abs() < 1e-12);
    // n >= 50: full autonomous weight.
    assert!((shrink_toward_market(0.9, 0.5, 50) - 0.9).abs() < 1e-12);
    assert!((shrink_toward_market(0.9, 0.5, 500) - 0.9).abs() < 1e-12);
}

// ---------------------------------------------------------- extremization

#[test]
fn extremization_pushes_outward_only_when_configured() {
    assert!((extremize(0.6, 1.0) - 0.6).abs() < 1e-12, "k=1 is identity");
    assert!(extremize(0.6, 2.0) > 0.6, "k>1 pushes away from 0.5");
    assert!(extremize(0.4, 2.0) < 0.4);
    assert!(
        (extremize(0.5, 3.0) - 0.5).abs() < 1e-12,
        "0.5 is the fixed point"
    );
    // Stays strictly inside (0,1).
    let e = extremize(0.99, 5.0);
    assert!(e > 0.99 && e < 1.0);
}

// ---------------------------------------------------------- full pipeline

#[test]
fn calibrate_uses_shrinkage_below_50_and_the_fit_at_or_above() {
    let mut samples = Vec::new();
    for i in 0..60 {
        samples.push((0.9, i % 10 < 7));
        samples.push((0.1, i % 10 < 3));
    }
    let platt = fit_platt(&samples).unwrap();
    let params = CalibrationParams {
        version: 1,
        method: CalibrationMethod::Platt(platt),
        extremization_k: 1.0,
        fitted_on_n: 120,
    };

    // Low-data category: the FIT IS IGNORED, shrinkage applies.
    let low = calibrate(0.9, &params, Some(0.5), 10);
    assert!((low - shrink_toward_market(0.9, 0.5, 10)).abs() < 1e-12);

    // Enough data: the fitted method applies (tempering the 0.9 claim).
    let high = calibrate(0.9, &params, Some(0.5), 120);
    assert!(high < 0.85, "fitted Platt tempers, got {high}");

    // No market prior available below threshold: fail CONSERVATIVE to
    // the raw p shrunk toward 0.5 (max uncertainty), never a crash.
    let no_market = calibrate(0.9, &params, None, 10);
    assert!(no_market < 0.9 && no_market > 0.5);

    // Output is always strictly inside (0,1).
    assert!(calibrate(0.999, &params, Some(0.5), 120) < 1.0);
    assert!(calibrate(0.001, &params, Some(0.5), 120) > 0.0);
}

// ----------------------------------------------------------------- quality

#[test]
fn quality_needs_both_sample_count_and_curve_accuracy() {
    // Perfect calibration with plenty of samples -> quality ~ 1.
    let mut good = Vec::new();
    for i in 0..100 {
        good.push((0.7, i % 10 < 7));
        good.push((0.3, i % 10 < 3));
    }
    let curve = calibration_curve(&good, 10);
    let q = calibration_quality(&curve, good.len());
    assert!(q > 0.85, "well-calibrated + n=200 => high quality, got {q}");

    // Same accuracy, tiny sample -> ramped down by n/50.
    let q_small = calibration_quality(&curve, 10);
    assert!(q_small <= 0.2 + 1e-9);

    // Badly miscalibrated record -> low quality regardless of n.
    let mut bad = Vec::new();
    for i in 0..100 {
        bad.push((0.9, i % 10 < 3)); // claims 90%, hits 30%
    }
    let bad_curve = calibration_curve(&bad, 10);
    let q_bad = calibration_quality(&bad_curve, bad.len());
    assert!(
        q_bad < 0.2,
        "60-point gap => near-zero quality, got {q_bad}"
    );

    // No samples at all -> zero.
    assert_eq!(calibration_quality(&[], 0), 0.0);
}

// -------------------------------------------------------------- versioning

#[test]
fn params_serde_round_trip_with_version() {
    let mut samples = Vec::new();
    for i in 0..30 {
        samples.push((0.8, i % 10 < 6));
        samples.push((0.2, i % 10 < 4));
    }
    let params = CalibrationParams {
        version: 3,
        method: CalibrationMethod::Isotonic(fit_isotonic(&samples).unwrap()),
        extremization_k: 1.2,
        fitted_on_n: 60,
    };
    let json = serde_json::to_string(&params).unwrap();
    let back: CalibrationParams = serde_json::from_str(&json).unwrap();
    assert_eq!(params, back);
    assert_eq!(back.version, 3);
}
