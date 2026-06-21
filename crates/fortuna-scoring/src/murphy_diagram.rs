//! Murphy diagram: the elementary cost-loss score swept over a decision
//! threshold, and the dominance test it makes legible (spec 5.5; research
//! §3.x edge; plan Task 5/S5, V&V-2 [M4 · S5]).
//!
//! A *single* proper score (Brier/Log) ranks two forecasters by one number, but
//! that number can hide a crossing: forecaster A may serve cautious users (who
//! act only on near-certain signals) better while B serves aggressive users
//! better. The Murphy diagram exposes this by scoring each forecaster with the
//! family of **elementary cost-loss scoring functions**, one per decision
//! threshold `θ ∈ (0, 1)`, and plotting the mean score against `θ`.
//!
//! For a binary event the elementary score of a forecast `p` against outcome
//! `o ∈ {0, 1}` at threshold `θ` is
//!
//! ```text
//! S_θ(p, o) = θ·(1 − o)·𝟙{p > θ} + (1 − θ)·o·𝟙{p ≤ θ}
//! ```
//!
//! This is the regret of a user with cost/loss ratio `θ` who **acts when
//! `p > θ`**: if they act and the event does not occur they pay cost `θ` (the
//! first term); if they abstain and the event does occur they suffer loss
//! `1 − θ` (the second term). Lower is better. Averaging `S_θ` over a
//! forecaster's `(p, o)` samples gives that forecaster's expected regret for a
//! θ-user; the Murphy *curve* is that mean against `θ`. A forecaster whose curve
//! lies at or below another's at **every** `θ` is preferred by **every** user
//! regardless of their cost/loss ratio — Murphy dominance, the shadow-comparison
//! evidence WS2 records (it RECOMMENDS, it does not auto-promote — I7).
//!
//! Pure: `std` + `serde` only. No panic — empty sample sets simply score `0.0`
//! everywhere, and `dominates` of an empty (or equal) curve returns `None`.

use crate::samples::CalibrationSample;
use serde::{Deserialize, Serialize};

/// One point of a Murphy diagram: the decision threshold `theta` and each
/// forecaster's mean elementary cost-loss score there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MurphyPoint {
    /// Decision threshold (cost/loss ratio) `θ ∈ (0, 1)`.
    pub theta: f64,
    /// Mean elementary score of forecaster A at this `θ` (lower is better).
    pub score_a: f64,
    /// Mean elementary score of forecaster B at this `θ` (lower is better).
    pub score_b: f64,
}

/// Mean elementary cost-loss score of one forecaster's samples at threshold `θ`.
///
/// `S_θ(p, o) = θ·(1 − o)·𝟙{p > θ} + (1 − θ)·o·𝟙{p ≤ θ}`, averaged over the
/// samples. An empty sample set scores `0.0` (no regret accrues over no
/// decisions); never panics.
fn mean_elementary_score(samples: &[CalibrationSample], theta: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let total: f64 = samples
        .iter()
        .map(|s| {
            if s.outcome {
                // Event occurred: regret only if the user abstained (p ≤ θ).
                if s.p <= theta {
                    1.0 - theta
                } else {
                    0.0
                }
            } else {
                // Event did not occur: regret only if the user acted (p > θ).
                if s.p > theta {
                    theta
                } else {
                    0.0
                }
            }
        })
        .sum();
    total / samples.len() as f64
}

/// Murphy diagram of two forecasters over an equal-spaced **interior** grid of
/// `grid` thresholds in `(0, 1)`.
///
/// The grid is `θ_i = (i + 1) / (grid + 1)` for `i ∈ [0, grid)`, so every
/// threshold is strictly inside `(0, 1)` (the degenerate `θ = 0` and `θ = 1`,
/// where the score is trivially zero, are excluded) and the points are evenly
/// spaced. Returns an empty `Vec` when `grid == 0`; never panics.
pub fn murphy_curve(
    a: &[CalibrationSample],
    b: &[CalibrationSample],
    grid: usize,
) -> Vec<MurphyPoint> {
    if grid == 0 {
        return Vec::new();
    }
    let denom = (grid + 1) as f64;
    (0..grid)
        .map(|i| {
            let theta = (i + 1) as f64 / denom;
            MurphyPoint {
                theta,
                score_a: mean_elementary_score(a, theta),
                score_b: mean_elementary_score(b, theta),
            }
        })
        .collect()
}

/// Murphy-dominance verdict over a curve from [`murphy_curve`].
///
/// Returns `Some("a")` if forecaster A's score is ≤ B's at **every** grid point
/// with at least one **strict** inequality (A is preferred by every cost/loss
/// user and strictly better for at least one); `Some("b")` symmetrically; and
/// `None` if the curves cross, are exactly equal, or the curve is empty (no
/// dominance can be established). Never panics.
pub fn dominates(curve: &[MurphyPoint]) -> Option<&'static str> {
    if curve.is_empty() {
        return None;
    }

    // Compare with a tiny tolerance so float round-off in the sweep is not read
    // as a crossing.
    const EPS: f64 = 1e-12;
    let mut a_ever_strictly_better = false;
    let mut b_ever_strictly_better = false;
    for pt in curve {
        if pt.score_a < pt.score_b - EPS {
            a_ever_strictly_better = true;
        } else if pt.score_b < pt.score_a - EPS {
            b_ever_strictly_better = true;
        }
    }

    match (a_ever_strictly_better, b_ever_strictly_better) {
        // A strictly better somewhere and never worse ⇒ A dominates.
        (true, false) => Some("a"),
        // B strictly better somewhere and never worse ⇒ B dominates.
        (false, true) => Some("b"),
        // Crossing (both better somewhere) or exactly equal (neither) ⇒ none.
        _ => None,
    }
}
