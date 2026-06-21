//! CORP reliability + the MCB−DSC+UNC decomposition of the Brier score.
//!
//! CORP (Consistent, Optimally binned, Reproducible, PAV-based — Dimitriadis,
//! Gneiting & Jordan 2021, PNAS; research §3.1) replaces the bin-and-count
//! Murphy/reliability diagram with a binning-free isotonic fit. For the *same*
//! Brier score the GO gate already uses, it yields the decomposition
//!
//! ```text
//! S̄ = MCB − DSC + UNC      (all three ≥ 0)
//! ```
//!
//! - **MCB** (miscalibration): score lost to being miscalibrated, recoverable
//!   by recalibration — "is the model honest".
//! - **DSC** (discrimination): score gained from separating outcomes — "does
//!   the model know anything", which a Brier-only gate cannot isolate.
//! - **UNC** (uncertainty): base-rate difficulty `ō(1−ō)`, identical across
//!   producers, so cross-period Brier comparisons are fair.
//!
//! The recalibrated event-rate is the Pool-Adjacent-Violators fit of the
//! outcomes ordered by forecast probability ([`crate::pav`]); the three terms
//! are means of squared error against the raw forecast, the recalibrated
//! forecast, and the climatology `ō`, so the identity holds **by
//! construction** (numerically exact, not approximate).
//!
//! Each curve point carries a **deterministic closed-form consistency band** —
//! a Wald/Wilson-style binomial interval on the recalibrated rate using the
//! point's `count` — so a reader can tell a real deviation from sampling noise.
//! This is a closed-form band; the full CORP asymptotic-resampling theory is a
//! later refinement. There is **no randomness**: two calls on the same input
//! return identical bands.
//!
//! Pure: `std` + `serde` only. No panic — an empty sample set returns `None`.
//!
//! Soundness note: MCB used as a *gate threshold* must be cross-fit
//! (recalibration fit on held-out data); the in-sample curve here is a
//! **diagnostic**. The scorecard layer (S6) computes any gated MCB out-of-sample.

use crate::pav::pav;
use crate::samples::CalibrationSample;
use serde::{Deserialize, Serialize};

/// z for a 95% two-sided normal interval (closed-form consistency band).
const BAND_Z: f64 = 1.96;

/// One point of the CORP reliability curve: a distinct forecast level `p`, its
/// recalibrated (isotonic) event rate, and how many samples it covers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReliabilityPoint {
    /// Forecast probability for this group of samples.
    pub p: f64,
    /// Recalibrated (PAV-isotonic) event rate at this level.
    pub recalibrated: f64,
    /// Number of samples at this forecast level.
    pub count: usize,
}

/// CORP decomposition of the Brier score plus the recalibrated reliability
/// curve and its closed-form consistency bands.
///
/// `band_lo`/`band_hi` are aligned index-for-index with `curve`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Corp {
    /// Miscalibration component (≥ 0).
    pub mcb: f64,
    /// Discrimination component (≥ 0).
    pub dsc: f64,
    /// Uncertainty component `ō(1−ō)` (≥ 0).
    pub unc: f64,
    /// Distinct `(p, recalibrated, count)` points, ascending in `p`.
    pub curve: Vec<ReliabilityPoint>,
    /// Lower consistency band per curve point (clamped to [0, 1]).
    pub band_lo: Vec<f64>,
    /// Upper consistency band per curve point (clamped to [0, 1]).
    pub band_hi: Vec<f64>,
}

/// Compute the CORP decomposition over binary `(p, outcome)` samples.
///
/// Returns `None` on an empty set. The decomposition obeys
/// `mcb − dsc + unc == mean Brier` numerically (within float epsilon), each
/// term is `≥ 0`, the recalibrated `curve` is nondecreasing, and the bands are
/// deterministic and clamped to `[0, 1]`.
pub fn corp(samples: &[CalibrationSample]) -> Option<Corp> {
    if samples.is_empty() {
        return None;
    }
    let n = samples.len();
    let n_f = n as f64;

    // Sort by forecast probability ascending (stable: preserves input order at
    // ties, which keeps the result deterministic).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| {
        samples[i]
            .p
            .partial_cmp(&samples[j].p)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let ps: Vec<f64> = order.iter().map(|&i| samples[i].p).collect();
    let outcomes: Vec<f64> = order
        .iter()
        .map(|&i| if samples[i].outcome { 1.0 } else { 0.0 })
        .collect();

    // Recalibrated event-rate = isotonic (PAV) fit of outcomes in p-order.
    let weights = vec![1.0_f64; n];
    let recal = pav(&outcomes, &weights);

    // Climatology (base rate).
    let o_bar = outcomes.iter().sum::<f64>() / n_f;

    // Mean squared error against: raw forecast, recalibrated forecast, climatology.
    let mut sse_raw = 0.0;
    let mut sse_recal = 0.0;
    for k in 0..n {
        let o = outcomes[k];
        let d_raw = ps[k] - o;
        sse_raw += d_raw * d_raw;
        let d_recal = recal[k] - o;
        sse_recal += d_recal * d_recal;
    }
    let mean_brier_raw = sse_raw / n_f;
    let mean_brier_recal = sse_recal / n_f;
    let mean_brier_clim = o_bar * (1.0 - o_bar);

    // Decomposition (identity holds by construction):
    //   UNC = ō(1−ō)
    //   DSC = mean_brier_clim − mean_brier_recal
    //   MCB = mean_brier_raw   − mean_brier_recal
    //   ⇒ MCB − DSC + UNC = mean_brier_raw
    let unc = mean_brier_clim;
    let dsc = mean_brier_clim - mean_brier_recal;
    let mcb = mean_brier_raw - mean_brier_recal;

    // Distinct curve points: group consecutive equal-p samples (already
    // p-sorted). The recalibrated value is averaged over the group (constant
    // within a PAV block; averaging keeps it well-defined and monotone across a
    // block boundary that splits a p-group).
    let mut curve: Vec<ReliabilityPoint> = Vec::new();
    let mut k = 0;
    while k < n {
        let p_here = ps[k];
        let mut j = k;
        let mut recal_sum = 0.0;
        while j < n && (ps[j] - p_here).abs() <= f64::EPSILON {
            recal_sum += recal[j];
            j += 1;
        }
        let count = j - k;
        curve.push(ReliabilityPoint {
            p: p_here,
            recalibrated: recal_sum / count as f64,
            count,
        });
        k = j;
    }

    // Closed-form consistency bands (deterministic; no RNG): a normal-approx
    // binomial interval on the recalibrated rate using the point's count.
    let mut band_lo = Vec::with_capacity(curve.len());
    let mut band_hi = Vec::with_capacity(curve.len());
    for pt in &curve {
        let r = pt.recalibrated;
        let half = if pt.count > 0 {
            BAND_Z * (r * (1.0 - r) / pt.count as f64).sqrt()
        } else {
            0.0
        };
        band_lo.push((r - half).clamp(0.0, 1.0));
        band_hi.push((r + half).clamp(0.0, 1.0));
    }

    Some(Corp {
        mcb,
        dsc,
        unc,
        curve,
        band_lo,
        band_hi,
    })
}
