//! PIT (Probability Integral Transform) value and histogram for *continuous*
//! scalar predictive distributions (spec 5.5; research §3.x calibration).
//!
//! The PIT value of a realized observation is `u = F(y)`: the predictive CDF
//! evaluated at the realized value `y`. This is the **same CDF-at-realized PIT
//! definition Aeolus documents for its `scorecards.pit` column** — the share of
//! the predictive mass at or below what actually happened. If the forecasts are
//! calibrated, the PIT values are uniform on `[0, 1]`; a U-shaped PIT histogram
//! flags under-dispersion (over-confidence) and a hump flags over-dispersion.
//!
//! Here `F` is reconstructed by **linear interpolation over the `(q, v)`
//! quantile ladder** (`q` = quantile level in `(0, 1)`, `v` = forecast value,
//! non-decreasing). `u` is clamped to `[0, 1]`: a realized value at or below the
//! lowest rung reads `0.0`, at or above the highest reads `1.0`.
//!
//! # Discrete-distribution caveat (load-bearing)
//!
//! This continuous-CDF PIT is only honest for a **continuous** predictive
//! distribution. For a *discrete* predictive distribution the CDF has jumps, so
//! `F(y)` is not uniform even under perfect calibration; the correct tools are
//! the **randomized PIT** (draw `U ~ Uniform(F(y⁻), F(y))`) or the
//! non-randomized Czado–Gneiting–Held (2009) PIT. **Neither is implemented
//! here**, and neither is needed: this function serves the **continuous Aeolus
//! quantile envelope** (a smooth quantile ladder), where the plain CDF-at-
//! realized PIT is the right diagnostic. Do not apply `pit_value` to a discrete
//! producer without first switching to a randomized PIT.
//!
//! Pure: `std` + `serde` only. No panic — an empty ladder returns `None`, and
//! `pit_histogram` with `k_bins == 0` returns an empty `Vec`.

use crate::rules::Quantile;
use serde::{Deserialize, Serialize};

/// One equal-width bin of a PIT histogram over `[0, 1]`.
///
/// `lo`/`hi` are the (inclusive-lo, exclusive-hi) edges of the bin — except the
/// top bin, whose upper edge `hi == 1.0` is inclusive so a PIT value of exactly
/// `1.0` is counted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitBin {
    /// Lower edge of the bin (inclusive).
    pub lo: f64,
    /// Upper edge of the bin (exclusive, except the top bin which is inclusive).
    pub hi: f64,
    /// Number of PIT values that fell in this bin.
    pub count: usize,
}

/// PIT value `u = F(realized)`: the predictive CDF at the realized value, by
/// linear interpolation over the `(q, v)` quantile ladder.
///
/// The ladder pairs each quantile level `q` (the CDF value, in `(0, 1)`) with
/// its forecast value `v` (non-decreasing). To evaluate the CDF at `realized`:
///
/// - `realized <= v[0]` → `0.0` (at or below the lowest rung).
/// - `realized >= v[last]` → `1.0` (at or above the highest rung).
/// - otherwise locate the bracketing rungs `i, i+1` with `v[i] <= realized <
///   v[i+1]` and interpolate the level:
///   `u = q[i] + (q[i+1] − q[i]) · (realized − v[i]) / (v[i+1] − v[i])`.
///
/// A flat segment (`v[i] == v[i+1]`) would divide by zero, so it is guarded to
/// return `q[i]` (the value is fully attained at the lower rung). The result is
/// clamped to `[0, 1]`.
///
/// Returns `None` for an empty ladder; never panics.
pub fn pit_value(quantiles: &[Quantile], realized: f64) -> Option<f64> {
    if quantiles.is_empty() {
        return None;
    }

    // At or below the lowest forecast value → no predictive mass below it.
    let first = &quantiles[0];
    if realized <= first.v {
        return Some(0.0);
    }

    // At or above the highest forecast value → all predictive mass is below it.
    let last = &quantiles[quantiles.len() - 1];
    if realized >= last.v {
        return Some(1.0);
    }

    // Strictly inside the ladder: find the bracketing rungs and interpolate the
    // quantile level. The ladder's `v` is non-decreasing (rules::validate_scalar
    // enforces this on stored claims), so the first rung whose upper `v`
    // exceeds `realized` is the right bracket.
    for pair in quantiles.windows(2) {
        let lo = &pair[0];
        let hi = &pair[1];
        if realized >= lo.v && realized < hi.v {
            let span = hi.v - lo.v;
            let u = if span <= 0.0 {
                // Flat segment v[i] == v[i+1]: no width to interpolate over.
                // The value is attained at the lower rung → use q[i].
                lo.q
            } else {
                lo.q + (hi.q - lo.q) * (realized - lo.v) / span
            };
            return Some(u.clamp(0.0, 1.0));
        }
    }

    // Unreachable for a non-decreasing ladder given the endpoint guards above,
    // but stay total: fall back to the top of the ladder rather than panic.
    Some(1.0)
}

/// Equal-width `[0, 1]` histogram of PIT values into `k_bins` bins.
///
/// Each bin spans `[lo, hi)` of width `1 / k_bins`, except the top bin whose
/// upper edge is inclusive so a PIT value of exactly `1.0` lands in it. PIT
/// values are assumed already in `[0, 1]` (the output of [`pit_value`]); any
/// value at or above `1.0` is placed in the top bin and any below `0.0` in the
/// bottom bin, so no observation is silently dropped.
///
/// Returns an empty `Vec` when `k_bins == 0`; never panics.
pub fn pit_histogram(us: &[f64], k_bins: usize) -> Vec<PitBin> {
    if k_bins == 0 {
        return Vec::new();
    }

    let width = 1.0 / k_bins as f64;
    let mut bins: Vec<PitBin> = (0..k_bins)
        .map(|i| {
            let lo = i as f64 * width;
            // Pin the final upper edge to exactly 1.0 (avoid float drift).
            let hi = if i + 1 == k_bins {
                1.0
            } else {
                (i + 1) as f64 * width
            };
            PitBin { lo, hi, count: 0 }
        })
        .collect();

    let top = k_bins - 1;
    for &u in us {
        // Map u → bin index. A u of exactly 1.0 (or above) lands in the top bin;
        // a u below 0.0 lands in the bottom bin. floor gives the equal-width
        // bucket for interior values.
        let idx = if u >= 1.0 {
            top
        } else if u <= 0.0 {
            0
        } else {
            // 0 < u < 1 ⇒ floor(u * k_bins) ∈ [0, k_bins-1].
            let raw = (u * k_bins as f64).floor() as usize;
            raw.min(top)
        };
        bins[idx].count += 1;
    }

    bins
}
