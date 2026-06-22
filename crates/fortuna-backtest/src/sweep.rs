//! The G-TRUTH sweep driver (spec §5 G-TRUTH, §7; plan S5).
//!
//! A [`run_sweep`] enumerates the trial space {calibration-window, recal-method,
//! scope, GO-threshold} — the knobs where backtest overfitting actually lives —
//! computes each config's out-of-sample forecasting edge, assembles the CSCV
//! matrix, and runs the deflation toolkit (`pbo`, `spa_c`, `effective_n`,
//! `mintrl`, `dsr`) from `fortuna_scoring::deflation`. It selects the best config
//! and produces a [`ValidationRun`]: the whole-truth, overfitting-deflated
//! GO/NO-GO surface — never a lone flattering number.
//!
//! # The two V&V-fix bindings (these are the whole point of S5)
//!
//! **BLOCK-1 — Brier is the PRIMARY gated metric.** The verdict is computed by
//! the pure `fortuna_scoring::decide` over a `DeflatedView`: GO iff `N_eff` is
//! sufficient AND the **Brier-skill** edge > 0 AND the **Brier-skill** PBO ≤ 0.05
//! AND the **Brier-loss** SPA `p_c` < α. CLV is reported with its own deflation as
//! a CORROBORATING axis only — it can never create a GO.
//!
//! **BLOCK-2 — the trial count N is the JOINT scope × config grid.** When `K`
//! scopes are validated, `family_n_trials = |scopes| × |configs|`, and the
//! DSR/SPA `N` deflate against this `family_n_trials` — never one scope's config
//! count. (Romano–Wolf StepM family-wise control across the grid is a recorded
//! deferral; the N-counting itself is NOT deferrable.)
//!
//! # The `pbo == 0.0` footgun (S4-verifier forward-note)
//!
//! An empty/degenerate `PboReport` returns `pbo == 0.0` (which alone points
//! GO-direction). The sweep carries `n_logits` into the `DeflatedView` so `decide`
//! treats `n_logits == 0` (with thin `N_eff`) as `Insufficient`, never a pass.
//!
//! # Determinism & purity
//!
//! No source-name literals appear here (the decoupling invariant). The edge
//! series are supplied by the caller through an [`EdgeProvider`], so the sweep is
//! source-agnostic and deterministic: the SPA bootstrap is driven by a seeded
//! `SplitMix64` from a caller-fixed seed.

use fortuna_scoring::deflation::{
    dsr, effective_n, mintrl, pbo, spa_c, Duration, LabelWindow, Matrix, PboReport, SplitMix64,
};
use fortuna_scoring::{decide, DeflatedView, GoDecision};
use serde::{Deserialize, Serialize};

/// The recalibration method knob of the trial space.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecalMethod {
    /// Platt (logistic) recalibration.
    Platt,
    /// Isotonic (PAV) recalibration.
    Isotonic,
    /// No recalibration (raw model probabilities).
    None,
}

/// The trial space the sweep enumerates: the knobs where overfitting lives.
///
/// The per-scope config grid is the cartesian product
/// `calibration_windows × recal_methods × go_thresholds`; its cardinality is
/// [`TrialSpace::n_configs`]. The JOINT family grid additionally multiplies by
/// `|scopes|` — see [`TrialSpace::family_n_trials`] (BLOCK-2).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TrialSpace {
    /// Candidate calibration-window lengths (observations).
    pub calibration_windows: Vec<u32>,
    /// Candidate recalibration methods.
    pub recal_methods: Vec<RecalMethod>,
    /// The scopes being validated (opaque labels). The trial count deflates
    /// against the JOINT scope × config grid.
    pub scopes: Vec<String>,
    /// Candidate GO thresholds.
    pub go_thresholds: Vec<f64>,
}

/// One concrete config drawn from the trial space.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct SelectedConfig {
    /// The calibration window of the selected config.
    pub calibration_window: u32,
    /// The recalibration method of the selected config.
    pub recal_method: RecalMethod,
    /// The GO threshold of the selected config.
    pub go_threshold: f64,
}

impl TrialSpace {
    /// The number of configs in the per-scope grid:
    /// `|calibration_windows| × |recal_methods| × |go_thresholds|`.
    pub fn n_configs(&self) -> usize {
        self.calibration_windows
            .len()
            .saturating_mul(self.recal_methods.len())
            .saturating_mul(self.go_thresholds.len())
    }

