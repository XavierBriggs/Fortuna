//! Effective sample size + Minimum Track Record Length (research §3).
//!
//! Serially-correlated observations carry less independent information than raw
//! `N`. Two tools:
//!
//! - [`effective_n`] — `N_eff`. Preferred AR(1) closed form
//!   `N_eff = N · (1 − ρ) / (1 + ρ)` (ρ = lag-1 autocorrelation; ρ=0.5 → 0.33·N).
//!   Falls back to the general `N_eff = N / (1 + 2·Σ_{t≥1} ρ_t)` when the series
//!   is not AR(1)-like (the AR(1) form would understate the haircut).
//! - [`mintrl`] — the minimum number of observations needed to claim
//!   `SR_hat > SR*` at confidence `1 − α` (one-sided `Z_α`):
//!   `MinTRL = 1 + [1 − γ3·SR + ((γ4−1)/4)·SR²] · (Z_α / (SR − SR*))²`
//!   (Bailey & López de Prado, *Sharpe Ratio Efficient Frontier*, Eq.13).
//!   Requires `SR > SR*`; otherwise it is mathematically undefined and we return
//!   a non-finite value rather than a silently-wrong positive count.

/// Effective sample size `N_eff` of a serially-correlated series.
///
/// Uses the AR(1) closed form `N · (1 − ρ) / (1 + ρ)` when the autocorrelation
/// structure is consistent with AR(1) (geometric decay of the first few lags);
/// otherwise the general `N / (1 + 2·Σ ρ_t)` over the positive-significant lags.
/// The result is clamped to `[1, N]`.
///
/// Degenerate inputs: a series of length `< 2`, or one with (near) zero variance,
/// returns its raw length as `f64` (no correlation can be estimated). Never
/// panics, never returns NaN.
pub fn effective_n(series: &[f64]) -> f64 {
    let n = series.len();
    if n < 2 {
        return n as f64;
    }
    let nf = n as f64;

    let mean = series.iter().sum::<f64>() / nf;
    let var = series.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / nf;
    if var <= 1e-12 {
        return nf;
    }

    // Autocorrelation at lag k.
    let autocorr = |k: usize| -> f64 {
        let mut acc = 0.0;
        for t in k..n {
            acc += (series[t] - mean) * (series[t - k] - mean);
        }
        (acc / nf) / var
    };

    let rho1 = autocorr(1);

    // Decide AR(1)-likeness: check whether lag-2 ≈ ρ1² (geometric decay). Compute
    // a handful of lags for the general fallback regardless.
    let rho2 = if n > 3 { autocorr(2) } else { rho1 * rho1 };

    // If ρ1 is essentially zero, the series is ~white noise → N_eff ≈ N.
    if rho1.abs() < 1e-6 {
        return nf;
    }

    let ar1_geometric = (rho2 - rho1 * rho1).abs() < 0.1 && rho1 > 0.0 && rho1 < 1.0;

    let neff = if ar1_geometric {
        // AR(1) closed form.
        nf * (1.0 - rho1) / (1.0 + rho1)
    } else {
        // General Σ ρ_t over lags whose autocorrelation is still meaningfully
        // positive (truncate at the first non-positive lag — the standard
        // initial-positive-sequence truncation that keeps the sum finite/stable).
        let max_lag = n / 4; // don't trust autocorrelations past ~N/4
        let mut sum_rho = 0.0;
        for k in 1..=max_lag.max(1) {
            let r = autocorr(k);
            if r <= 0.0 {
                break;
            }
            sum_rho += r;
        }
        nf / (1.0 + 2.0 * sum_rho)
    };

    neff.clamp(1.0, nf)
}

/// Minimum Track Record Length: the minimum number of observations to claim
/// `sr_hat > sr_star` at one-sided confidence `1 − α` (caller passes `z_alpha`).
///
/// `MinTRL = 1 + [1 − γ3·SR + ((γ4−1)/4)·SR²] · (Z_α / (SR − SR*))²`, with
/// `SR = sr_hat`, `γ3 = skew`, `γ4 = kurt` (raw kurtosis; Normal = 3 ⇒ the
/// excess term is `½`). All Sharpe values must be in the SAME per-period units.
///
/// Returns a **non-finite** value (`+∞`/`NaN`) when `sr_hat ≤ sr_star`, because
/// the track-record length is then undefined — a deliberate sentinel, not a
/// silently-wrong finite count. Never panics.
pub fn mintrl(sr_hat: f64, sr_star: f64, skew: f64, kurt: f64, z_alpha: f64) -> f64 {
    let gap = sr_hat - sr_star;
    if gap <= 0.0 {
        // Undefined: cannot establish SR_hat > SR* with any finite track record.
        return f64::INFINITY;
    }
    let variance_term = 1.0 - skew * sr_hat + ((kurt - 1.0) / 4.0) * sr_hat * sr_hat;
    1.0 + variance_term * (z_alpha / gap) * (z_alpha / gap)
}
