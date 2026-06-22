//! The source-agnostic [`Scorecard`] and its honest GO/NO-GO surface (spec 5.5,
//! §11; plan Task 6 / S6a).
//!
//! A `Scorecard` is the single immutable artifact that carries the full
//! research-backed metric suite for one `(scope, producer, window)` triple —
//! Brier (the gate metric) plus its baseline, the recorded Log/RPS/CRPS, the
//! tail-event count, mean CLV, the CORP reliability decomposition, the PIT
//! histogram, and the Diebold–Mariano test against the baseline — together with
//! an `go` surface that states the GO decision *and the whole truth behind it*.
//!
//! # The GO decision is Brier-beats-baseline, strict `<`
//!
//! Per the constitution (Brier stays the **sole** GO gate) and to match the WS1
//! gate at `review.rs:279` (which returns NoGo when `brier >= baseline`), the
//! decision is:
//!
//! - `Insufficient` if `n < min_n` (not enough trials to judge);
//! - else `Go` if `brier < baseline_brier` (strict — a **tie is NoGo**);
//! - else `NoGo`.
//!
//! RPS, Log, CORP, PIT, and DM are recorded and surfaced but **never** change
//! the decision. The CORP **MCB shown here is the in-sample diagnostic**; a
//! gated MCB would be cross-fit (out-of-sample), which is deferred to the gating
//! layer — the `reasoning` string says so explicitly so no reader mistakes the
//! in-sample MCB for a gated number.
//!
//! # Source-agnostic by construction
//!
//! `assemble_scorecard` reads `scope`/`producer`/`window` as opaque labels and
//! never branches on them; identical samples/inputs produce an identical
//! `Scorecard` apart from those labels. This is what lets WS3 reuse the exact
//! same assembly on a historical window without any cognition coupling.
//!
//! Pure: `std` + `serde` only. No panic — an empty sample set yields `n == 0`,
//! `Insufficient`, `corp == None`, and a finite reasoning string.

use crate::corp::{corp, Corp};
use crate::dm::{diebold_mariano, DmResult};
use crate::pit::PitBin;
use crate::samples::CalibrationSample;
use serde::{Deserialize, Serialize};

/// The GO/NO-GO verdict for a scorecard.
///
/// `Insufficient` means there were too few trials (`n < min_n`) to judge the
/// gate at all — it is distinct from `NoGo`, which is a judged failure to beat
/// the baseline.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoDecision {
    /// Brier strictly beat the baseline with enough trials.
    Go,
    /// Enough trials, but the Brier did not strictly beat the baseline (ties
    /// included — the gate is strict `<`).
    NoGo,
    /// Too few trials (`n < min_n`) to judge.
    Insufficient,
}

/// The GO surface: the verdict plus a whole-truth human-readable rationale.
///
/// `reasoning` names the trial count, the Brier-vs-baseline comparison, the Log
/// score and tail-event count, the CORP MCB/DSC/UNC (tagged as an in-sample
/// diagnostic, cross-fit deferred to gating), the DM p-value when available, and
/// the no-selection caveat — so a reader never has to trust the bare verdict.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GoSurface {
    /// The gate verdict.
    pub decision: GoDecision,
    /// Whole-truth rationale for the verdict.
    pub reasoning: String,
}