    /// The JOINT scope × config grid size (BLOCK-2): `|scopes| × n_configs`.
    /// This is the `N` the DSR/SPA deflate against — never a single scope's
    /// config count.
    pub fn family_n_trials(&self) -> usize {
        self.scopes.len().saturating_mul(self.n_configs())
    }

    /// The config at flat index `i` of the per-scope grid, decoded in
    /// `window`-major, then `method`, then `threshold` order. `None` when the grid
    /// is empty or `i` is out of range.
    fn config_at(&self, i: usize) -> Option<SelectedConfig> {
        let nm = self.recal_methods.len();
        let nt = self.go_thresholds.len();
        let per_window = nm.saturating_mul(nt);
        if per_window == 0 || i >= self.n_configs() {
            return None;
        }
        let wi = i / per_window;
        let rem = i % per_window;
        let mi = rem / nt;
        let ti = rem % nt;
        Some(SelectedConfig {
            calibration_window: *self.calibration_windows.get(wi)?,
            recal_method: *self.recal_methods.get(mi)?,
            go_threshold: *self.go_thresholds.get(ti)?,
        })
    }
}

/// The out-of-sample edge series the sweep needs for one `(scope, config)` pair.
///
/// All four are per-slice series of equal length. The sweep is source-agnostic:
/// the caller computes these by replaying the source through the scoring rules.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigEdges {
    /// Per-slice Brier-skill (beats-baseline margin) OOS edge — the PRIMARY metric.
    pub brier_oos: Vec<f64>,
    /// Per-slice Brier-loss differential (model − baseline) for the SPA test.
    pub brier_loss_diff: Vec<f64>,
    /// Per-slice CLV edge — the corroborating axis only.
    pub clv_oos: Vec<f64>,
    /// Per-slice paper-trade returns for the walled-off DSR context.
    pub sharpe_returns: Vec<f64>,
}

/// Supplies the OOS edge series for a `(scope, config_index)` pair. Pure and
/// deterministic — the sweep never reads wall-clock time or external state.
pub trait EdgeProvider {
    /// Edges for the config at per-scope grid index `config_index` under `scope`.
    fn edges(&self, scope: &str, config_index: usize) -> ConfigEdges;

    /// The per-row label eval windows + embargo for purge/embargo under `scope`.
    ///
    /// The returned `Vec<LabelWindow>` MUST have the SAME length as each
    /// `(scope, config)` edge series (`t`, the matrix row count). `pbo` only
    /// purges when `windows.len() == t` (else it silently takes the no-purge path
    /// and UNDERSTATES overfitting), so `run_sweep` asserts this equality before
    /// calling `pbo` and the provider is contractually obliged to honor it.
    ///
    /// The default returns `(empty, zero)` — the no-purge baseline — so callers
    /// that supply already-OOS series with no within-fold leak (e.g. the
    /// closure-based `Fn` provider) keep the WS3 behavior unchanged.
    fn windows(&self, _scope: &str) -> (Vec<LabelWindow>, Duration) {
        (Vec::new(), Duration::zero())
    }
}

impl<F> EdgeProvider for F
where
    F: Fn(&str, usize) -> ConfigEdges,
{
    fn edges(&self, scope: &str, config_index: usize) -> ConfigEdges {
        self(scope, config_index)
    }
}

/// Tunables for the sweep's deflation calls.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepParams {
    /// Significance level the Brier SPA `p_c` is gated against (strict `<`).
    pub alpha: f64,
    /// Number of CSCV submatrices `S` (default 16; validated against `T` by `pbo`).
    pub cscv_s: usize,
    /// Stationary-block-bootstrap mean block length.
    pub block_len: usize,
    /// Number of SPA bootstrap resamples.
    pub n_boot: usize,
    /// Seed for the deterministic SPA bootstrap PRNG.
    pub seed: u64,
    /// One-sided `Z_α` for the MinTRL guard.
    pub z_alpha: f64,
}

impl Default for SweepParams {
    fn default() -> Self {
        SweepParams {
            alpha: 0.05,
            cscv_s: 16,
            block_len: 4,
            n_boot: 200,
            seed: 0x5713_2026_0621,
            z_alpha: 1.645,
        }
    }
}

