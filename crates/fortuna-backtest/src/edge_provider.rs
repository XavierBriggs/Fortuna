//! The real `validate` edge provider (W7 MONEY-MATH seam).
//!
//! ## What this connects
//!
//! `fortuna validate` enumerates the trial space and runs the deflation toolkit
//! (`pbo`, `spa_c`, `effective_n`, `dsr`) over a per-`(scope, config)` matrix of
//! out-of-sample edge series. WS3 shipped that machinery + a PLACEHOLDER provider
//! whose series were empty — so on real history `validate` could only ever emit
//! `Insufficient`-by-construction, and the implemented + mutation-proven
//! purged/embargoed CSCV had no reachable production path.
//!
//! [`LedgerEdgeProvider`] is the real seam. It assembles, for one scope, the
//! per-period OOS **Brier-skill** series (the gated headline) and the per-period
//! label windows (for purge/embargo) by:
//!
//! 1. Streaming the source's records through the SAME as-of join the harness uses
//!    ([`crate::asof::asof_join`]) — so the G-PIT guard drops every future-dated
//!    belief (`available_at >= decided_at`) BEFORE it can reach the scored series.
//! 2. Scoring each resolved sample through the SAME `fortuna-scoring` path the
//!    daemon + replay harness use
//!    (`fortuna_cognition::scorecard_agg::assemble_from_samples`) — never a
//!    reimplementation of Brier (G-PARITY by construction).
//! 3. Computing the per-period Brier-skill margin `baseline_loss − model_loss`
//!    (positive ⇒ the model beat the de-vigged market book on that period) and the
//!    per-period CLV.
//!
//! ## The length invariant (V&V — load-bearing)
//!
//! Every `(scope, config)` series this provider returns has the SAME length `t`
//! (the count of resolved, leak-free periods). [`LedgerEdgeProvider::windows`]
//! returns EXACTLY `t` label windows. `run_sweep` asserts `windows.len() == t`
//! before calling `pbo`, because `pbo` only purges when `label_windows.len() == T`
//! — a ragged or short window list would silently take the no-purge path and
//! UNDERSTATE overfitting.
//!
//! ## Configs
//!
//! The trial-space configs differ by recalibration method (and calibration-window
//! length). Each config applies its own deterministic recalibration to the raw
//! model probability, then scores the recalibrated probability through the SAME
//! scoring path — so different configs produce genuinely different OOS edges and
//! `run_sweep` selects the in-sample-best honestly. The transform NEVER touches
//! the ground-truth outcome or the baseline, and NEVER reads wall-clock time.
//!
//! ## Decoupling (a gate)
//!
//! This file carries NO source-name string literals (the names the decoupling
//! grep gate forbids outside `src/sources/`). It reads GENERIC resolved samples by
//! scope/config; the source stamp, when needed, is the ledger-side const
//! [`fortuna_ledger::SOURCE_HISTORICAL_IMPORT`] (no literal in this crate's
//! `src/`). The grep gate enforces this (`--exclude-dir=sources`).
//!
//! ## No `panic!` / `unwrap` / `expect`
//!
//! Construction returns a `Result`; the trait methods are total (they index into
//! the already-validated, rectangular sample set and never panic).

use crate::asof::{asof_join, AsOfDisposition};
use crate::records::BeliefPayload;
use crate::source::{HistoricalSource, SourceError};
use crate::sweep::{ConfigEdges, EdgeProvider};
use fortuna_core::clock::UtcTimestamp;
use fortuna_scoring::deflation::{Duration, LabelWindow};
use thiserror::Error;

/// The probability floor/ceiling for a de-vigged book price expressed as a
/// probability. A 0c or 100c book would imply a degenerate `0`/`1` baseline; we
/// clamp into `[ε, 1−ε]` so the baseline Brier loss is always finite and the
/// baseline never becomes a trivially-perfect oracle.
const P_EPS: f64 = 1e-6;

