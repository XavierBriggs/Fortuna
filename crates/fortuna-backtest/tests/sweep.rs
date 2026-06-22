//! WS3 S5 tests: the sweep driver + the G-TRUTH GO surface.
//!
//! Written FROM the plan text (S5) + the two V&V-fix bindings BEFORE the
//! implementation (TDD). The two load-bearing bindings these tests pin:
//!
//!   BLOCK-1 — **Brier is the PRIMARY gated metric.** `decide` returns `Go`
//!   only when N_eff is sufficient AND the Brier-skill OOS edge > 0 AND the
//!   Brier-skill PBO ≤ 0.05 AND the Brier-loss SPA p_c < α. CLV is a
//!   corroborating axis ONLY — a CLV-positive but Brier-flat config is NoGo,
//!   never Go. (`verdict_clv_cannot_rescue_brier_flat`.)
//!
//!   BLOCK-2 — **trial count N = the joint scope × config grid.** When K scopes
//!   are validated, `n_trials = |scopes| × |configs|` and DSR/SPA deflate
//!   against this `family_n_trials`, never one scope's config count.
//!   (`sweep_n_trials_counts_scope_x_config_grid`.)
//!
//! Plus the S4-verifier forward-notes: the `pbo == 0.0` footgun. An empty /
//! degenerate `PboReport` returns `pbo == 0.0` (alone GO-pointing); `decide`
//! MUST read `n_logits == 0` AND `effective_n < 30` as `Insufficient`, never as
//! "no overfitting / pass". (`verdict_insufficient_on_thin_n`.)

use fortuna_backtest::sweep::{
    run_sweep, ConfigEdges, RecalMethod, SweepParams, TrialSpace, ValidationRun,
};
use fortuna_scoring::deflation::PboReport;
use fortuna_scoring::{decide, DeflatedView, GoDecision};

// ---------------------------------------------------------------------------
// `decide` unit tests — BLOCK-1, the Brier-primary GO surface.
// ---------------------------------------------------------------------------

/// A baseline `DeflatedView` that *passes* every GO conjunct: N_eff well over
/// the floor, a real CSCV partition, a positive Brier edge, low Brier PBO, a
/// significant Brier SPA. CLV is flat/absent so the GO is driven purely by Brier.
fn passing_view() -> DeflatedView {
    DeflatedView {
        effective_n: 120.0,
        n_logits: 20,
        alpha: 0.05,
        brier_edge: 0.04,
        brier_pbo: 0.01,
        brier_spa_p: 0.01,
        clv_edge: 0.0,
        clv_pbo: 1.0,
        clv_spa_p: 1.0,
        mintrl_ok: true,
        sharpe_dsr: 0.99,
    }
}

#[test]
fn verdict_go_when_all_conjuncts_hold() {
    assert_eq!(decide(&passing_view()), GoDecision::Go);
}

#[test]
fn verdict_go_requires_all() {
    // Drop ANY ONE conjunct -> NOT Go (each conjunct is load-bearing, so a
    // mutation that removes a conjunct from the AND reds at least one of these).

    // 1. Brier edge not positive (exactly 0 is "flat", a tie is NOT a GO).
    let mut v = passing_view();
    v.brier_edge = 0.0;
    assert_ne!(decide(&v), GoDecision::Go, "brier_edge == 0 must not be GO");

    // 2. Brier PBO above the 0.05 ceiling.
    let mut v = passing_view();
    v.brier_pbo = 0.06;
    assert_ne!(
        decide(&v),
        GoDecision::Go,
        "brier_pbo > 0.05 must not be GO"
    );

    // 3. Brier SPA p_c not below alpha (a tie at alpha is NOT significant).
    let mut v = passing_view();
    v.brier_spa_p = 0.05;
    assert_ne!(
        decide(&v),
        GoDecision::Go,
        "brier_spa_p == alpha must not be GO"
    );

    // 4. N_eff below the 30 floor.
    let mut v = passing_view();
    v.effective_n = 29.0;
    assert_ne!(
        decide(&v),
        GoDecision::Go,
        "effective_n < 30 must not be GO"
    );
}

