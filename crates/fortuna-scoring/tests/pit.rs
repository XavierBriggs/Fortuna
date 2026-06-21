//! Black-box tests for the PIT (Probability Integral Transform) value and
//! histogram (S4). The brief defines `pit_value(quantiles, realized)` as the
//! predictive CDF at the realized value by linear interpolation over the `(q, v)`
//! quantile ladder, and `pit_histogram(us, k_bins)` as an equal-width [0, 1]
//! histogram of PIT values.

use fortuna_scoring::pit::{pit_histogram, pit_value};
use fortuna_scoring::rules::Quantile;

/// Convenience: build a quantile ladder from (q, v) pairs.
fn ladder(rungs: &[(f64, f64)]) -> Vec<Quantile> {
    rungs.iter().map(|&(q, v)| Quantile { q, v }).collect()
}

#[test]
fn pit_at_median_is_half() {
    // Realized lands exactly on the median rung (q=0.5, v=10.0) → CDF = 0.5.
    let q = ladder(&[(0.25, 9.0), (0.5, 10.0), (0.75, 11.0)]);
    let u = pit_value(&q, 10.0).expect("non-empty ladder yields Some");
    assert!((u - 0.5).abs() < 1e-9, "expected PIT≈0.5, got {u}");
}

#[test]
fn pit_below_lowest_is_zero() {
    // Realized below the lowest forecast value → CDF clamped to 0.0.
    let q = ladder(&[(0.25, 9.0), (0.5, 10.0), (0.75, 11.0)]);
    let u = pit_value(&q, 8.0).expect("non-empty ladder yields Some");
    assert_eq!(u, 0.0, "below-lowest PIT must be exactly 0.0, got {u}");
}

#[test]
fn pit_above_highest_is_one() {
    // Realized above the highest forecast value → CDF clamped to 1.0.
    let q = ladder(&[(0.25, 9.0), (0.5, 10.0), (0.75, 11.0)]);
    let u = pit_value(&q, 12.0).expect("non-empty ladder yields Some");
    assert_eq!(u, 1.0, "above-highest PIT must be exactly 1.0, got {u}");
}

#[test]
fn pit_interpolates() {
    // realized=9.5 sits halfway between (0.25, 9.0) and (0.5, 10.0):
    //   u = 0.25 + (0.5 - 0.25) * (9.5 - 9.0) / (10.0 - 9.0) = 0.375
    let q = ladder(&[(0.25, 9.0), (0.5, 10.0), (0.75, 11.0)]);
    let u = pit_value(&q, 9.5).expect("non-empty ladder yields Some");
    assert!((u - 0.375).abs() < 1e-9, "expected PIT=0.375, got {u}");
}

#[test]
fn pit_empty_ladder_is_none() {
    let q: Vec<Quantile> = Vec::new();
    assert!(pit_value(&q, 10.0).is_none());
}

#[test]
fn pit_histogram_bins_sum_to_n() {
    // Five PIT values, 10 bins: total count must equal the input length, and a
    // u of exactly 1.0 must be counted (lands in the top bin).
    let us = [0.05, 0.15, 0.55, 0.95, 1.0];
    let bins = pit_histogram(&us, 10);
    let total: usize = bins.iter().map(|b| b.count).sum();
    assert_eq!(total, us.len(), "histogram must count every PIT value once");
}

#[test]
fn pit_histogram_uniform_ish() {
    // Evenly-spread PIT values → roughly equal bin counts (loose: a uniform
    // sample should never pile most of its mass into one bin). With 100 evenly
    // spaced values over [0,1) and 10 bins, each bin holds ~10.
    let us: Vec<f64> = (0..100).map(|i| i as f64 / 100.0).collect();
    let bins = pit_histogram(&us, 10);
    assert_eq!(bins.len(), 10, "k_bins bins expected");
    let total: usize = bins.iter().map(|b| b.count).sum();
    assert_eq!(total, us.len());
    // Loose calibration assertion: no bin is empty and none holds more than 3×
    // the uniform expectation.
    let expected = us.len() / bins.len();
    for b in &bins {
        assert!(b.count > 0, "uniform input should not leave a bin empty");
        assert!(
            b.count <= expected * 3,
            "bin count {} far exceeds uniform expectation {expected}",
            b.count
        );
    }
}

#[test]
fn pit_histogram_zero_bins_is_empty() {
    // k_bins = 0 must not panic; it returns an empty histogram.
    let us = [0.1, 0.5, 0.9];
    assert!(pit_histogram(&us, 0).is_empty());
}

#[test]
fn pit_histogram_bins_partition_unit_interval() {
    // Bin edges tile [0, 1] contiguously: lo[0]=0, hi[last]=1, hi[i]==lo[i+1].
    let us = [0.5];
    let bins = pit_histogram(&us, 4);
    assert_eq!(bins.len(), 4);
    assert!((bins[0].lo - 0.0).abs() < 1e-12);
    assert!((bins[3].hi - 1.0).abs() < 1e-12);
    for w in bins.windows(2) {
        assert!((w[0].hi - w[1].lo).abs() < 1e-12, "bins must be contiguous");
    }
}

#[test]
fn pit_value_guards_flat_ladder_segment() {
    // Brief: guard div-by-zero when v[i]==v[i+1] → use q[i]. A ladder with a
    // flat segment must not panic or produce NaN.
    let q = ladder(&[(0.25, 9.0), (0.5, 9.0), (0.75, 11.0)]);
    let u = pit_value(&q, 9.0).expect("non-empty ladder yields Some");
    assert!(u.is_finite(), "flat-segment PIT must be finite, got {u}");
    assert!((0.0..=1.0).contains(&u), "PIT must be in [0,1], got {u}");
}
