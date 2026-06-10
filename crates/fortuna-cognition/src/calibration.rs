//! The calibration layer (spec 5.10): Platt scaling, isotonic regression,
//! shrinkage-toward-market prior, extremization, and the quality factor
//! that feeds the sizing haircut (spec 5.14 via T2.6).
//!
//! Doctrine:
//! - Fits use the FORWARD record only. These functions take resolved
//!   (p_raw, outcome) samples handed in by the caller; nothing here reads
//!   history or the ledger.
//! - Below `FULL_AUTONOMY_N` (50) resolved beliefs in a category, a
//!   conservative shrinkage-toward-market prior applies and the fitted
//!   method (and extremization) is IGNORED — low-data categories get
//!   little autonomous weight.
//! - Everything is deterministic: fixed initialization, fixed iteration
//!   bounds, no randomness. The same samples always produce the same
//!   parameters (bit-identical, so fits are comparable and replayable).
//! - Parameters are VERSIONED data (`CalibrationParams`), stored via the
//!   ledger as config changes recorded in audit; this module only defines
//!   the math and the (de)serializable parameter shapes.
//! - Calibrated outputs always stay strictly inside (0,1) — a calibrated
//!   certainty would be a lie and would break log-loss scoring.

use crate::beliefs::CalibrationBucket;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Resolved-belief count at which a category earns full autonomous
/// weight (spec 5.10: "Until a category has N >= 50 resolved beliefs, a
/// conservative shrinkage-toward-market prior applies").
pub const FULL_AUTONOMY_N: usize = 50;

/// Probability clamp keeping every output strictly inside (0,1).
const P_EPS: f64 = 1e-9;

#[derive(Debug, Error)]
pub enum CalibrationError {
    #[error("cannot fit on an empty resolved record")]
    EmptyRecord,
    #[error("degenerate record: {reason}")]
    Degenerate { reason: String },
}

/// Clamp a probability strictly inside (0,1). Non-finite input fails
/// CONSERVATIVE to 0.5 (max uncertainty) — beliefs are validated strictly
/// in (0,1) upstream, so this is defense in depth, not a code path.
fn clamp_p(p: f64) -> f64 {
    if !p.is_finite() {
        return 0.5;
    }
    p.clamp(P_EPS, 1.0 - P_EPS)
}

fn logit(p: f64) -> f64 {
    let p = clamp_p(p);
    (p / (1.0 - p)).ln()
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

// ------------------------------------------------------------------ platt

/// Fitted Platt scaler: `calibrated = sigmoid(a * logit(p_raw) + b)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Platt {
    pub a: f64,
    pub b: f64,
}

impl Platt {
    pub fn apply(&self, p_raw: f64) -> f64 {
        clamp_p(sigmoid(self.a * logit(p_raw) + self.b))
    }
}

/// Fit a Platt scaler on resolved (claimed probability, outcome) samples
/// by Newton iteration on the logistic log-likelihood. Deterministic:
/// fixed start (a=1, b=0 — the identity map), fixed iteration bound,
/// tolerance-based early exit. Degenerate records (empty, all one
/// outcome, no spread) are refused — an identity fallback would lie.
pub fn fit_platt(samples: &[(f64, bool)]) -> Result<Platt, CalibrationError> {
    if samples.is_empty() {
        return Err(CalibrationError::EmptyRecord);
    }
    let positives = samples.iter().filter(|(_, y)| *y).count();
    if positives == 0 || positives == samples.len() {
        return Err(CalibrationError::Degenerate {
            reason: "all outcomes identical; a logistic fit cannot be identified".to_string(),
        });
    }
    for (p, _) in samples {
        if !p.is_finite() {
            return Err(CalibrationError::Degenerate {
                reason: "non-finite probability in the resolved record".to_string(),
            });
        }
    }

    let xs: Vec<f64> = samples.iter().map(|(p, _)| logit(*p)).collect();
    let ys: Vec<f64> = samples
        .iter()
        .map(|(_, y)| if *y { 1.0 } else { 0.0 })
        .collect();

    let (mut a, mut b) = (1.0_f64, 0.0_f64);
    for _ in 0..100 {
        let (mut ga, mut gb) = (0.0_f64, 0.0_f64);
        let (mut haa, mut hab, mut hbb) = (0.0_f64, 0.0_f64, 0.0_f64);
        for (x, y) in xs.iter().zip(ys.iter()) {
            let mu = sigmoid(a * x + b);
            let r = mu - y;
            ga += r * x;
            gb += r;
            let w = mu * (1.0 - mu);
            haa += w * x * x;
            hab += w * x;
            hbb += w;
        }
        let det = haa * hbb - hab * hab;
        if det.abs() < 1e-12 {
            // No spread in predictions (single distinct input, or the fit
            // saturated under perfect separation): unidentifiable.
            return Err(CalibrationError::Degenerate {
                reason: "singular Hessian; predictions carry no identifiable spread".to_string(),
            });
        }
        let da = (gb * hab - ga * hbb) / det;
        let db = (ga * hab - gb * haa) / det;
        a += da;
        b += db;
        if !a.is_finite() || !b.is_finite() {
            return Err(CalibrationError::Degenerate {
                reason: "fit diverged".to_string(),
            });
        }
        if da.abs() < 1e-12 && db.abs() < 1e-12 {
            break;
        }
    }
    Ok(Platt { a, b })
}