/// BLOCK-1: a config with strongly positive, significant, non-overfit CLV but a
/// Brier-skill edge that does NOT beat the baseline must be `NoGo` — CLV can
/// strengthen a read but can never CREATE a GO. A mutation that ORs CLV into the
/// GO condition reds this test (the view would then read GO).
#[test]
fn verdict_clv_cannot_rescue_brier_flat() {
    let mut v = passing_view();
    // Brier-skill is flat (no edge over baseline) — the gated metric fails.
    v.brier_edge = 0.0;
    v.brier_pbo = 0.5; // overfit on the Brier axis too
    v.brier_spa_p = 0.9; // not significant on the Brier axis
                         // CLV is pristine: large positive edge, no overfit, highly significant.
    v.clv_edge = 25.0;
    v.clv_pbo = 0.0;
    v.clv_spa_p = 0.0001;

    assert_eq!(
        decide(&v),
        GoDecision::NoGo,
        "a CLV-positive but Brier-flat config is NoGo, never Go (BLOCK-1)"
    );
}

/// The `pbo == 0.0` footgun (S4-verifier forward-note). A degenerate
/// `PboReport` has `pbo == 0.0` (which alone points GO-direction) AND
/// `n_logits == 0`. With a thin effective-N this must be `Insufficient`, NEVER
/// read as "no overfitting / pass" (Go) and NEVER demoted to a judged NoGo.
#[test]
fn verdict_insufficient_on_thin_n() {
    // Sanity: the degenerate PboReport really does carry pbo == 0.0.
    let degenerate = PboReport {
        pbo: 0.0,
        degradation_slope: 0.0,
        prob_loss: 0.0,
        n_logits: 0,
    };
    assert_eq!(degenerate.pbo, 0.0);

    // Thin N_eff alone -> Insufficient.
    let mut v = passing_view();
    v.effective_n = 12.0;
    assert_eq!(
        decide(&v),
        GoDecision::Insufficient,
        "effective_n < 30 is Insufficient, not NoGo and not Go"
    );

    // The degenerate-CSCV case: pbo == 0.0 but n_logits == 0. Even with a fat
    // N_eff, no valid CSCV partition means we cannot read the pbo at all.
    let mut v = passing_view();
    v.n_logits = degenerate.n_logits; // 0
    v.brier_pbo = degenerate.pbo; // 0.0 — the footgun value
    assert_eq!(
        decide(&v),
        GoDecision::Insufficient,
        "n_logits == 0 (pbo == 0.0 degenerate) is Insufficient, never a GO"
    );
}

// ---------------------------------------------------------------------------
// `run_sweep` integration tests — BLOCK-2 + whole-truth serialization.
// ---------------------------------------------------------------------------

/// A `TrialSpace` with `n_configs = |windows| × |methods| × |thresholds|`.
/// 2 × 2 × 2 = 8 configs per scope. `scopes` is set per test.
fn trial_space(scopes: Vec<String>) -> TrialSpace {
    TrialSpace {
        calibration_windows: vec![30, 60],
        recal_methods: vec![RecalMethod::Platt, RecalMethod::Isotonic],
        scopes,
        go_thresholds: vec![0.50, 0.55],
    }
}

/// A deterministic edge provider: every (scope, config) yields a fixed, modest
/// positive Brier-skill OOS series, a matching loss-differential series, a flat
/// CLV series, and trivial Sharpe inputs. Enough to drive a full sweep with a
/// well-defined selected config and verdict.
fn flat_edges(_scope: &str, _config_index: usize) -> ConfigEdges {
    // 40 slices of a small consistent positive Brier-skill edge.
    let brier_oos: Vec<f64> = (0..40).map(|_| 0.02).collect();
    let brier_loss_diff: Vec<f64> = (0..40).map(|_| 0.02).collect();
    let clv_oos: Vec<f64> = (0..40).map(|_| 0.0).collect();
    let sharpe_returns: Vec<f64> = (0..40).map(|i| 0.01 * (i as f64 % 3.0 - 1.0)).collect();
    ConfigEdges {
        brier_oos,
        brier_loss_diff,
        clv_oos,
        sharpe_returns,
    }
}

