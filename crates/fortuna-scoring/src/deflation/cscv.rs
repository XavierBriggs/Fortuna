//! Probability of Backtest Overfitting via purged + embargoed CSCV (research
//! §1 + §2).
//!
//! Combinatorially Symmetric Cross-Validation (Bailey, Borwein, López de Prado,
//! Zhu 2017), made leak-proof by purging + embargo (López de Prado AFML ch.7):
//!
//! 1. Build the `T × N` matrix `M` — `matrix[t][n]` is config `n`'s
//!    forecasting-edge value on time-slice `t`.
//! 2. Partition the `T` rows into `S` even, disjoint, contiguous submatrices.
//! 3. For all `C(S, S/2)` ways of choosing which `S/2` submatrices form the IS
//!    (train) set (the rest are OOS/test):
//!    - **Purge + embargo** the IS rows against the OOS rows (drop IS rows whose
//!      label window overlaps any embargo-extended OOS window) — this is what
//!      stops the same-resolution-event leak from inflating IS performance.
//!    - Pick the IS-best config `n*` (highest mean IS edge).
//!    - Find `n*`'s OOS relative rank `ω̄_c = rank_{n*} / (N + 1) ∈ (0, 1)`
//!      (rank by mean OOS edge; rank 1 = worst, `N` = best, so high rank = good).
//!    - Logit `λ_c = ln(ω̄_c / (1 − ω̄_c))`. High λ ⇒ IS/OOS consistent.
//! 4. **PBO `φ` = fraction of combinations with `λ_c < 0`** = P(the IS-best
//!    config lands below the OOS median). Decision rule (gating layer): reject
//!    configs with `PBO > 0.05`.
//!
//! The procedure is **metric-agnostic** (verbatim, §1: "can be applied to any
//! performance evaluation metric") — it only ever ranks cells, so an
//! order-preserving transform of `M` (Brier-skill → Sharpe) gives an identical
//! PBO.
//!
//! Auxiliary outputs (§1): the **degradation slope** (OLS slope of OOS-best edge
//! on IS-best edge across combinations; `< 0` ⇒ overfit) and the **probability
//! of loss** (fraction of combinations whose OOS-best edge `< 0`).

use super::purge::{purge_embargo, Duration, LabelWindow};
use super::Matrix;
use serde::{Deserialize, Serialize};

/// Result of a purged+embargoed CSCV / PBO computation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PboReport {
    /// Probability of Backtest Overfitting `φ ∈ [0, 1]` — fraction of CSCV
    /// combinations whose logit `λ_c < 0`. `> 0.05` ⇒ overfit (gating layer).
    pub pbo: f64,
    /// OLS slope of the OOS-selected edge regressed on the IS-selected edge over
    /// all combinations. `< 0` ⇒ performance degrades out of sample (overfit).
    pub degradation_slope: f64,
    /// Fraction of combinations whose OOS-selected edge is `< 0` (probability of
    /// loss for the IS-best pick).
    pub prob_loss: f64,
    /// Number of logits actually computed (= number of valid CSCV combinations).
    pub n_logits: usize,
}

