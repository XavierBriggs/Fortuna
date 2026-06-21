//! Murphy-diagram dominance + Diebold–Mariano tests — written BEFORE the
//! implementation per the TDD doctrine (plan Task 5/S5, V&V-2 [Important · S5]
//! + [M4 · S5]).
//!
//! Under test:
//! - `murphy_curve` / `dominates`: the cost-loss elementary score
//!   `S_θ(p,o) = θ·(1−o)·𝟙{p>θ} + (1−θ)·o·𝟙{p≤θ}` swept over an interior
//!   θ-grid in (0,1); `dominates` reports the forecaster whose mean elementary
//!   score is ≤ the other's at EVERY grid point (with ≥1 strict), else `None`.
//! - `diebold_mariano`: the DM statistic `d̄ / sqrt(HAC_var/n)` on the loss
//!   differential with a Newey–West HAC variance, a two-sided normal p-value,
//!   and the `n < 8` / zero-variance guards.
//!
//! Assertions are black-box: they pin the CONTRACT (which curve dominates, the
//! sign/magnitude of the stat, the significance verdict, the `None` guards),
//! never the internal sweep or variance accumulation.

use fortuna_scoring::{diebold_mariano, dominates, murphy_curve, CalibrationSample};

// ─── helpers ────────────────────────────────────────────────────────────────

fn samples(pairs: &[(f64, bool)]) -> Vec<CalibrationSample> {
    pairs
        .iter()
        .map(|(p, o)| CalibrationSample { p: *p, outcome: *o })
        .collect()
}

// ─── Murphy diagram ───────────────────────────────────────────────────────────

/// Forecaster A is uniformly closer to the truth than B on every sample, so its
/// mean cost-loss is ≤ B's at every θ and strictly less somewhere → A dominates.
#[test]
fn murphy_strict_dominance() {
    // Three events happened, three did not. A puts probability hard on the right
    // side every time; B hedges to the wrong side of 0.5 on every sample.
    let a = samples(&[
        (0.95, true),
        (0.92, true),
        (0.90, true),
        (0.05, false),
        (0.08, false),
        (0.10, false),
    ]);
    let b = samples(&[
        (0.40, true),
        (0.45, true),
        (0.30, true),
        (0.60, false),
        (0.55, false),
        (0.70, false),
    ]);

    let curve = murphy_curve(&a, &b, 19);
    assert_eq!(curve.len(), 19, "grid count");
    // Every interior θ is in (0,1) and ascending.
    assert!(curve.iter().all(|pt| pt.theta > 0.0 && pt.theta < 1.0));
    assert!(curve.windows(2).all(|w| w[1].theta > w[0].theta));
    // A is at least as good everywhere (the dominance contract the diagram makes).
    assert!(
        curve.iter().all(|pt| pt.score_a <= pt.score_b + 1e-12),
        "A should be ≤ B at every θ"
    );

    assert_eq!(dominates(&curve), Some("a"));
}

/// The symmetric case: swap the roles so B is the uniformly-better forecaster.
#[test]
fn murphy_strict_dominance_b() {
    let worse = samples(&[
        (0.40, true),
        (0.45, true),
        (0.30, true),
        (0.60, false),
        (0.55, false),
        (0.70, false),
    ]);
    let better = samples(&[
        (0.95, true),
        (0.92, true),
        (0.90, true),
        (0.05, false),
        (0.08, false),
        (0.10, false),
    ]);

    // a = worse, b = better ⇒ the second curve dominates.
    let curve = murphy_curve(&worse, &better, 19);
    assert_eq!(dominates(&curve), Some("b"));
}

/// Two forecasters whose mean cost-loss curves cross: B is cheaper for users
/// with a low cost/loss ratio (small θ), A is cheaper for high-θ users. Neither
/// dominates → `None`.
#[test]
fn murphy_crossing_is_none() {
    // Asymmetric base rate (the event happens 4/5 of the time). A is confident
    // on the majority class (good for high-θ users who act readily) but takes a
    // false positive on the rare non-event; B hedges the majority and is
    // overconfident on the rare class — so B wins for low-θ users and A wins for
    // high-θ users, and the curves cross (verified by the assertion below).
    let a = samples(&[
        (0.9, true),
        (0.9, true),
        (0.9, true),
        (0.2, false),
        (0.8, false),
    ]);
    let b = samples(&[
        (0.6, true),
        (0.6, true),
        (0.6, true),
        (0.05, false),
        (0.95, true),
    ]);

    let curve = murphy_curve(&a, &b, 49);
    let some_a_better = curve.iter().any(|pt| pt.score_a < pt.score_b - 1e-12);
    let some_b_better = curve.iter().any(|pt| pt.score_b < pt.score_a - 1e-12);
    assert!(
        some_a_better && some_b_better,
        "test setup must actually cross: a_better={some_a_better} b_better={some_b_better}"
    );

    assert_eq!(dominates(&curve), None);
}

