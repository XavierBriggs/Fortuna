//! Diebold–Mariano test of equal predictive accuracy (spec 5.5; research §3.x
//! edge; plan Task 5/S5, V&V-2 [Important · S5]).
//!
//! Given two forecasters' per-observation losses (e.g. their per-sample Brier
//! losses), the Diebold–Mariano test asks whether the *mean* loss differential
//! `d_t = loss_a[t] − loss_b[t]` is significantly different from zero — i.e.
//! whether one forecaster is genuinely more accurate, not just luckier on this
//! sample. Because the differential series is typically autocorrelated, the
//! variance of `d̄` is estimated with a **Newey–West HAC** (heteroskedasticity-
//! and autocorrelation-consistent) long-run variance:
//!
//! ```text
//! γ_k     = (1/n) Σ_t (d_t − d̄)(d_{t−k} − d̄)
//! HAC_var = γ_0 + 2 Σ_{k=1}^{L} (1 − k/(L+1)) γ_k          (Bartlett weights)
//! stat    = d̄ / sqrt(HAC_var / n)
//! p_value = 2 · (1 − Φ(|stat|))                            (two-sided normal)
//! ```
//!
//! where `L = hac_lag`. A negative `stat` means A's losses are lower (A is the
//! better forecaster); the sign convention follows `d = loss_a − loss_b`.
//!
//! Guards (return `None`, never panic):
//! - mismatched input lengths (no per-observation differential exists);
//! - `n < 8` (too short for a meaningful HAC variance);
//! - `HAC_var ≤ ~1e-12` (a zero/degenerate differential — e.g. identical
//!   losses — has no variance, so the statistic is undefined; per V&V-2 this is
//!   `None`, NOT a `stat ≈ 0` result).
//!
//! Pure: `std` + `serde` only. The normal CDF `Φ` is a self-contained rational
//! approximation (Abramowitz & Stegun 7.1.26), so the crate carries no `rand`,
//! no `libm`, and no `Clock`.

use serde::{Deserialize, Serialize};

/// Smallest HAC variance treated as nonzero. Below this the loss differential is
/// degenerate (e.g. identical losses) and the statistic is undefined → `None`.
const MIN_HAC_VAR: f64 = 1e-12;

/// Minimum sample length for a meaningful HAC variance.
const MIN_N: usize = 8;

/// Result of a Diebold–Mariano test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DmResult {
    /// The DM statistic `d̄ / sqrt(HAC_var/n)` (negative ⇒ A more accurate).
    pub stat: f64,
    /// Two-sided normal p-value `2·(1 − Φ(|stat|))` in `[0, 1]`.
    pub p_value: f64,
    /// Number of paired observations the statistic was computed over.
    pub n: usize,
}

/// Diebold–Mariano test on the loss differential `d = loss_a − loss_b` with a
/// Newey–West HAC variance of lag `hac_lag`.
///
/// Returns `None` when the inputs cannot yield a meaningful statistic: unequal
/// lengths, `n < 8`, or a near-zero HAC variance (degenerate/zero differential).
/// Otherwise returns the statistic, the two-sided normal p-value, and `n`. Never
/// panics.
pub fn diebold_mariano(loss_a: &[f64], loss_b: &[f64], hac_lag: usize) -> Option<DmResult> {
    if loss_a.len() != loss_b.len() {
        return None;
    }
    let n = loss_a.len();
    if n < MIN_N {
        return None;
    }
    let n_f = n as f64;

    // Per-observation loss differential and its mean.
    let d: Vec<f64> = loss_a
        .iter()
        .zip(loss_b.iter())
        .map(|(a, b)| a - b)
        .collect();
    let d_bar = d.iter().sum::<f64>() / n_f;

    // Autocovariances γ_k = (1/n) Σ (d_t − d̄)(d_{t−k} − d̄).
    let demeaned: Vec<f64> = d.iter().map(|x| x - d_bar).collect();
    let gamma = |k: usize| -> f64 {
        let mut acc = 0.0;
        for t in k..n {
            acc += demeaned[t] * demeaned[t - k];
        }
        acc / n_f
    };

    // Newey–West long-run variance with Bartlett weights. Cap the lag at n−1 so
    // a caller-supplied lag ≥ n contributes only the autocovariances that exist
    // (γ_k is 0 for k ≥ n); the Bartlett weight uses the requested L either way.
    let gamma_0 = gamma(0);
    let l = hac_lag;
    let denom = (l + 1) as f64;
    let mut hac_var = gamma_0;
    for k in 1..=l {
        if k >= n {
            break;
        }
        let weight = 1.0 - (k as f64) / denom;
        hac_var += 2.0 * weight * gamma(k);
    }

    // A degenerate (zero/near-zero) differential has no variance ⇒ undefined.
    // The Bartlett-weighted sum is guaranteed ≥ 0 in exact arithmetic; guard the
    // floor (and any tiny negative float drift) here.
    if hac_var <= MIN_HAC_VAR {
        return None;
    }

    let stat = d_bar / (hac_var / n_f).sqrt();
    let p_value = 2.0 * (1.0 - standard_normal_cdf(stat.abs()));

    Some(DmResult {
        stat,
        p_value: p_value.clamp(0.0, 1.0),
        n,
    })
}

/// Standard-normal CDF `Φ(x) = ½(1 + erf(x/√2))`.
fn standard_normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

/// Error function via the Abramowitz & Stegun 7.1.26 rational approximation
/// (maximum absolute error ≈ 1.5e-7). Pure — no `libm`, no external deps.
fn erf(x: f64) -> f64 {
    // A&S 7.1.26 is stated for x ≥ 0; erf is odd, so reflect for negatives.
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();

    const A1: f64 = 0.254829592;
    const A2: f64 = -0.284496736;
    const A3: f64 = 1.421413741;
    const A4: f64 = -1.453152027;
    const A5: f64 = 1.061405429;
    const P: f64 = 0.3275911;

    let t = 1.0 / (1.0 + P * x);
    let poly = ((((A5 * t + A4) * t + A3) * t + A2) * t + A1) * t;
    let y = 1.0 - poly * (-x * x).exp();

    sign * y
}