/// The whole-truth, overfitting-deflated GO/NO-GO surface for one sweep (spec §7).
///
/// Every field is carried so a reader never has to trust a lone number: Brier is
/// the gated headline (`brier_*`), CLV is corroborating (`clv_*`), `mintrl_ok` and
/// `sharpe_dsr` are walled-off context, and `n_trials`/`family_n_trials` make the
/// multiple-testing scope explicit (BLOCK-2). Persisted append-only to
/// `validation_runs` (I5).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ValidationRun {
    /// ULID of this run.
    pub run_id: String,
    /// The scope this verdict is reported under (the scope of the selected config).
    pub scope: String,
    /// The producer attributed (opaque), when one is.
    pub producer: Option<String>,
    /// The trial space that was enumerated.
    pub trial_space: TrialSpace,
    /// Configs explored in the per-scope grid (`TrialSpace::n_configs`).
    pub n_trials: usize,
    /// The JOINT scope × config grid — the deflation `N` (BLOCK-2).
    pub family_n_trials: usize,
    /// The selected (in-sample-best) config.
    pub selected_config: Option<SelectedConfig>,
    /// The selected config's Brier-skill OOS edge (the gated headline).
    pub brier_edge: f64,
    /// PBO on the Brier-skill matrix.
    pub brier_pbo: f64,
    /// SPA `p_c` on the Brier-loss differential.
    pub brier_spa_p: f64,
    /// The CLV OOS edge (corroborating only).
    pub clv_edge: f64,
    /// PBO on the CLV matrix (corroborating only).
    pub clv_pbo: f64,
    /// SPA `p_c` on the CLV differential (corroborating only).
    pub clv_spa_p: f64,
    /// Effective independent sample size after purging.
    pub effective_n: f64,
    /// Whether `effective_n >= MinTRL` (supporting context).
    pub mintrl_ok: bool,
    /// Deflated Sharpe Ratio of the paper-trade PnL (walled-off context).
    pub sharpe_dsr: f64,
    /// The deflated verdict (the existing WS2 `GoDecision` — no fourth state).
    pub verdict: GoDecision,
    /// UTC ISO8601 timestamp the run was computed at.
    pub computed_at: String,
}