/// Negative/degenerate: an empty curve cannot establish dominance → `None` (and
/// no panic).
#[test]
fn murphy_dominates_empty_is_none() {
    assert_eq!(dominates(&[]), None);
    // An empty sample set still yields a grid (all scores 0.0); equal curves are
    // a non-dominance → None.
    let curve = murphy_curve(&[], &[], 11);
    assert_eq!(curve.len(), 11);
    assert_eq!(dominates(&curve), None);
}

// ─── Diebold–Mariano ──────────────────────────────────────────────────────────

/// Identical losses ⇒ a zero differential ⇒ zero HAC variance ⇒ the guard
/// returns `None` (per V&V-2: NOT a stat≈0 result — the variance is genuinely
/// zero, so the statistic is undefined).
#[test]
fn dm_identical_returns_none() {
    let loss: Vec<f64> = (0..32).map(|i| 0.3 + (i as f64) * 0.001).collect();
    assert_eq!(diebold_mariano(&loss, &loss, 4), None);
}

/// A tiny NOISY differential with mean ≈ 0 and genuine (small) variance: the
/// statistic is small and the two-sided p-value is high (not significant). This
/// exercises the stat≈0 path that the identical case can no longer reach.
#[test]
fn dm_near_zero_noisy_small_stat() {
    // A symmetric ±0.01 differential about zero: d̄ ≈ 0, nonzero variance.
    let n = 40;
    let loss_b: Vec<f64> = (0..n).map(|i| 0.5 + (i as f64) * 0.002).collect();
    let loss_a: Vec<f64> = loss_b
        .iter()
        .enumerate()
        .map(|(i, &b)| if i % 2 == 0 { b + 0.01 } else { b - 0.01 })
        .collect();

    let r = diebold_mariano(&loss_a, &loss_b, 4).expect("nonzero variance ⇒ Some");
    assert_eq!(r.n, n);
    assert!(
        r.stat.abs() < 1.0,
        "near-zero mean ⇒ small |stat|, got {}",
        r.stat
    );
    assert!(
        r.p_value > 0.05,
        "near-zero differential is not significant, got p={}",
        r.p_value
    );
    assert!(
        (0.0..=1.0).contains(&r.p_value),
        "p in [0,1], got {}",
        r.p_value
    );
}

/// A differential where A genuinely beats B on average (d ≈ −0.5) WITH real
/// variance: the statistic is large in magnitude and the p-value is significant.
#[test]
fn dm_clear_winner_significant() {
    // d_t = −0.5 + small deterministic wobble ⇒ d̄ ≈ −0.5, small nonzero var.
    let n = 40;
    let loss_b: Vec<f64> = (0..n).map(|_| 1.0).collect();
    let loss_a: Vec<f64> = (0..n)
        .map(|i| {
            let wobble = if i % 2 == 0 { 0.02 } else { -0.02 };
            0.5 + wobble // ⇒ d = loss_a − loss_b = −0.5 ± 0.02
        })
        .collect();

    let r = diebold_mariano(&loss_a, &loss_b, 4).expect("nonzero variance ⇒ Some");
    assert_eq!(r.n, n);
    assert!(r.stat < 0.0, "A beats B ⇒ negative stat, got {}", r.stat);
    assert!(
        r.stat.abs() > 3.0,
        "clear winner ⇒ large |stat|, got {}",
        r.stat
    );
    assert!(
        r.p_value < 0.05,
        "clear winner is significant, got p={}",
        r.p_value
    );
}

/// Boundary: `n < 8` is too short for a meaningful HAC variance → `None`.
#[test]
fn dm_too_short_is_none() {
    let loss_a: Vec<f64> = vec![0.1, 0.9, 0.2, 0.8, 0.3, 0.7, 0.4]; // n = 7
    let loss_b: Vec<f64> = vec![0.9, 0.1, 0.8, 0.2, 0.7, 0.3, 0.6];
    assert_eq!(loss_a.len(), 7);
    assert_eq!(diebold_mariano(&loss_a, &loss_b, 3), None);
}

/// Boundary: exactly `n == 8` with genuine variance is admitted (the guard is
/// `< 8`, not `<= 8`).
#[test]
fn dm_n_eight_is_some() {
    let loss_a: Vec<f64> = vec![0.10, 0.90, 0.20, 0.80, 0.30, 0.70, 0.40, 0.60];
    let loss_b: Vec<f64> = vec![0.90, 0.10, 0.80, 0.20, 0.70, 0.30, 0.60, 0.40];
    let r = diebold_mariano(&loss_a, &loss_b, 2).expect("n==8 with variance ⇒ Some");
    assert_eq!(r.n, 8);
}

/// Negative: mismatched lengths cannot form per-observation differentials →
/// `None` (no panic, no silent truncation).
#[test]
fn dm_length_mismatch_is_none() {
    let loss_a: Vec<f64> = (0..10).map(|i| i as f64 * 0.1).collect();
    let loss_b: Vec<f64> = (0..9).map(|i| i as f64 * 0.1).collect();
    assert_eq!(diebold_mariano(&loss_a, &loss_b, 3), None);
}
