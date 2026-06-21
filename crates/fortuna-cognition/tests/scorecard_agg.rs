//! WS2 S6b tests: the scorecard AGGREGATION pure core (plan Task 6 Step 5).
//!
//! `assemble_from_samples` is the pure, DB-free heart of the aggregation: given a
//! scope/producer/window and the already-gathered forward-resolved
//! `(p, outcome)` samples + per-sample de-vigged-market baseline Brier losses +
//! CLV, it computes the binary metric core (Brier baseline = mean of the baseline
//! losses; Log score + tail-event count from the samples) and delegates to the
//! pure `fortuna_scoring::assemble_scorecard`. The DB-walking that fills these
//! vectors from the ledger (the WS1 `forward_resolved_for_brier_baseline` path)
//! lives in the daemon (`fortuna-live`, where the ledger handle + that exact loop
//! already exist) — a follow-on wiring; the math is proven here without a
//! database (the brief's "no DB needed for the assemble step" path).
//!
//! Written FROM the plan text BEFORE the implementation (TDD). Coverage,
//! adversarially (BVA + negative + the source-agnostic property):
//!   - GO path: model Brier strictly beats the baseline + n >= min_n -> Go, and
//!     baseline_brier == mean(baseline_losses), n == samples.len();
//!   - tie -> NoGo (strict `<`, matching review.rs and assemble_scorecard);
//!   - n < min_n -> Insufficient (boundary on min_n);
//!   - empty samples -> Insufficient, n == 0, no panic;
//!   - the Log score + tail-event count are wired from the binary samples (a
//!     confident-wrong sample is counted as a tail event);
//!   - CLV is averaged into clv_mean_bps;
//!   - source-agnostic seam: identical inputs under window="forward" vs
//!     "historical" yield identical cards apart from `window`.

use fortuna_cognition::scorecard_agg::assemble_from_samples;
use fortuna_scoring::GoDecision;

#[test]
fn go_when_model_beats_baseline_with_enough_trials() {
    // 8 well-calibrated samples (>= the DM MIN_N of 8 so the DM test attaches);
    // per-sample market baseline losses uniformly 0.25 (the model strictly wins).
    let samples = vec![
        (0.1, false),
        (0.2, false),
        (0.8, true),
        (0.9, true),
        (0.3, false),
        (0.7, true),
        (0.15, false),
        (0.85, true),
    ];
    let baseline_losses = vec![0.25; 8];
    let clv = vec![10.0, 20.0];
    let card = assemble_from_samples(
        "weather:KNYC",
        Some("aeolus"),
        "forward",
        &samples,
        &baseline_losses,
        &clv,
        3,
    );

    assert_eq!(
        card.go.decision,
        GoDecision::Go,
        "model beats baseline -> Go"
    );
    assert_eq!(card.n, 8);
    assert_eq!(card.scope, "weather:KNYC");
    assert_eq!(card.producer.as_deref(), Some("aeolus"));
    assert_eq!(card.window, "forward");
    // baseline_brier is the mean of the supplied per-sample baseline losses.
    assert!((card.brier_baseline - 0.25).abs() < 1e-12);
    // The model's mean Brier is well under 0.25 here.
    assert!(card.brier < 0.25, "model Brier {} < 0.25", card.brier);
    // CLV averaged.
    assert_eq!(card.clv_mean_bps, Some(15.0));
    // A DM test against the baseline losses is attached (n >= MIN_N, real
    // differential with variance — the model uniformly beats a constant baseline).
    assert!(
        card.dm_vs_baseline.is_some(),
        "DM vs baseline attached at n>=8"
    );
}

#[test]
fn tie_is_nogo_strict_less_than() {
    // Model Brier == baseline Brier exactly: a tie is NO-GO (strict `<`).
    // One sample p=0.5, outcome=true -> model loss 0.25; baseline loss 0.25.
    let samples = vec![(0.5, true)];
    let baseline_losses = vec![0.25];
    let card = assemble_from_samples("s", None, "forward", &samples, &baseline_losses, &[], 1);
    assert!(
        (card.brier - card.brier_baseline).abs() < 1e-12,
        "exact tie"
    );
    assert_eq!(card.go.decision, GoDecision::NoGo, "a tie is NO-GO");
}

#[test]
fn below_min_n_is_insufficient() {
    // 2 samples, min_n = 3 -> Insufficient regardless of how good the model is.
    let samples = vec![(0.9, true), (0.1, false)];
    let baseline_losses = vec![0.25, 0.25];
    let card = assemble_from_samples("s", None, "forward", &samples, &baseline_losses, &[], 3);
    assert_eq!(card.go.decision, GoDecision::Insufficient);
    assert_eq!(card.n, 2);
}

#[test]
fn empty_samples_is_insufficient_no_panic() {
    let card = assemble_from_samples("s", None, "forward", &[], &[], &[], 3);
    assert_eq!(card.n, 0);
    assert_eq!(card.go.decision, GoDecision::Insufficient);
    assert_eq!(card.clv_mean_bps, None);
}

#[test]
fn log_score_and_tail_events_are_wired_from_samples() {
    // A confident-WRONG sample (p=1.0 but outcome=false) is a tail event: the
    // realized side's probability (1 - p = 0) is below the 1e-15 floor.
    let samples = vec![(0.6, true), (1.0, false)];
    let baseline_losses = vec![0.25, 0.25];
    let card = assemble_from_samples("s", None, "forward", &samples, &baseline_losses, &[], 1);
    assert!(
        card.log_score.is_some(),
        "log score is computed from the samples"
    );
    assert_eq!(
        card.log_tail_events, 1,
        "the confident-wrong sample is one tail event"
    );
    // The log score is finite even though one sample was confident-wrong (floor).
    assert!(card.log_score.unwrap().is_finite());
}

#[test]
fn source_agnostic_window_seam() {
    // Identical inputs under two windows -> identical cards apart from `window`.
    let samples = vec![(0.2, false), (0.8, true), (0.7, true)];
    let baseline = vec![0.25, 0.25, 0.25];
    let fwd = assemble_from_samples("s", Some("p"), "forward", &samples, &baseline, &[], 1);
    let hist = assemble_from_samples("s", Some("p"), "historical", &samples, &baseline, &[], 1);
    assert_ne!(fwd.window, hist.window);
    // Everything else is identical: copy the window across and compare.
    let mut hist_as_fwd = hist.clone();
    hist_as_fwd.window = fwd.window.clone();
    // The reasoning string is identical too (it never names the window).
    assert_eq!(
        fwd, hist_as_fwd,
        "cards are identical apart from the window label"
    );
}