/// Run the G-TRUTH sweep over `space`, returning the deflated GO surface.
///
/// For each `(scope, config)` pair the `provider` supplies the OOS edge series.
/// The sweep assembles, **per scope**, a `T × n_configs` CSCV matrix of
/// Brier-skill edges and a `T × n_configs` matrix of Brier-loss differentials,
/// then:
///
/// - `pbo` on the Brier-skill matrix (purged is the caller's concern at the edge
///   layer; here the matrix is metric-agnostic) → `brier_pbo`, `n_logits`;
/// - `spa_c` on the Brier-loss differentials → `brier_spa_p`;
/// - `effective_n` / `mintrl` on the selected config's edge series → the N guard;
/// - `dsr` on the paper returns, deflating against **`family_n_trials`** (BLOCK-2);
/// - the CLV axis is deflated identically and reported as corroborating only.
///
/// The verdict is the pure `fortuna_scoring::decide` over the assembled
/// `DeflatedView` (BLOCK-1). The selected scope is the first scope (the report is
/// per-scope; the family count spans all scopes for the deflation).
///
/// Never panics: an empty trial space yields a `n_trials == 0`, `Insufficient`
/// run with finite (sentinel) metrics. The `run_id`/`computed_at` are deterministic
/// placeholders derived from the seed so the pure sweep needs no `Clock`; the CLI
/// (S7) stamps the real ULID + injected-clock timestamp on persist.
pub fn run_sweep<P: EdgeProvider>(
    space: &TrialSpace,
    params: &SweepParams,
    provider: P,
) -> ValidationRun {
    let n_configs = space.n_configs();
    let family_n_trials = space.family_n_trials();

    // The report is reported under the first scope; the deflation N spans all
    // scopes (family_n_trials). An empty trial space is well-defined.
    let scope = space.scopes.first().cloned().unwrap_or_default();

    // Degenerate trial space: nothing to evaluate.
    if n_configs == 0 || scope.is_empty() {
        return ValidationRun {
            run_id: format!("01SWEEP{:016X}", params.seed),
            scope,
            producer: None,
            trial_space: space.clone(),
            n_trials: n_configs,
            family_n_trials,
            selected_config: None,
            brier_edge: 0.0,
            brier_pbo: 0.0,
            brier_spa_p: 1.0,
            clv_edge: 0.0,
            clv_pbo: 0.0,
            clv_spa_p: 1.0,
            effective_n: 0.0,
            mintrl_ok: false,
            sharpe_dsr: 0.5,
            verdict: GoDecision::Insufficient,
            computed_at: String::new(),
        };
    }

    // Assemble the per-scope CSCV matrices for the reported scope. matrix[t][c]
    // = config c's Brier-skill edge on slice t; loss_diff[t][c] likewise for the
    // Brier-loss differential. CLV gets its own pair.
    let mut per_config: Vec<ConfigEdges> = Vec::with_capacity(n_configs);
    for c in 0..n_configs {
        per_config.push(provider.edges(&scope, c));
    }

    // Number of time slices (the shortest series across configs, so the matrix is
    // rectangular even if a provider is ragged).
    let t = per_config
        .iter()
        .map(|e| e.brier_oos.len())
        .min()
        .unwrap_or(0);

    let brier_matrix: Matrix = (0..t)
        .map(|row| per_config.iter().map(|e| e.brier_oos[row]).collect())
        .collect();
    let brier_loss_matrix: Matrix = (0..t)
        .map(|row| {
            per_config
                .iter()
                .map(|e| *e.brier_loss_diff.get(row).unwrap_or(&0.0))
                .collect()
        })
        .collect();
    let clv_matrix: Matrix = (0..t)
        .map(|row| {
            per_config
                .iter()
                .map(|e| *e.clv_oos.get(row).unwrap_or(&0.0))
                .collect()
        })
        .collect();
    let clv_loss_matrix = clv_matrix.clone();

    // Per-slice purge windows + embargo from the provider (W7). `pbo` ONLY purges
    // when `label_windows.len() == T` (cscv.rs); a ragged or short window list
    // silently takes the no-purge path and UNDERSTATES overfitting. So we assert
    // the length invariant LOUDLY here: the provider's window count must equal the
    // matrix row count `t`. A provider that returns no windows (the default /
    // closure path) supplies an empty list — the no-purge baseline — which is the
    // correct behavior for already-OOS series with no within-fold leak, and which
    // the assertion below explicitly tolerates (`empty == no-purge`, never a
    // length mismatch against a non-empty matrix).
    let (windows, embargo) = provider.windows(&scope);
    if !windows.is_empty() {
        // Non-empty windows MUST match the matrix rows exactly, or `pbo` would
        // silently no-op the purge. Fail loudly rather than ship a no-purge run
        // that masquerades as purged. (`t == 0` with non-empty windows is also a
        // mismatch and is caught here.)
        assert_eq!(
            windows.len(),
            t,
            "EdgeProvider::windows must return EXACTLY t windows (one per matrix \
             row) so pbo takes the purge path; got {} windows for {} rows",
            windows.len(),
            t,
        );
    }

    let brier_pbo_report: PboReport = pbo(&brier_matrix, params.cscv_s, &windows, embargo);
    let clv_pbo_report: PboReport = pbo(&clv_matrix, params.cscv_s, &windows, embargo);

    // SPA on the loss differentials, deterministic via a seeded SplitMix64.
    let mut brier_rng = SplitMix64::seed(params.seed);
    let brier_spa = spa_c(
        &brier_loss_matrix,
        params.block_len,
        params.n_boot,
        &mut brier_rng,
    );
    let mut clv_rng = SplitMix64::seed(params.seed ^ 0xC1F);
    let clv_spa = spa_c(
        &clv_loss_matrix,
        params.block_len,
        params.n_boot,
        &mut clv_rng,
    );

    // Select the in-sample-best config: the highest mean Brier-skill edge over the
    // slices (the gated metric drives selection).
    let mut best_c = 0usize;
    let mut best_mean = f64::NEG_INFINITY;
    for (c, e) in per_config.iter().enumerate() {
        let mean = mean_of(&e.brier_oos);
        if mean > best_mean {
            best_mean = mean;
            best_c = c;
        }
    }
    let selected_config = space.config_at(best_c);
    let selected = &per_config[best_c];

    let brier_edge = mean_of(&selected.brier_oos);
    let clv_edge = mean_of(&selected.clv_oos);

    // Effective-N / MinTRL on the selected config's edge series.
    let eff_n = effective_n(&selected.brier_oos);
    let sr_hat = sharpe(&selected.brier_oos);
    let (skew, kurt) = skew_kurt(&selected.brier_oos);
    let min_trl = mintrl(sr_hat, 0.0, skew, kurt, params.z_alpha);
    let mintrl_ok = eff_n >= min_trl;

    // DSR on the paper-trade returns, deflating against the JOINT family grid
    // (BLOCK-2): n_eff_trials = family_n_trials, NOT n_configs.
    let ret_sr = sharpe(&selected.sharpe_returns);
    let (ret_skew, ret_kurt) = skew_kurt(&selected.sharpe_returns);
    let trial_sr_variance = trial_sharpe_variance(&per_config);
    let sharpe_dsr = dsr(
        ret_sr,
        selected.sharpe_returns.len() as f64,
        ret_skew,
        ret_kurt,
        trial_sr_variance,
        family_n_trials as f64,
    );

    // The whole-truth deflated view -> the pure BLOCK-1 verdict.
    let view = DeflatedView {
        effective_n: eff_n,
        n_logits: brier_pbo_report.n_logits,
        alpha: params.alpha,
        brier_edge,
        brier_pbo: brier_pbo_report.pbo,
        brier_spa_p: brier_spa.p_c,
        clv_edge,
        clv_pbo: clv_pbo_report.pbo,
        clv_spa_p: clv_spa.p_c,
        mintrl_ok,
        sharpe_dsr,
    };
    let verdict = decide(&view);

    ValidationRun {
        run_id: format!("01SWEEP{:016X}", params.seed),
        scope,
        producer: None,
        trial_space: space.clone(),
        n_trials: n_configs,
        family_n_trials,
        selected_config,
        brier_edge,
        brier_pbo: brier_pbo_report.pbo,
        brier_spa_p: brier_spa.p_c,
        clv_edge,
        clv_pbo: clv_pbo_report.pbo,
        clv_spa_p: clv_spa.p_c,
        effective_n: eff_n,
        mintrl_ok,
        sharpe_dsr,
        verdict,
        computed_at: String::new(),
    }
}