/// Purged + embargoed CSCV → [`PboReport`].
///
/// `matrix[t][n]` is config `n`'s edge on slice `t`. `s` is the (even) number of
/// CSCV submatrices. `label_windows[t]` is slice `t`'s label window (used for
/// purge+embargo); pass an **empty** slice to disable purging (the no-purge
/// baseline). `embargo` is the one-sided post-test embargo span.
///
/// Degenerate inputs are well-defined and never panic:
/// - empty matrix, or `n < 1` columns → `n_logits == 0`, `pbo == 0`.
/// - `s < 2`, `s` odd, or `s > T` → the largest even `s' ≤ min(s, T)` with
///   `s' ≥ 2` is used; if none exists `n_logits == 0`.
pub fn pbo(
    matrix: &Matrix,
    s: usize,
    label_windows: &[LabelWindow],
    embargo: Duration,
) -> PboReport {
    let t = matrix.len();
    let n = matrix.first().map(|r| r.len()).unwrap_or(0);

    let empty = PboReport {
        pbo: 0.0,
        degradation_slope: 0.0,
        prob_loss: 0.0,
        n_logits: 0,
    };

    if t == 0 || n == 0 {
        return empty;
    }

    // Coerce s to the largest even value in [2, min(s, t)].
    let s_eff = {
        let cap = s.min(t);
        cap - (cap % 2)
    };
    if s_eff < 2 {
        return empty;
    }

    // Partition rows into s_eff contiguous, near-even submatrices.
    let groups = partition_rows(t, s_eff);

    // All C(S, S/2) ways to choose which S/2 groups are IS.
    let combos = choose_half(s_eff);
    if combos.is_empty() {
        return empty;
    }

    let mut neg_logits = 0usize;
    let mut is_edges = Vec::with_capacity(combos.len());
    let mut oos_edges = Vec::with_capacity(combos.len());
    let mut loss_count = 0usize;
    let mut valid = 0usize;

    let have_windows = label_windows.len() == t;

    for combo in &combos {
        // Rows in the IS and OOS sets (in original order).
        let mut is_rows: Vec<usize> = Vec::new();
        let mut oos_rows: Vec<usize> = Vec::new();
        for (gi, grp) in groups.iter().enumerate() {
            if combo.contains(&gi) {
                is_rows.extend_from_slice(grp);
            } else {
                oos_rows.extend_from_slice(grp);
            }
        }

        // Purge + embargo: drop IS rows whose label window overlaps any
        // embargo-extended OOS window. Only applied when per-slice windows are
        // supplied (their count matches T); otherwise this is the no-purge path.
        if have_windows {
            let train: Vec<LabelWindow> = is_rows.iter().map(|&r| label_windows[r]).collect();
            let test: Vec<LabelWindow> = oos_rows.iter().map(|&r| label_windows[r]).collect();
            let keep = purge_embargo(&train, &test, embargo);
            is_rows = keep.into_iter().map(|k| is_rows[k]).collect();
        }

        if is_rows.is_empty() || oos_rows.is_empty() {
            // A fully-purged IS set can't pick a best config; skip the combo.
            continue;
        }

        // Per-config mean edge over the IS rows and the OOS rows.
        let is_mean = col_means(matrix, &is_rows, n);
        let oos_mean = col_means(matrix, &oos_rows, n);

        // IS-best config n* (highest IS mean edge).
        let n_star = argmax(&is_mean);

        // OOS relative rank of n*: rank 1 = lowest OOS mean, N = highest.
        let rank = oos_rank(&oos_mean, n_star); // in 1..=n
        let omega = rank as f64 / (n as f64 + 1.0); // (0,1)
        let lambda = (omega / (1.0 - omega)).ln();

        if lambda < 0.0 {
            neg_logits += 1;
        }
        is_edges.push(is_mean[n_star]);
        oos_edges.push(oos_mean[n_star]);
        if oos_mean[n_star] < 0.0 {
            loss_count += 1;
        }
        valid += 1;
    }

    if valid == 0 {
        return empty;
    }

    PboReport {
        pbo: neg_logits as f64 / valid as f64,
        degradation_slope: ols_slope(&is_edges, &oos_edges),
        prob_loss: loss_count as f64 / valid as f64,
        n_logits: valid,
    }
}

/// Partition `t` row indices into `s` contiguous, near-even groups (the first
/// `t % s` groups get one extra row). Preserves original row order within each
/// group, which is what keeps each group serially coherent.
fn partition_rows(t: usize, s: usize) -> Vec<Vec<usize>> {
    let base = t / s;
    let rem = t % s;
    let mut groups = Vec::with_capacity(s);
    let mut start = 0usize;
    for g in 0..s {
        let len = base + usize::from(g < rem);
        groups.push((start..start + len).collect());
        start += len;
    }
    groups
}

/// All combinations of `s/2` group-indices chosen from `0..s`.
fn choose_half(s: usize) -> Vec<Vec<usize>> {
    let k = s / 2;
    let mut out = Vec::new();
    let mut cur = Vec::with_capacity(k);
    fn rec(start: usize, s: usize, k: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        // Need (k - cur.len()) more from [start, s).
        let need = k - cur.len();
        if s - start < need {
            return;
        }
        for i in start..s {
            cur.push(i);
            rec(i + 1, s, k, cur, out);
            cur.pop();
        }
    }
    rec(0, s, k, &mut cur, &mut out);
    out
}

/// Per-config mean edge over the given row indices.
fn col_means(matrix: &Matrix, rows: &[usize], n: usize) -> Vec<f64> {
    let mut sums = vec![0.0f64; n];
    for &r in rows {
        for (c, v) in matrix[r].iter().enumerate() {
            sums[c] += v;
        }
    }
    let denom = rows.len() as f64;
    for s in sums.iter_mut() {
        *s /= denom;
    }
    sums
}

/// Index of the maximum value (first on ties).
fn argmax(v: &[f64]) -> usize {
    let mut best = 0usize;
    let mut best_v = f64::NEG_INFINITY;
    for (i, &x) in v.iter().enumerate() {
        if x > best_v {
            best_v = x;
            best = i;
        }
    }
    best
}

/// The 1-based rank of `target` in `oos_mean` by value: rank 1 = strictly fewer
/// values below it (lowest), rank `N` = highest. Ties share the count-below.
fn oos_rank(oos_mean: &[f64], target: usize) -> usize {
    let v = oos_mean[target];
    // count strictly-less values; rank = that count + 1.
    let below = oos_mean.iter().filter(|&&x| x < v).count();
    below + 1
}

/// OLS slope of `y` on `x` (`Σ(x−x̄)(y−ȳ) / Σ(x−x̄)²`). Returns 0 when `x` has no
/// variance (slope undefined) or fewer than two points.
fn ols_slope(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len();
    if n < 2 || n != y.len() {
        return 0.0;
    }
    let nf = n as f64;
    let xbar = x.iter().sum::<f64>() / nf;
    let ybar = y.iter().sum::<f64>() / nf;
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    for i in 0..n {
        let dx = x[i] - xbar;
        sxx += dx * dx;
        sxy += dx * (y[i] - ybar);
    }
    if sxx <= 1e-12 {
        0.0
    } else {
        sxy / sxx
    }
}