// --------------------------------------------------------------- isotonic

/// One step of a fitted isotonic (non-decreasing) map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IsoStep {
    /// Claimed-probability threshold (a distinct input value seen in the
    /// resolved record).
    pub x: f64,
    /// Pooled observed frequency assigned to inputs at or above `x`
    /// (until the next step).
    pub y: f64,
}

/// Fitted isotonic regressor: a non-decreasing step function from
/// claimed probability to pooled observed frequency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Isotonic {
    pub steps: Vec<IsoStep>,
}

impl Isotonic {
    /// Step-function application: the fitted value of the largest
    /// threshold at or below `p_raw`; inputs below the first threshold
    /// take the first fitted value. Monotone by construction.
    pub fn apply(&self, p_raw: f64) -> f64 {
        let p = clamp_p(p_raw);
        let mut value = match self.steps.first() {
            Some(first) => first.y,
            // Unreachable for any fit (fit_isotonic refuses empty), but
            // a deserialized empty map fails conservative, not loud.
            None => 0.5,
        };
        for step in &self.steps {
            if step.x <= p {
                value = step.y;
            } else {
                break;
            }
        }
        value
    }
}

/// Fit an isotonic regression by pool-adjacent-violators on resolved
/// (claimed probability, outcome) samples. Deterministic: samples are
/// sorted by claimed probability, ties merged, then PAV pools adjacent
/// blocks whose observed frequencies violate monotonicity.
pub fn fit_isotonic(samples: &[(f64, bool)]) -> Result<Isotonic, CalibrationError> {
    if samples.is_empty() {
        return Err(CalibrationError::EmptyRecord);
    }
    for (p, _) in samples {
        if !p.is_finite() {
            return Err(CalibrationError::Degenerate {
                reason: "non-finite probability in the resolved record".to_string(),
            });
        }
    }

    // Sort by claimed probability; merge identical inputs into weighted
    // points (x, hit_sum, weight).
    let mut sorted: Vec<(f64, f64)> = samples
        .iter()
        .map(|(p, y)| (clamp_p(*p), if *y { 1.0 } else { 0.0 }))
        .collect();
    sorted.sort_by(|l, r| l.partial_cmp(r).unwrap_or(std::cmp::Ordering::Equal));
    let mut points: Vec<(f64, f64, f64)> = Vec::new();
    for (x, y) in sorted {
        match points.last_mut() {
            Some((px, sum, w)) if *px == x => {
                *sum += y;
                *w += 1.0;
            }
            _ => points.push((x, y, 1.0)),
        }
    }

    // PAV: maintain a stack of blocks (mean, weight, first point index);
    // merge while the tail violates non-decreasing order.
    let mut blocks: Vec<(f64, f64, usize)> = Vec::new();
    for (i, (_, sum, w)) in points.iter().enumerate() {
        blocks.push((sum / w, *w, i));
        while blocks.len() >= 2 {
            let (m2, w2, _) = blocks[blocks.len() - 1];
            let (m1, w1, s1) = blocks[blocks.len() - 2];
            if m1 <= m2 {
                break;
            }
            blocks.pop();
            blocks.pop();
            blocks.push(((m1 * w1 + m2 * w2) / (w1 + w2), w1 + w2, s1));
        }
    }

    // Expand blocks back over their covered points as steps.
    let mut steps = Vec::with_capacity(points.len());
    for (bi, &(mean, _, start)) in blocks.iter().enumerate() {
        let end = blocks
            .get(bi + 1)
            .map(|&(_, _, next_start)| next_start)
            .unwrap_or(points.len());
        for point in &points[start..end] {
            steps.push(IsoStep {
                x: point.0,
                y: mean,
            });
        }
    }
    Ok(Isotonic { steps })
}