/// Mean of a slice; `0.0` for an empty slice (never NaN, never panics).
fn mean_of(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Per-period Sharpe of a return series (`mean / std`); `0.0` for a degenerate
/// (empty or zero-variance) series.
fn sharpe(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean_of(xs);
    let var = xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / xs.len() as f64;
    if var <= 1e-12 {
        return 0.0;
    }
    m / var.sqrt()
}

/// Sample skewness and raw kurtosis of a series. Returns `(0.0, 3.0)` (the Normal
/// reference) for a degenerate series so the MinTRL/DSR moment terms stay sane.
fn skew_kurt(xs: &[f64]) -> (f64, f64) {
    if xs.len() < 2 {
        return (0.0, 3.0);
    }
    let n = xs.len() as f64;
    let m = mean_of(xs);
    let var = xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / n;
    if var <= 1e-12 {
        return (0.0, 3.0);
    }
    let sd = var.sqrt();
    let skew = xs.iter().map(|x| ((x - m) / sd).powi(3)).sum::<f64>() / n;
    let kurt = xs.iter().map(|x| ((x - m) / sd).powi(4)).sum::<f64>() / n;
    (skew, kurt)
}

/// Variance of the per-config Sharpe ratios `V[{SR_n}]` — the DSR trial-Sharpe
/// dispersion term. `0.0` when there are fewer than two configs.
fn trial_sharpe_variance(configs: &[ConfigEdges]) -> f64 {
    if configs.len() < 2 {
        return 0.0;
    }
    let srs: Vec<f64> = configs.iter().map(|e| sharpe(&e.sharpe_returns)).collect();
    let m = mean_of(&srs);
    srs.iter().map(|s| (s - m) * (s - m)).sum::<f64>() / srs.len() as f64
}
