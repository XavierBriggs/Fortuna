//! Scorecard aggregation (WS2 S6b; plan `2026-06-20-ws2-proof-layer.md` Task 6
//! Step 5).
//!
//! This module turns the resolved binary `(p, outcome)` samples + the de-vigged
//! market baseline + CLV for one `(scope, producer, window)` into a pure
//! [`fortuna_scoring::Scorecard`] via [`fortuna_scoring::assemble_scorecard`]. It
//! is the seam between the SAMPLE SOURCE and the pure scoring math.
//!
//! # Why this layer is pure (no ledger dependency)
//!
//! `fortuna-cognition` does not depend on `fortuna-ledger` (and must not — the
//! ledger dev-depends on cognition, and the plan's A7/decoupling constraint keeps
//! the scoring path off IO). So the aggregation core here is PURE: it takes the
//! already-gathered samples/baseline/CLV and returns a `Scorecard`. The
//! DB-walking that fills those vectors is the WS1 Brier-beats-baseline path
//! (`BeliefsRepo::forward_resolved_for_brier_baseline` →
//! `EdgesRepo::current_edges_for_event` → `SnapshotsRepo::latest_liquid_before`),
//! which already lives in the daemon (`run_weekly_review` in `fortuna-live`,
//! where the ledger handle and that exact loop already exist). Wiring the daemon
//! to call [`assemble_from_samples`] with that gathered set + persist via
//! `ScorecardsRepo` is the follow-on; this slice ships the pure core + the ledger
//! store + the endpoint.
//!
//! # The required core: the binary metric suite for the weather demo scope
//!
//! [`assemble_from_samples`] wires Brier (the gate metric) + its de-vigged-market
//! baseline, the Log score + tail-event count (from the binary samples, matching
//! the canonical [`fortuna_scoring::LogScoreRule`] convention), mean CLV, the CORP
//! reliability decomposition (inside `assemble_scorecard`), and the
//! Diebold–Mariano test of the model vs the market baseline. The
//! scalar/categorical metrics (`rps`/`crps`/`pit_bins`) are honest `None`/empty —
//! a binary scope carries no quantile ladder.
//!
//! `// follow-on`: (1) daemon wiring of the DB-gathered set → `assemble_from_samples`
//! → `ScorecardsRepo::insert_scorecard`; (2) scalar/categorical aggregation
//! (CRPS/PIT for scalar producer scopes, RPS for the weather ladder) when a
//! scalar-sample source is gathered — the pure rules already exist in
//! `fortuna-scoring`.

use fortuna_scoring::{
    assemble_scorecard, CalibrationSample, LogScoreRule, PredictiveDistribution, RealizedOutcome,
    Scorecard, ScoringRule,
};

/// Probability floor for the Log score / tail-event detection — the fixed
/// `1e-15` the [`fortuna_scoring::LogScoreRule`] uses (lower is better; never
/// tuned). A sample whose REALIZED-side probability is below this floor is a
/// "tail event": a confident-wrong forecast the floor rescues from `+∞`.
const LOG_SCORE_EPS: f64 = 1e-15;

/// The PURE aggregation core: assemble a [`Scorecard`] from already-gathered
/// binary samples, per-sample de-vigged-market baseline Brier losses, and CLV.
///
/// - `baseline_brier` is the MEAN of `baseline_losses` (empty → `0.0`, which with
///   no samples yields an `Insufficient` card — never a panic).
/// - the Log score + `log_tail_events` are computed from `samples` using the
///   `LogScoreRule` convention (so they match a per-belief Log score exactly).
/// - `baseline_losses` are forwarded to `assemble_scorecard` as the DM
///   comparison series; CLV is averaged to `clv_mean_bps` there.
/// - `rps`/`crps`/`pit_bins` are `None`/empty for a binary scope (see module
///   docs) — honest, not fabricated.
///
/// `scope`/`producer`/`window` are opaque labels passed straight through; the
/// card is identical apart from the `window` field for identical other inputs
/// (the source-agnostic seam WS3 reuses).
pub fn assemble_from_samples(
    scope: &str,
    producer: Option<&str>,
    window: &str,
    samples: &[(f64, bool)],
    baseline_losses: &[f64],
    clv: &[f64],
    min_n: u32,
) -> Scorecard {
    let calibration: Vec<CalibrationSample> = samples
        .iter()
        .map(|(p, outcome)| CalibrationSample {
            p: *p,
            outcome: *outcome,
        })
        .collect();

    let baseline_brier = if baseline_losses.is_empty() {
        0.0
    } else {
        baseline_losses.iter().sum::<f64>() / baseline_losses.len() as f64
    };

    let (log_score, log_tail_events) = log_score_and_tail(samples);

    assemble_scorecard(
        scope,
        producer,
        window,
        &calibration,
        baseline_brier,
        // DM model-vs-baseline only when a per-sample baseline series exists.
        if baseline_losses.is_empty() {
            None
        } else {
            Some(baseline_losses)
        },
        None, // rps — binary scope (follow-on: weather-ladder RPS)
        log_score,
        log_tail_events,
        None, // crps — needs a scalar-sample source (follow-on)
        clv,
        Vec::new(), // pit_bins — needs a scalar-sample source (follow-on)
        min_n,
    )
}

/// Mean Log score over the binary samples + the count of tail events.
///
/// The Log score mirrors [`fortuna_scoring::LogScoreRule`] exactly (clamp `p` to
/// `[EPS, 1−EPS]`, then `−ln(p)` if happened else `−ln(1−p)`), so the aggregate
/// equals the mean of the per-belief Log scores. A "tail event" is a sample whose
/// realized-side probability is `< EPS` BEFORE clamping (a confident-wrong miss).
/// Empty samples → `(None, 0)`.
fn log_score_and_tail(samples: &[(f64, bool)]) -> (Option<f64>, u32) {
    if samples.is_empty() {
        return (None, 0);
    }
    let rule = LogScoreRule;
    let mut sum = 0.0_f64;
    let mut tail = 0u32;
    for (p, outcome) in samples {
        // The realized-side probability before clamping; below EPS == tail event.
        let realized_p = if *outcome { *p } else { 1.0 - *p };
        if realized_p < LOG_SCORE_EPS {
            tail += 1;
        }
        // Reuse the canonical rule so the convention can never drift.
        match rule.score(
            &PredictiveDistribution::Binary { p: *p },
            &RealizedOutcome::Binary { happened: *outcome },
        ) {
            Ok(s) => sum += s,
            // A Binary/Binary pair never violates the rule's guards; on the
            // impossible error path, fall back to the floored loss directly so
            // the aggregate stays finite (no panic, no silent skew).
            Err(_) => {
                let clamped = realized_p.clamp(LOG_SCORE_EPS, 1.0 - LOG_SCORE_EPS);
                sum += -clamped.ln();
            }
        }
    }
    (Some(sum / samples.len() as f64), tail)
}