// -------------------------------------------------------------- shrinkage

/// The conservative low-data prior (spec 5.10): blend the model's claim
/// toward the market's implied probability with autonomous weight
/// `w = min(n / 50, 1)`. At n=0 the output IS the market; at n>=50 the
/// model speaks for itself.
pub fn shrink_toward_market(p_model: f64, p_market: f64, resolved_n: usize) -> f64 {
    let w = (resolved_n as f64 / FULL_AUTONOMY_N as f64).min(1.0);
    clamp_p(w * clamp_p(p_model) + (1.0 - w) * clamp_p(p_market))
}

// ----------------------------------------------------------- extremization

/// Extremization (spec 5.10: "where the weekly audit supports it"):
/// `sigmoid(k * logit(p))`. k=1 is the identity; k>1 pushes away from
/// 0.5; 0.5 is a fixed point. Output stays strictly inside (0,1).
pub fn extremize(p: f64, k: f64) -> f64 {
    let p = clamp_p(p);
    if k == 1.0 {
        return p;
    }
    clamp_p(sigmoid(k * logit(p)))
}

// ------------------------------------------------------------- parameters

/// Which fitted method a parameter set carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CalibrationMethod {
    Platt(Platt),
    Isotonic(Isotonic),
}

impl CalibrationMethod {
    /// Stable kind tag for storage and audit rows.
    pub fn kind(&self) -> &'static str {
        match self {
            CalibrationMethod::Platt(_) => "platt",
            CalibrationMethod::Isotonic(_) => "isotonic",
        }
    }

    fn apply(&self, p_raw: f64) -> f64 {
        match self {
            CalibrationMethod::Platt(platt) => platt.apply(p_raw),
            CalibrationMethod::Isotonic(iso) => iso.apply(p_raw),
        }
    }
}

/// Versioned calibration parameters for one (model, strategy, category)
/// scope. Parameter updates are config changes recorded in audit; the
/// version rides into every belief calibrated with them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationParams {
    pub version: u32,
    pub method: CalibrationMethod,
    /// Extremization exponent; 1.0 disables (the conservative default
    /// until the weekly audit supports more).
    pub extremization_k: f64,
    /// Resolved-sample count the method was fitted on (provenance).
    pub fitted_on_n: usize,
}

/// The full calibration pipeline for one raw claim.
///
/// Below `FULL_AUTONOMY_N` resolved beliefs in the category, the fitted
/// method and extremization are IGNORED and the shrinkage prior applies;
/// with no market prior available, the claim shrinks toward 0.5 (max
/// uncertainty) — conservative, never a crash. At or above the
/// threshold, the fitted method runs, then extremization.
pub fn calibrate(
    p_raw: f64,
    params: &CalibrationParams,
    market_p: Option<f64>,
    resolved_n: usize,
) -> f64 {
    if resolved_n < FULL_AUTONOMY_N {
        let prior = market_p.map(clamp_p).unwrap_or(0.5);
        return shrink_toward_market(p_raw, prior, resolved_n);
    }
    extremize(params.method.apply(p_raw), params.extremization_k)
}

// ---------------------------------------------------------------- quality

/// Calibration quality in [0,1] for the sizing haircut (T2.6):
/// `min(n/50, 1) * max(0, 1 - 2 * weighted_mean(|mean_p - observed|))`.
///
/// Both factors are required: a tiny sample ramps quality down even if
/// the curve looks perfect (it proves nothing yet), and a wide
/// claim-vs-frequency gap zeroes quality regardless of n (a 50-point
/// average gap — coin-flip claimed as certainty — hits exactly zero).
pub fn calibration_quality(curve: &[CalibrationBucket], resolved_n: usize) -> f64 {
    if curve.is_empty() || resolved_n == 0 {
        return 0.0;
    }
    let total: f64 = curve.iter().map(|b| b.n as f64).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let weighted_gap: f64 = curve
        .iter()
        .map(|b| b.n as f64 * (b.mean_p - b.observed_frequency).abs())
        .sum::<f64>()
        / total;
    let accuracy = (1.0 - 2.0 * weighted_gap).max(0.0);
    let ramp = (resolved_n as f64 / FULL_AUTONOMY_N as f64).min(1.0);
    (ramp * accuracy).clamp(0.0, 1.0)
}