/// One immutable scorecard for a `(scope, producer, window)` triple.
///
/// All metrics beyond `brier`/`brier_baseline` are recorded for transparency and
/// never affect `go.decision`. `Option` fields are `None` when the metric does
/// not apply to this scope (e.g. `rps` for a binary scope) or could not be
/// computed (e.g. `corp` on an empty sample set).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Scorecard {
    /// Opaque scope label (e.g. a market/strategy scope key).
    pub scope: String,
    /// Opaque producer label, when one is attributed.
    pub producer: Option<String>,
    /// Opaque window label (e.g. `"forward"` / `"historical"`).
    pub window: String,
    /// Number of `(p, outcome)` samples the scorecard reduced over.
    pub n: u32,
    /// Mean Brier score of the model `(p − o)²`.
    pub brier: f64,
    /// Baseline Brier the gate is judged against.
    pub brier_baseline: f64,
    /// Ranked Probability Score (categorical scopes only).
    pub rps: Option<f64>,
    /// Mean logarithmic score, when computed.
    pub log_score: Option<f64>,
    /// Count of tail events `p < ε` contributing to the Log score.
    pub log_tail_events: u32,
    /// Continuous Ranked Probability Score (scalar scopes only).
    pub crps: Option<f64>,
    /// Mean closing-line value in basis points, when CLV linkage is available.
    pub clv_mean_bps: Option<f64>,
    /// CORP reliability decomposition (`None` on an empty sample set).
    pub corp: Option<Corp>,
    /// PIT histogram bins (empty when not applicable).
    pub pit_bins: Vec<PitBin>,
    /// Diebold–Mariano test of the model vs the baseline losses, when baseline
    /// per-sample losses were supplied and the series was long enough.
    pub dm_vs_baseline: Option<DmResult>,
    /// The GO surface (verdict + whole-truth reasoning).
    pub go: GoSurface,
}

/// Newey–West HAC lag for the DM test: the `⌊n^{1/3}⌋` rule of thumb, floored at
/// 1 so a HAC term is always considered. Deterministic in `n`.
fn dm_lag(n: usize) -> usize {
    ((n as f64).powf(1.0 / 3.0).floor() as usize).max(1)
}

/// Assemble a [`Scorecard`] from already-reduced inputs.
///
/// `samples` drives `brier`, `n`, the CORP decomposition, and the model side of
/// the DM test; `baseline_brier` is the gate's comparison point; the remaining
/// metrics (`rps`/`log_score`/`log_tail_events`/`crps`/`pit_bins`) are recorded
/// as supplied. `clv` is averaged to `clv_mean_bps` (or `None` when empty). When
/// `baseline_losses` is `Some`, a Diebold–Mariano test of the model's per-sample
/// Brier losses against those baseline losses is attached.
///
/// The GO decision is strict-`<` Brier-beats-baseline (see the module docs);
/// `min_n` is the trial-count floor below which the verdict is `Insufficient`.
///
/// Never panics: `n == 0` yields an `Insufficient` card with `corp == None` and a
/// finite reasoning string.
#[allow(clippy::too_many_arguments)]
pub fn assemble_scorecard(
    scope: &str,
    producer: Option<&str>,
    window: &str,
    samples: &[CalibrationSample],
    baseline_brier: f64,
    baseline_losses: Option<&[f64]>,
    rps: Option<f64>,
    log_score: Option<f64>,
    log_tail_events: u32,
    crps: Option<f64>,
    clv: &[f64],
    pit_bins: Vec<PitBin>,
    min_n: u32,
) -> Scorecard {
    let n = samples.len() as u32;

    // Per-sample Brier losses `(p − o)²` and their mean. An empty set has mean 0.
    let model_losses: Vec<f64> = samples
        .iter()
        .map(|s| {
            let o = if s.outcome { 1.0 } else { 0.0 };
            let d = s.p - o;
            d * d
        })
        .collect();
    let brier = if model_losses.is_empty() {
        0.0
    } else {
        model_losses.iter().sum::<f64>() / model_losses.len() as f64
    };

    let corp = corp(samples);

    // Diebold–Mariano model-vs-baseline on the per-sample Brier losses. The DM
    // guard handles short/degenerate series by returning None; mismatched
    // lengths likewise yield None.
    let dm_vs_baseline =
        baseline_losses.and_then(|b| diebold_mariano(&model_losses, b, dm_lag(model_losses.len())));

    let clv_mean_bps = if clv.is_empty() {
        None
    } else {
        Some(clv.iter().sum::<f64>() / clv.len() as f64)
    };

    // GO: Brier-beats-baseline, strict `<` (tie → NoGo), matching review.rs:279.
    let decision = if n < min_n {
        GoDecision::Insufficient
    } else if brier < baseline_brier {
        GoDecision::Go
    } else {
        GoDecision::NoGo
    };

    let reasoning = build_reasoning(
        &decision,
        n,
        min_n,
        brier,
        baseline_brier,
        log_score,
        log_tail_events,
        corp.as_ref(),
        dm_vs_baseline.as_ref(),
    );

    Scorecard {
        scope: scope.to_string(),
        producer: producer.map(str::to_string),
        window: window.to_string(),
        n,
        brier,
        brier_baseline: baseline_brier,
        rps,
        log_score,
        log_tail_events,
        crps,
        clv_mean_bps,
        corp,
        pit_bins,
        dm_vs_baseline,
        go: GoSurface {
            decision: decision.clone(),
            reasoning,
        },
    }
}

