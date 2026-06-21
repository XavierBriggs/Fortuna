//! Tests for the pure source-agnostic [`Scorecard`] contract and its honest
//! GO/NO-GO surface (plan Task 6 / S6a; V&V-2 [Guardian-1] strict `<` + tie→NoGo,
//! [Imp-4] concrete G-TRUTH reasoning assertion).
//!
//! Black-box: every assertion is against the public `assemble_scorecard` return
//! and its `serde_json` shape — never against private state. The GO decision is
//! exercised at the strict-`<` boundary (below/at/above baseline) to match the
//! WS1 gate at `review.rs:279` (NoGo when `brier >= baseline`).

use fortuna_scoring::samples::CalibrationSample;
use fortuna_scoring::scorecard::{assemble_scorecard, GoDecision, Scorecard};

/// Build `n` binary samples whose mean Brier is well below a 0.25 baseline:
/// confident-and-correct on both classes (p=0.1 for misses, p=0.9 for hits).
/// Each sample contributes `(0.1)^2 = 0.01` to the Brier mean, so mean ≈ 0.01.
fn strong_samples(n: usize) -> Vec<CalibrationSample> {
    (0..n)
        .map(|i| {
            if i % 2 == 0 {
                CalibrationSample {
                    p: 0.9,
                    outcome: true,
                }
            } else {
                CalibrationSample {
                    p: 0.1,
                    outcome: false,
                }
            }
        })
        .collect()
}

/// Samples whose mean Brier is EXACTLY a given target by making every sample a
/// `p` vs `outcome=false` pair, so each loss is `p^2`. With `p = sqrt(target)`
/// the mean Brier equals `target` exactly (all identical terms).
fn samples_with_exact_brier(n: usize, target_brier: f64) -> Vec<CalibrationSample> {
    let p = target_brier.sqrt();
    (0..n)
        .map(|_| CalibrationSample { p, outcome: false })
        .collect()
}

#[test]
fn scorecard_go_strict_lt() {
    // ~40 samples, mean brier (~0.01) strictly < baseline (0.25), n >= min_n.
    let samples = strong_samples(40);
    let card = assemble_scorecard(
        "weather:knyc:high",
        Some("aeolus"),
        "forward",
        &samples,
        0.25,                  // baseline_brier
        None,                  // no baseline losses
        None,                  // rps
        Some(-(0.9_f64).ln()), // log_score (illustrative)
        0,                     // log_tail_events
        None,                  // crps
        &[],                   // clv
        Vec::new(),
        30, // min_n
    );

    assert_eq!(card.go.decision, GoDecision::Go, "brier < baseline must Go");
    assert_eq!(card.n, 40, "n must equal samples.len()");
    assert!(
        card.corp.is_some(),
        "corp must be computed for non-empty samples"
    );
    assert!(
        card.brier < card.brier_baseline,
        "the GO decision must rest on a genuine brier < baseline"
    );
}

#[test]
fn scorecard_tie_is_nogo() {
    // brier == baseline EXACTLY → NoGo (strict `<`, matching WS1 review.rs:279
    // which returns NoGo on `b >= mb`). This is the load-bearing boundary.
    let target = 0.16;
    let samples = samples_with_exact_brier(40, target);
    let card = assemble_scorecard(
        "scope",
        None,
        "forward",
        &samples,
        target, // baseline equals the model's brier exactly
        None,
        None,
        None,
        0,
        None,
        &[],
        Vec::new(),
        30,
    );

    assert!(
        (card.brier - card.brier_baseline).abs() < 1e-12,
        "precondition: this test must exercise the exact tie (brier == baseline)"
    );
    assert_eq!(
        card.go.decision,
        GoDecision::NoGo,
        "a tie must be NoGo under strict `<`"
    );
}

#[test]
fn scorecard_insufficient_below_min_n() {
    // n < min_n → Insufficient, regardless of how good the brier is.
    let samples = strong_samples(5);
    let card = assemble_scorecard(
        "scope",
        None,
        "forward",
        &samples,
        0.25,
        None,
        None,
        None,
        0,
        None,
        &[],
        Vec::new(),
        30, // min_n far above n
    );

    assert_eq!(
        card.go.decision,
        GoDecision::Insufficient,
        "n below min_n must be Insufficient even with an excellent brier"
    );
    assert_eq!(card.n, 5);
}