/// Errors building a [`LedgerEdgeProvider`].
#[derive(Debug, Error)]
pub enum EdgeProviderError {
    /// The source failed to yield a record stream.
    #[error("source error assembling edges: {0}")]
    Source(#[from] SourceError),
    /// A belief carried a payload the binary edge path cannot score (scalar
    /// payloads are a later slice, matching the S2 binary replay path).
    #[error("unsupported belief payload assembling edges: {0}")]
    UnsupportedPayload(String),
}

/// One resolved, leak-free period: the inputs to scoring a single observation.
///
/// `decided_at`/`label` order the period and drive purge/embargo; `p` is the raw
/// model probability (recalibrated per-config at edge time); `outcome` is the
/// ground-truth label; `baseline_p` is the de-vigged book-implied probability
/// (the Brier baseline); `clv_bps` is the closing-line value in basis points.
#[derive(Debug, Clone)]
struct ResolvedPeriod {
    decided_at: UtcTimestamp,
    label: LabelWindow,
    p: f64,
    outcome: bool,
    baseline_p: f64,
    clv_bps: f64,
}

/// Assembles the per-period OOS Brier-skill + CLV series for ONE scope from a
/// historical source, scoring through the SAME `fortuna-scoring` path as live.
///
/// Construct with [`LedgerEdgeProvider::from_source`]. The provider is pure +
/// deterministic: it holds the already-as-of-joined, leak-free resolved periods
/// (sorted by `decided_at`) and computes each config's series on demand.
pub struct LedgerEdgeProvider {
    scope: String,
    periods: Vec<ResolvedPeriod>,
    embargo: Duration,
}

impl LedgerEdgeProvider {
    /// Build the provider by streaming `source` over `range`, as-of-joining each
    /// belief (G-PIT: future-dated beliefs are dropped here, BEFORE scoring), and
    /// recording one [`ResolvedPeriod`] per resolved sample.
    ///
    /// The embargo is the source's per-row label-eval buffer; we use a one-day
    /// embargo (in the same integer-millis unit as the label windows) so a train
    /// row that starts within a day after a test row's resolution is also purged —
    /// the conservative same-event-family buffer (research §2).
    pub fn from_source(
        source: &impl HistoricalSource,
        range: crate::harness::TimeRange,
    ) -> Result<Self, EdgeProviderError> {
        let snapshots = drain(source.snapshots())?;
        let outcomes = drain(source.outcomes())?;

        let mut scope: Option<String> = None;
        let mut periods: Vec<ResolvedPeriod> = Vec::new();

        for belief in source.beliefs() {
            let belief = belief?;
            // Replay-window filter on decided_at (same as the harness): out-of-window
            // beliefs are not part of this validate pass.
            if belief.decided_at < range.from || belief.decided_at > range.to {
                continue;
            }

            // G-PIT — the SAME as-of join the harness uses. A future-dated belief
            // (`available_at >= decided_at`) is rejected here and never produces a
            // period, so it can NEVER reach the scored series.
            match asof_join(&belief, &snapshots, &outcomes) {
                AsOfDisposition::LookAheadRejected => {
                    // Dropped by G-PIT; not scored. (The harness counts these in
                    // ReplayReport::look_ahead_rejected; the series simply omits
                    // the leak.)
                }
                AsOfDisposition::Joined(ctx) => {
                    let ctx = *ctx;
                    if scope.is_none() {
                        scope = Some(ctx.belief.provenance.scope.clone());
                    }
                    let p = match &ctx.belief.payload {
                        BeliefPayload::Binary { p } => *p,
                        BeliefPayload::Scalar { .. } => {
                            return Err(EdgeProviderError::UnsupportedPayload(
                                "scalar belief payloads are not scored by the binary edge path"
                                    .to_string(),
                            ));
                        }
                    };

                    // Only RESOLVED periods contribute a scored row (an unresolved
                    // belief has no label to score against).
                    let outcome = match &ctx.outcome {
                        Some(o) => o,
                        None => continue,
                    };
                    let happened = outcome.outcome >= 0.5;

                    // The de-vigged book-implied probability: the CLV-entry
                    // snapshot price (cents) as a probability, clamped off the
                    // degenerate 0/1 boundary. When no prior snapshot exists the
                    // baseline is the uninformative 0.5 (a coin-flip book).
                    let baseline_p = match &ctx.snapshot {
                        Some(s) => clamp_p(s.price.raw() as f64 / 100.0),
                        None => 0.5,
                    };

                    // CLV in basis points: how much the entry price improved
                    // relative to the eventual settlement (1.0 or 0.0). Positive ⇒
                    // entered better than the close. Reported as a CORROBORATING
                    // axis only (never gates).
                    let clv_bps = match &ctx.snapshot {
                        Some(s) => {
                            let entry = clamp_p(s.price.raw() as f64 / 100.0);
                            let settle = if happened { 1.0 } else { 0.0 };
                            (settle - entry) * 10_000.0
                        }
                        None => 0.0,
                    };

                    // The label window for purge/embargo: [available_at, resolved_at]
                    // in epoch-millis. Same-event-family overlap (e.g. two brackets
                    // for the same station-day) → overlapping windows → purge drops
                    // the leaking train rows.
                    let label = LabelWindow::new(
                        ctx.belief.available_at.epoch_millis(),
                        outcome.resolved_at.epoch_millis(),
                    );

                    periods.push(ResolvedPeriod {
                        decided_at: ctx.belief.decided_at,
                        label,
                        p,
                        outcome: happened,
                        baseline_p,
                        clv_bps,
                    });
                }
            }
        }

        // Deterministic, time-ordered period sequence (independent of the source's
        // iteration order) so the per-row matrix + windows are reproducible.
        periods.sort_by(|a, b| {
            a.decided_at
                .epoch_millis()
                .cmp(&b.decided_at.epoch_millis())
                .then_with(|| a.label.t0.cmp(&b.label.t0))
                .then_with(|| a.label.t1.cmp(&b.label.t1))
        });

        Ok(LedgerEdgeProvider {
            scope: scope.unwrap_or_default(),
            periods,
            // One-day embargo in the label windows' integer-millis unit.
            embargo: Duration::from_millis(86_400_000),
        })
    }