/// BLOCK-2: validating K scopes records `n_trials = K × |configs|` and the
/// DSR/SPA deflation uses this `family_n_trials`. A mutation that counts only one
/// scope's configs (e.g. `family_n_trials = n_configs`) reds this test.
#[test]
fn sweep_n_trials_counts_scope_x_config_grid() {
    let params = SweepParams::default();

    // One scope: 8 configs -> family_n_trials = 8.
    let one = run_sweep(&trial_space(vec!["s0".into()]), &params, flat_edges);
    assert_eq!(one.n_trials, 8, "1 scope × 8 configs = 8 configs explored");
    assert_eq!(
        one.family_n_trials, 8,
        "1 scope × 8 configs -> family_n_trials = 8"
    );

    // Three scopes: the SAME 8 configs each -> family_n_trials = 24, NOT 8.
    let three = run_sweep(
        &trial_space(vec!["s0".into(), "s1".into(), "s2".into()]),
        &params,
        flat_edges,
    );
    assert_eq!(
        three.family_n_trials, 24,
        "3 scopes × 8 configs -> family_n_trials = 24 (the JOINT grid, BLOCK-2)"
    );
    assert!(
        three.family_n_trials > one.family_n_trials,
        "adding scopes inflates the family trial count — a single-scope count would not"
    );
    assert_eq!(
        three.family_n_trials,
        3 * 8,
        "family_n_trials is |scopes| × |configs|, never a single scope's config count"
    );
}

/// The serialized GO surface carries the WHOLE truth — never a lone flattering
/// number. Every G-TRUTH column must round-trip through serde.
#[test]
fn go_surface_serializes_whole_truth() {
    let params = SweepParams::default();
    let run = run_sweep(
        &trial_space(vec!["s0".into(), "s1".into()]),
        &params,
        flat_edges,
    );

    let json = serde_json::to_value(&run).expect("ValidationRun serializes");
    for key in [
        "n_trials",
        "family_n_trials",
        "effective_n",
        "brier_edge",
        "brier_pbo",
        "brier_spa_p",
        "clv_edge",
        "clv_pbo",
        "clv_spa_p",
        "sharpe_dsr",
        "mintrl_ok",
        "verdict",
        "selected_config",
        "scope",
        "producer",
        "run_id",
        "computed_at",
        "trial_space",
    ] {
        assert!(
            json.get(key).is_some(),
            "the serialized GO surface must carry `{key}` — never a lone number"
        );
    }

    // The verdict round-trips back to a real GoDecision.
    let round: ValidationRun = serde_json::from_value(json).expect("round-trips");
    assert_eq!(round.verdict, run.verdict);
}

/// A sweep over a consistently-skilled edge produces a coherent run: a selected
/// config drawn from the explored grid, the Brier edge surfaced, and a verdict
/// that is one of the three legal states (never a fourth).
#[test]
fn sweep_produces_coherent_run() {
    let params = SweepParams::default();
    let run: ValidationRun = run_sweep(&trial_space(vec!["s0".into()]), &params, flat_edges);

    assert!(run.n_trials >= 1, "at least one config explored");
    assert!(
        matches!(
            run.verdict,
            GoDecision::Go | GoDecision::NoGo | GoDecision::Insufficient
        ),
        "the verdict is exactly one of the three existing GoDecision states"
    );
    // The Brier edge is the headline metric and is populated.
    assert!(run.brier_edge.is_finite(), "brier_edge is a real number");
}