#[test]
fn scorecard_empty_is_insufficient_no_panic() {
    // n == 0 must NOT panic and must be Insufficient with a finite reasoning.
    let card = assemble_scorecard(
        "scope",
        None,
        "forward",
        &[],
        0.25,
        None,
        None,
        None,
        0,
        None,
        &[],
        Vec::new(),
        30,
    );
    assert_eq!(card.n, 0);
    assert_eq!(card.go.decision, GoDecision::Insufficient);
    assert!(card.corp.is_none(), "empty samples → no corp");
    assert!(
        !card.go.reasoning.is_empty(),
        "reasoning must be a finite, non-empty string even at n=0"
    );
}

#[test]
fn scorecard_reasoning_whole_truth() {
    // G-TRUTH: the reasoning string must NAME the trial count, the baseline, the
    // in-sample MCB diagnostic, and the PBO-N/A selection caveat.
    let samples = strong_samples(40);
    let baseline_losses: Vec<f64> = (0..40)
        .map(|i| if i % 2 == 0 { 0.04 } else { 0.05 })
        .collect();
    let card = assemble_scorecard(
        "weather:knyc:high",
        Some("aeolus"),
        "forward",
        &samples,
        0.25,
        Some(&baseline_losses),
        None,
        Some(-(0.9_f64).ln()),
        3, // log_tail_events
        None,
        &[12.0, 8.0, 10.0],
        Vec::new(),
        30,
    );

    let r = &card.go.reasoning;
    assert!(
        r.contains(&card.n.to_string()),
        "reasoning must name the trial count n; got: {r}"
    );
    assert!(
        r.contains("baseline"),
        "reasoning must name the baseline; got: {r}"
    );
    assert!(
        r.contains("MCB"),
        "reasoning must name the CORP MCB diagnostic; got: {r}"
    );
    assert!(
        r.contains("PBO N/A"),
        "reasoning must carry the no-selection PBO-N/A caveat; got: {r}"
    );
    // [advisory · S6] the in-sample MCB must be tagged as a diagnostic so no
    // reader mistakes it for a gated (cross-fit) number.
    assert!(
        r.contains("diagnostic"),
        "the in-sample MCB must be labelled a diagnostic; got: {r}"
    );
    // [Imp-4] the literal cross-fit-deferred phrasing must be present.
    assert!(
        r.contains("cross-fit deferred to gating"),
        "the diagnostic must say cross-fit is deferred to gating; got: {r}"
    );
}

#[test]
fn scorecard_rps_none_for_binary() {
    // A binary scope passes rps=None → the surfaced rps stays None.
    let samples = strong_samples(40);
    let card = assemble_scorecard(
        "scope",
        None,
        "forward",
        &samples,
        0.25,
        None,
        None, // rps None
        None,
        0,
        None,
        &[],
        Vec::new(),
        30,
    );
    assert_eq!(card.rps, None, "rps passed as None must surface as None");
}

#[test]
fn scorecard_clv_mean_when_present_else_none() {
    // clv non-empty → mean; empty → None. (BVA on the empty boundary.)
    let samples = strong_samples(40);
    let with_clv = assemble_scorecard(
        "scope",
        None,
        "forward",
        &samples,
        0.25,
        None,
        None,
        None,
        0,
        None,
        &[10.0, 20.0, 30.0],
        Vec::new(),
        30,
    );
    assert_eq!(
        with_clv.clv_mean_bps,
        Some(20.0),
        "clv_mean_bps must be the arithmetic mean of a non-empty clv slice"
    );

    let no_clv = assemble_scorecard(
        "scope",
        None,
        "forward",
        &samples,
        0.25,
        None,
        None,
        None,
        0,
        None,
        &[],
        Vec::new(),
        30,
    );
    assert_eq!(
        no_clv.clv_mean_bps, None,
        "an empty clv slice must surface as None, not 0.0"
    );
}