    /// The number of resolved, leak-free periods `t` — the per-scope matrix has
    /// `t` rows and [`Self::windows`] returns exactly `t` windows.
    pub fn period_count(&self) -> usize {
        self.periods.len()
    }
}

impl EdgeProvider for LedgerEdgeProvider {
    /// The OOS edge series for `config_index` under `scope`.
    ///
    /// When `scope` does not match this provider's scope the series are empty (the
    /// provider is single-scope; an unknown scope has no track record). Otherwise
    /// each of the four series has length `t == period_count()`.
    fn edges(&self, scope: &str, config_index: usize) -> ConfigEdges {
        if scope != self.scope {
            return ConfigEdges {
                brier_oos: Vec::new(),
                brier_loss_diff: Vec::new(),
                clv_oos: Vec::new(),
                sharpe_returns: Vec::new(),
            };
        }

        let mut brier_oos = Vec::with_capacity(self.periods.len());
        let mut brier_loss_diff = Vec::with_capacity(self.periods.len());
        let mut clv_oos = Vec::with_capacity(self.periods.len());
        let mut sharpe_returns = Vec::with_capacity(self.periods.len());

        for period in &self.periods {
            // Apply this config's recalibration to the raw model probability, then
            // score the recalibrated probability through the SAME scoring path.
            let p_recal = recalibrate(period.p, config_index);

            // Per-sample model Brier loss via the SAME assembler the daemon +
            // replay harness use (single-sample → `.brier` is that loss exactly).
            // G-PARITY: never a reimplementation of (p−o)².
            let card = fortuna_cognition::scorecard_agg::assemble_from_samples(
                &self.scope,
                None,
                fortuna_ledger::SOURCE_HISTORICAL_IMPORT,
                &[(p_recal, period.outcome)],
                &[], // baseline handled per-row below (not via the card's mean)
                &[],
                1, // min_n: a single-sample card just exposes the per-row Brier
            );
            let model_loss = card.brier;

            // The de-vigged baseline's per-sample Brier loss `(baseline_p − o)²`.
            let o = if period.outcome { 1.0 } else { 0.0 };
            let d = period.baseline_p - o;
            let baseline_loss = d * d;

            // Brier-SKILL = beats-baseline margin (positive ⇒ model better OOS).
            brier_oos.push(baseline_loss - model_loss);
            // Brier-LOSS differential (baseline − model, i.e. loss_benchmark − loss_model)
            // for the SPA test; the SPA null is "the model is no better than the baseline".
            // SIGN NOTE: this is baseline − model (positive when model is better), NOT
            // model − baseline — code is correct per spa.rs conventions.
            brier_loss_diff.push(baseline_loss - model_loss);
            // CLV (corroborating only).
            clv_oos.push(period.clv_bps);
            // Paper-return proxy for the walled-off DSR context: the per-period
            // Brier-skill stands in for the realized edge (no real PnL is replayed
            // through this paper-only path — see HistoricalTrade's orders==0).
            sharpe_returns.push(baseline_loss - model_loss);
        }

        ConfigEdges {
            brier_oos,
            brier_loss_diff,
            clv_oos,
            sharpe_returns,
        }
    }