/// Build the whole-truth reasoning string for the GO surface.
///
/// Names: the verdict and trial count `n` (with `min_n` when insufficient); the
/// strict Brier-vs-`baseline` comparison; the Log score and `log_tail_events`;
/// the CORP `MCB`/DSC/UNC tagged as an **in-sample diagnostic (cross-fit
/// deferred to gating)**; the DM p-value when present; and the literal
/// no-selection caveat. No panic on any `None`.
#[allow(clippy::too_many_arguments)]
fn build_reasoning(
    decision: &GoDecision,
    n: u32,
    min_n: u32,
    brier: f64,
    baseline_brier: f64,
    log_score: Option<f64>,
    log_tail_events: u32,
    corp: Option<&Corp>,
    dm: Option<&DmResult>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Verdict + trial count.
    match decision {
        GoDecision::Go => parts.push(format!(
            "GO: n={n} trials; Brier {brier:.4} < baseline {baseline_brier:.4} (strict)"
        )),
        GoDecision::NoGo => parts.push(format!(
            "NO-GO: n={n} trials; Brier {brier:.4} does not beat baseline {baseline_brier:.4} (strict <, tie is NO-GO)"
        )),
        GoDecision::Insufficient => parts.push(format!(
            "INSUFFICIENT: n={n} trials < min_n {min_n}; Brier {brier:.4} vs baseline {baseline_brier:.4} not yet judged"
        )),
    }

    // Log score + tail-event count.
    match log_score {
        Some(ls) => parts.push(format!(
            "log score {ls:.4} over {log_tail_events} tail event(s) (p<ε)"
        )),
        None => parts.push(format!(
            "log score n/a; {log_tail_events} tail event(s) (p<ε)"
        )),
    }

    // CORP decomposition — explicitly the in-sample diagnostic.
    match corp {
        Some(c) => parts.push(format!(
            "CORP MCB {:.4} / DSC {:.4} / UNC {:.4} (in-sample diagnostic, cross-fit deferred to gating)",
            c.mcb, c.dsc, c.unc
        )),
        None => parts.push(
            "CORP MCB n/a (in-sample diagnostic, cross-fit deferred to gating)".to_string(),
        ),
    }

    // Diebold–Mariano significance vs baseline, when available.
    if let Some(d) = dm {
        parts.push(format!(
            "DM vs baseline p-value {:.4} (stat {:.3}, n={})",
            d.p_value, d.stat, d.n
        ));
    }

    // No-selection caveat (a single forward window — no multiple-testing).
    parts.push("single forward window, no selection (PBO N/A — WS3)".to_string());

    parts.join("; ")
}

