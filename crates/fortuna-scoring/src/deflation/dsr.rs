//! Deflated Sharpe Ratio (research §5) — the walled-off PnL-context number.
//!
//! `DSR = Φ[ (SR_hat − SR0) · √(T − 1) / √(1 − γ3·SR_hat + ((γ4−1)/4)·SR_hat²) ]`
//!
//! The denominator uses **`SR_hat`** (the selected strategy's Sharpe), NOT
//! `SR0` — this was the one contested point in the research and is resolved
//! against the reference implementation (Wikipedia renders it wrong). DSR = PSR
//! evaluated at the benchmark `SR0`.
//!
//! `SR0` is the expected maximum Sharpe under `N` independent trials (Gumbel):
//! `SR0 = √(V[{SR_n}]) · [ (1 − γ_e)·Φ⁻¹(1 − 1/N) + γ_e·Φ⁻¹(1 − 1/(N·e)) ]`,
//! where `γ_e = 0.5772…` (Euler–Mascheroni), `V[{SR_n}]` is the variance of the
//! `N` trial Sharpes, and `N` is the **effective independent** trial count
//! (bigger `N` ⇒ bigger `SR0` ⇒ smaller DSR — undercounting `N` is the #1 way to
//! inflate the DSR).
//!
//! `γ4` is **raw** kurtosis (Normal = 3); it enters as `(γ4 − 1)/4`.

use super::{inv_standard_normal_cdf, standard_normal_cdf};

/// Euler–Mascheroni constant `γ_e`.
const EULER_MASCHERONI: f64 = 0.577_215_664_901_532_9;

/// Deflated Sharpe Ratio.
///
/// - `sr_hat` — the selected strategy's Sharpe (per-period).
/// - `t` — number of observations (`T`); the SR standard error scales as
///   `√(T − 1)`.
/// - `skew` — `γ3` of the selected strategy's returns.
/// - `kurt` — `γ4` (raw) of the selected strategy's returns.
/// - `trial_sr_variance` — `V[{SR_n}]`, the variance of the `N` trial Sharpes.
/// - `n_eff_trials` — `N`, the effective independent trial count.
///
/// Returns a probability in `[0, 1]`. Decision (gating layer): `DSR > 0.95`.
/// Degenerate inputs are well-defined and never panic:
/// - `n_eff_trials < 2` → `SR0 = 0` (no multiple-testing inflation possible).
/// - `t ≤ 1` → `√(T − 1) = 0` → `DSR = Φ(0) = 0.5`.
/// - a non-positive denominator (pathological skew/kurtosis) → `DSR = 0.5`.
pub fn dsr(
    sr_hat: f64,
    t: f64,
    skew: f64,
    kurt: f64,
    trial_sr_variance: f64,
    n_eff_trials: f64,
) -> f64 {
    let sr0 = expected_max_sharpe(trial_sr_variance, n_eff_trials);

    // Variance term uses SR_hat (the resolved contested point).
    let variance_term = 1.0 - skew * sr_hat + ((kurt - 1.0) / 4.0) * sr_hat * sr_hat;
    if variance_term <= 0.0 || !variance_term.is_finite() {
        // Pathological moments: the studentization is undefined → neutral 0.5.
        return 0.5;
    }

    let denom = variance_term.sqrt();
    let t_term = (t - 1.0).max(0.0).sqrt();
    let arg = (sr_hat - sr0) * t_term / denom;
    standard_normal_cdf(arg)
}

/// Expected maximum Sharpe under `n` independent trials with trial-Sharpe
/// variance `var` (Gumbel approximation).
///
/// For `n < 2` returns `0.0` (a single trial has no selection bias).
fn expected_max_sharpe(var: f64, n: f64) -> f64 {
    if n < 2.0 || var <= 0.0 {
        return 0.0;
    }
    let sd = var.sqrt();
    let term1 = (1.0 - EULER_MASCHERONI) * inv_standard_normal_cdf(1.0 - 1.0 / n);
    let term2 = EULER_MASCHERONI * inv_standard_normal_cdf(1.0 - 1.0 / (n * std::f64::consts::E));
    sd * (term1 + term2)
}