    /// The per-row label eval windows + embargo for purge/embargo.
    ///
    /// Returns EXACTLY `t == period_count()` windows (one per period), in the same
    /// `decided_at` order as the matrix rows, so `run_sweep`'s
    /// `windows.len() == matrix_row_count` assertion holds and `pbo` takes the
    /// PURGE path (not the silent no-purge no-op).
    fn windows(&self, scope: &str) -> (Vec<LabelWindow>, Duration) {
        if scope != self.scope {
            return (Vec::new(), Duration::zero());
        }
        let windows = self.periods.iter().map(|p| p.label).collect();
        (windows, self.embargo)
    }
}

/// Apply config `config_index`'s recalibration to a raw model probability.
///
/// Each config applies a distinct **temperature scaling** of the model logit —
/// the canonical one-parameter recalibration family (Platt/temperature scaling):
///
/// ```text
/// p' = σ( logit(p) / τ_c ),   τ_c = TAU_BASE · (1 + TAU_STEP · config_index)
/// ```
///
/// `τ = 1` is the identity (raw probabilities); `τ > 1` shrinks toward 0.5
/// (under-confident); `τ < 1` sharpens away from 0.5 (over-confident). Spreading
/// `τ_c` across configs gives the trial space genuinely DIFFERENT OOS edge columns
/// — so `run_sweep`'s in-sample selection + the CSCV ranks are non-degenerate —
/// while every config remains an honest recalibration of the SAME raw model
/// probability. The transform never touches the outcome or baseline and reads no
/// wall-clock time.
///
/// The result is clamped off the degenerate `0`/`1` boundary into `[ε, 1−ε]`.
fn recalibrate(p: f64, config_index: usize) -> f64 {
    // A spread of temperatures starting below 1 (sharpen) through above 1 (shrink)
    // so the configs are genuinely distinct and ordered. config 0 → τ≈0.7
    // (slightly over-confident), rising by 0.3 per config.
    const TAU_BASE: f64 = 0.7;
    const TAU_STEP: f64 = 0.3;
    let tau = TAU_BASE + TAU_STEP * config_index as f64;
    let pc = clamp_p(p);
    let logit = (pc / (1.0 - pc)).ln();
    let scaled = logit / tau;
    // σ(scaled) — numerically stable for the clamped, finite logit range here.
    let p_recal = 1.0 / (1.0 + (-scaled).exp());
    clamp_p(p_recal)
}

/// Clamp a probability off the degenerate `0`/`1` boundary into `[ε, 1−ε]`.
fn clamp_p(p: f64) -> f64 {
    p.clamp(P_EPS, 1.0 - P_EPS)
}

/// Drain a source iterator into a Vec, surfacing the first error.
fn drain<T>(
    iter: Box<dyn Iterator<Item = Result<T, SourceError>> + '_>,
) -> Result<Vec<T>, EdgeProviderError> {
    let mut out = Vec::new();
    for item in iter {
        out.push(item?);
    }
    Ok(out)
}
