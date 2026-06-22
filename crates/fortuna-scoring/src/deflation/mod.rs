//! Backtest-overfitting deflation: purged+embargoed CSCV/PBO, Hansen `SPA_c`,
//! effective-N + MinTRL, and the Deflated Sharpe Ratio (spec §5 G-TRUTH, §7;
//! plan S4).
//!
//! Every formula here is implemented EXACTLY as quoted in
//! `docs/research/2026-06-21-ws3-backtest-overfitting-grounding.md`:
//!
//! - §1  PBO via CSCV — [`pbo`] (rank-based, metric-agnostic).
//! - §2  Purging + embargo — [`purge_embargo`] (the #1 lie-prevention).
//! - §3  Effective-N + MinTRL — [`effective_n`], [`mintrl`].
//! - §4  Hansen SPA_c (studentized + recentered, stationary block bootstrap) —
//!   [`spa_c`].
//! - §5  Deflated Sharpe Ratio — [`dsr`].
//!
//! # Purity (the load-bearing crate invariant)
//!
//! This module adds NO dependency to `fortuna-scoring`. The SPA stationary block
//! bootstrap needs randomness; rather than pull in `rand`/`getrandom`, all
//! randomness flows through the [`SeededRng`] trait and a hand-rolled
//! [`SplitMix64`] (the same "carries no `rand`/`libm`" posture as the
//! `dm.rs` hand-rolled erf). The normal CDF / inverse-CDF used by MinTRL and
//! DSR are likewise self-contained rational approximations. No IO, no `Clock`,
//! no async — pure math on `&[f64]`.

mod cscv;
mod dsr;
mod effective_n;
mod purge;
mod spa;

pub use cscv::{pbo, PboReport};
pub use dsr::dsr;
pub use effective_n::{effective_n, mintrl};
pub use purge::{purge_embargo, Duration, LabelWindow};
pub use spa::{spa_c, SeededRng, SpaReport, SplitMix64};

/// A `T × N` matrix: `matrix[t][n]` is config `n`'s value on time-slice `t`.
///
/// For [`pbo`] the cell is a forecasting-edge value (e.g. mean Brier-skill); for
/// [`spa_c`] it is a per-slice loss differential `d_{k,t}`. The math is agnostic
/// to which.
pub type Matrix = Vec<Vec<f64>>;

/// Standard-normal CDF `Φ(x) = ½(1 + erf(x/√2))`.
///
/// Shared by MinTRL/DSR. The `erf` is the Abramowitz & Stegun 7.1.26 rational
/// approximation (max abs error ≈ 1.5e-7) — identical in spirit to the `dm.rs`
/// precedent, so the crate carries no `libm`.
pub(crate) fn standard_normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

/// Error function via Abramowitz & Stegun 7.1.26 (pure — no `libm`).
pub(crate) fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();

    const A1: f64 = 0.254_829_592;
    const A2: f64 = -0.284_496_736;
    const A3: f64 = 1.421_413_741;
    const A4: f64 = -1.453_152_027;
    const A5: f64 = 1.061_405_429;
    const P: f64 = 0.327_591_1;

    let t = 1.0 / (1.0 + P * x);
    let poly = ((((A5 * t + A4) * t + A3) * t + A2) * t + A1) * t;
    let y = 1.0 - poly * (-x * x).exp();

    sign * y
}

/// Inverse standard-normal CDF `Φ⁻¹(p)` via Acklam's rational approximation
/// (max relative error ≈ 1.15e-9 over `p ∈ (0,1)`). Used for the Gumbel
/// expected-maximum-Sharpe `SR0` term in [`dsr`]. Pure — no `libm`.
///
/// For `p` at or outside `(0, 1)` it returns `±∞`/`NaN` rather than panicking;
/// callers pass `1 − 1/N` style arguments which stay strictly interior for the
/// `N ≥ 2` regime the DSR is defined on.
pub(crate) fn inv_standard_normal_cdf(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }

    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239e0,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838e0,
        -2.549_732_539_343_734e0,
        4.374_664_141_464_968e0,
        2.938_163_982_698_783e0,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996e0,
        3.754_408_661_907_416e0,
    ];

    let p_low = 0.024_25;
    let p_high = 1.0 - p_low;

    if p < p_low {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= p_high {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}