/// The deflated G-TRUTH view of one selected sweep config (spec §7; plan S5,
/// BLOCK-1).
///
/// This is the *whole-truth* surface the [`decide`] gate reads — never a lone
/// flattering number. **Brier is the PRIMARY gated metric** (consistent with the
/// WS2 scorecard's "Brier is the sole GO gate"): the GO decision is driven by the
/// Brier-skill axis (`brier_edge`/`brier_pbo`/`brier_spa_p`). The CLV axis
/// (`clv_edge`/`clv_pbo`/`clv_spa_p`) is **corroborating only** — it is carried so
/// a reader sees the full picture, but it can never CREATE a GO (CLV's benchmark
/// is a market price, not ground truth). `mintrl_ok`/`sharpe_dsr` are walled-off
/// supporting context.
///
/// Pure: this struct and [`decide`] hold no IO and never branch on a scope or
/// producer label, so WS3's sweep and the live gating layer share one verdict
/// function.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DeflatedView {
    /// Effective independent sample size after purging (`N_eff`). The GO floor is
    /// 30; below it the verdict is `Insufficient`.
    pub effective_n: f64,
    /// Number of valid CSCV combinations the PBO was computed over. `0` means no
    /// coherent partition existed — the `pbo == 0.0` degenerate sentinel — so the
    /// PBO cannot be read at all (verdict `Insufficient`, never a GO).
    pub n_logits: usize,
    /// The significance level the Brier SPA p-value is gated against (strict `<`).
    pub alpha: f64,
    /// The selected config's Brier-skill (beats-baseline margin) OOS edge. The
    /// PRIMARY gated metric; must be strictly `> 0` for a GO.
    pub brier_edge: f64,
    /// PBO computed on the Brier-skill matrix. Must be `<= 0.05` for a GO.
    pub brier_pbo: f64,
    /// Hansen SPA `p_c` on the Brier-loss differential. Must be `< alpha` for a GO.
    pub brier_spa_p: f64,
    /// The CLV OOS edge (corroborating only — never gates).
    pub clv_edge: f64,
    /// PBO on the CLV matrix (corroborating only).
    pub clv_pbo: f64,
    /// SPA `p_c` on the CLV differential (corroborating only).
    pub clv_spa_p: f64,
    /// Whether `effective_n >= MinTRL` for the CSCV partition (supporting context).
    pub mintrl_ok: bool,
    /// Deflated Sharpe Ratio of the paper-trade PnL (walled-off context; not the
    /// edge claim).
    pub sharpe_dsr: f64,
}

/// The minimum effective independent sample size below which the deflated verdict
/// is `Insufficient` (matches WS2's `n < min_n -> Insufficient` posture and the
/// research §3 MinTRL floor).
pub const MIN_EFFECTIVE_N: f64 = 30.0;

/// The deflated GO gate (BLOCK-1; spec §7).
///
/// Reuses the WS2 [`GoDecision`] — there is no fourth verdict. The decision is:
///
/// - **`Insufficient`** iff under-powered: `effective_n < MIN_EFFECTIVE_N`, OR
///   `n_logits == 0` (no valid CSCV partition — the `pbo == 0.0` degenerate
///   sentinel, which alone points GO-direction and must never be read as a pass).
/// - else **`Go`** iff ALL of: the Brier-skill edge is strictly positive AND the
///   Brier PBO is `<= 0.05` AND the Brier SPA `p_c` is strictly `< alpha`.
/// - else **`NoGo`**.
///
/// **CLV never enters.** A CLV-positive but Brier-flat config is `NoGo`, never
/// `Go` — CLV corroborates, it cannot rescue (BLOCK-1).
pub fn decide(view: &DeflatedView) -> GoDecision {
    // Under-powered: too few independent observations, OR no coherent CSCV
    // partition (n_logits == 0 -> the degenerate pbo == 0.0 sentinel). Read this
    // FIRST so the pbo == 0.0 footgun can never be mistaken for "no overfitting".
    if view.effective_n < MIN_EFFECTIVE_N || view.n_logits == 0 {
        return GoDecision::Insufficient;
    }

    // Brier is the PRIMARY gated metric. GO iff every Brier conjunct holds. CLV is
    // deliberately absent from this condition.
    let brier_go = view.brier_edge > 0.0 && view.brier_pbo <= 0.05 && view.brier_spa_p < view.alpha;

    if brier_go {
        GoDecision::Go
    } else {
        GoDecision::NoGo
    }
}