#[test]
fn scorecard_dm_present_when_baseline_losses_given() {
    // baseline_losses Some + enough samples → a DmResult is computed; absent → None.
    let samples = strong_samples(40);
    let baseline_losses: Vec<f64> = (0..40)
        .map(|i| if i % 2 == 0 { 0.04 } else { 0.05 })
        .collect();
    let with_dm = assemble_scorecard(
        "scope",
        None,
        "forward",
        &samples,
        0.25,
        Some(&baseline_losses),
        None,
        None,
        0,
        None,
        &[],
        Vec::new(),
        30,
    );
    assert!(
        with_dm.dm_vs_baseline.is_some(),
        "baseline_losses Some with n>=8 must yield a DM result"
    );

    let without = assemble_scorecard(
        "scope",
        None,
        "forward",
        &samples,
        0.25,
        None, // no baseline losses
        None,
        None,
        0,
        None,
        &[],
        Vec::new(),
        30,
    );
    assert_eq!(
        without.dm_vs_baseline, None,
        "no baseline_losses must mean no DM result"
    );
}

#[test]
fn scorecard_serialize_golden_shape() {
    // The serialized JSON object must have EXACTLY the contract's top-level keys.
    let samples = strong_samples(40);
    let card = assemble_scorecard(
        "weather:knyc:high",
        Some("aeolus"),
        "forward",
        &samples,
        0.25,
        None,
        None,
        Some(-(0.9_f64).ln()),
        0,
        None,
        &[],
        Vec::new(),
        30,
    );

    let v = serde_json::to_value(&card).expect("Scorecard must serialize");
    let obj = v
        .as_object()
        .expect("Scorecard serializes to a JSON object");

    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut expected = vec![
        "scope",
        "producer",
        "window",
        "n",
        "brier",
        "brier_baseline",
        "rps",
        "log_score",
        "log_tail_events",
        "crps",
        "clv_mean_bps",
        "corp",
        "pit_bins",
        "dm_vs_baseline",
        "go",
    ];
    expected.sort_unstable();
    assert_eq!(
        keys, expected,
        "top-level Scorecard keys must match the contract exactly"
    );

    // GoDecision must serialize snake_case ("go"/"no_go"/"insufficient").
    assert_eq!(
        v["go"]["decision"], "go",
        "GoDecision must serialize snake_case"
    );
}

#[test]
fn scorecard_serialize_nogo_snake_case() {
    // The NoGo variant must serialize as "no_go" (snake_case rename).
    let target = 0.16;
    let samples = samples_with_exact_brier(40, target);
    let card = assemble_scorecard(
        "scope",
        None,
        "forward",
        &samples,
        target,
        None,
        None,
        None,
        0,
        None,
        &[],
        Vec::new(),
        30,
    );
    let v = serde_json::to_value(&card).expect("serialize");
    assert_eq!(v["go"]["decision"], "no_go");
}

#[test]
fn scorecard_parity_seam() {
    // Source-agnosticism (WS3 reuse): the SAME inputs assembled with
    // window="forward" vs window="historical" must produce identical Scorecards
    // EXCEPT the `window` field. Proves the math never branches on the source.
    let samples = strong_samples(36);
    let baseline_losses: Vec<f64> = (0..36)
        .map(|i| if i % 2 == 0 { 0.04 } else { 0.05 })
        .collect();
    let clv = [11.0, 9.0, 13.0, 7.0];

    let mk = |window: &str| -> Scorecard {
        assemble_scorecard(
            "weather:knyc:high",
            Some("aeolus"),
            window,
            &samples,
            0.25,
            Some(&baseline_losses),
            Some(0.13),
            Some(-(0.85_f64).ln()),
            2,
            Some(1.4),
            &clv,
            Vec::new(),
            30,
        )
    };

    let mut fwd = mk("forward");
    let hist = mk("historical");

    assert_ne!(fwd.window, hist.window, "precondition: the windows differ");
    // Normalise the one allowed difference, then everything else must be equal.
    fwd.window = hist.window.clone();
    assert_eq!(
        fwd, hist,
        "Scorecard must be source-agnostic: identical apart from the window label"
    );
}
