//! Hansen's Superior Predictive Ability test, consistent variant `SPA_c`
//! (research §4).
//!
//! Given a `T × K` matrix of loss differentials `d_{k,t} = loss_benchmark_t −
//! loss_model_{k,t}` (positive ⇒ model `k` beats the benchmark on slice `t`),
//! `SPA_c` tests the composite null `H0: max_k E(d_k) ≤ 0` ("the best of the K
//! models is no better than the benchmark") while correcting for the
//! data-snooping that comes from picking the best of K.
//!
//! Verbatim from §4:
//! - **Studentize:** `T^SPA = max_k max(√n·d̄_k/ω̂_k, 0)`.
//! - **Recenter the null (the consistent variant):**
//!   `µ̂_k^c = d̄_k · 1{ √n·d̄_k/ω̂_k ≤ −√(2·ln ln n) }` — i.e. in the bootstrap
//!   world a model is given a non-zero (negative) mean ONLY if it is
//!   demonstrably inferior; every other model is recentered to mean 0. This is
//!   what makes poor/irrelevant models asymptotically irrelevant (White's RC,
//!   by contrast, recenters *every* model to its own sample mean and so can be
//!   "eroded to power 0" by a bad model).
//! - **Stationary block bootstrap** (Politis–Romano) with mean block length
//!   `block_len ∝ n^(1/3)`; the bootstrap statistic uses the recentered means.
//! - The consistent p-value `p_c` sits between the liberal `p_l` (recenter
//!   nothing) and the conservative `p_u` (recenter everything to its sample
//!   mean — i.e. White's RC).
//!
//! All randomness flows through [`SeededRng`] so the bootstrap is deterministic
//! and reproducible — and the crate stays free of `rand`/`getrandom`.

use super::Matrix;
use serde::{Deserialize, Serialize};

/// Smallest per-model std treated as nonzero. Below this the model's
/// differential is degenerate (constant) and it contributes nothing to the
/// studentized statistic.
const MIN_STD: f64 = 1e-12;

/// A deterministic seeded PRNG. The deflation library takes randomness only
/// through this trait so it stays pure (no `rand`/`getrandom` dependency).
pub trait SeededRng {
    /// Next 64-bit pseudo-random word.
    fn next_u64(&mut self) -> u64;

    /// A uniform integer in `[lo, hi)`. For `lo >= hi` returns `lo`.
    fn gen_range(&mut self, lo: usize, hi: usize) -> usize {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo) as u64;
        lo + (self.next_u64() % span) as usize
    }
}

/// SplitMix64 — a hand-rolled, dependency-free 64-bit PRNG (Steele, Lea & Flood
/// 2014). Deterministic given a seed; used for the SPA stationary block
/// bootstrap and the test fixtures. Carries no `rand`.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Seed the generator.
    pub fn seed(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }
}

impl SeededRng for SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Result of a `SPA_c` test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaReport {
    /// The studentized SPA statistic `max_k max(√n·d̄_k/ω̂_k, 0)`.
    pub statistic: f64,
    /// The **consistent** p-value `p_c` (recenter only demonstrably-inferior
    /// models). This is the one to gate on.
    pub p_c: f64,
    /// The **liberal** lower-bound p-value `p_l` (recenter nothing).
    pub p_l: f64,
    /// The **conservative** upper-bound p-value `p_u` (= White's RC; recenter
    /// every model to its own sample mean).
    pub p_u: f64,
}

/// How the bootstrap-world per-model means are recentered.
#[derive(Clone, Copy)]
enum Recenter {
    /// Liberal: recenter EVERY model to mean 0 (subtract each sample mean), so no
    /// model is ever treated as inferior. Yields the smallest p-value `p_l`.
    Liberal,
    /// Consistent: recenter only demonstrably-inferior models. `p_c`.
    Consistent,
    /// Conservative (White RC): every model keeps its own sample mean. `p_u`.
    Conservative,
}

/// Hansen `SPA_c` on the loss-differential matrix.
///
/// `loss_diffs[t][k]` is `d_{k,t}`. `block_len` is the mean block length for the
/// stationary block bootstrap (the caller chooses it `∝ n^(1/3) ≥` the
/// autocorrelation horizon); a value `< 1` is treated as `1`. `n_boot` bootstrap
/// resamples are drawn through `rng`.
///
/// Returns `{statistic, p_c, p_l, p_u}`. Degenerate inputs are well-defined and
/// never panic: an empty matrix or `n_boot == 0` yields a statistic of `0` and
/// p-values of `1.0`.
pub fn spa_c(
    loss_diffs: &Matrix,
    block_len: usize,
    n_boot: usize,
    rng: &mut impl SeededRng,
) -> SpaReport {
    let n = loss_diffs.len();
    let k = loss_diffs.first().map(|r| r.len()).unwrap_or(0);

    if n == 0 || k == 0 || n_boot == 0 {
        return SpaReport {
            statistic: 0.0,
            p_c: 1.0,
            p_l: 1.0,
            p_u: 1.0,
        };
    }

    let n_f = n as f64;
    let sqrt_n = n_f.sqrt();
    let block = block_len.max(1);

    // Per-model sample mean d̄_k and std ω̂_k.
    let mut mean = vec![0.0f64; k];
    for row in loss_diffs.iter() {
        for (kk, v) in row.iter().enumerate() {
            mean[kk] += v;
        }
    }
    for m in mean.iter_mut() {
        *m /= n_f;
    }
    let mut std = vec![0.0f64; k];
    for row in loss_diffs.iter() {
        for (kk, v) in row.iter().enumerate() {
            let d = v - mean[kk];
            std[kk] += d * d;
        }
    }
    for s in std.iter_mut() {
        *s = (*s / n_f).sqrt();
    }

    // Observed studentized statistic T^SPA = max_k max(√n·d̄_k/ω̂_k, 0).
    let studentized = |kk: usize| -> f64 {
        if std[kk] <= MIN_STD {
            0.0
        } else {
            sqrt_n * mean[kk] / std[kk]
        }
    };
    let statistic = (0..k)
        .map(|kk| studentized(kk).max(0.0))
        .fold(0.0f64, f64::max);

    // Recentering threshold for the consistent variant: −√(2·ln ln n). For very
    // small n where ln ln n is undefined/non-positive the threshold collapses to
    // 0 (only strictly-negative-mean models are recentered), which is the safe
    // conservative-of-consistent behaviour.
    let lln = n_f.ln().ln();
    let threshold = if lln > 0.0 { -(2.0 * lln).sqrt() } else { 0.0 };

    // Per-model bootstrap-world mean offset (subtracted from each bootstrap
    // resample mean) for a given recentering policy.
    let offset = |recenter: Recenter, kk: usize| -> f64 {
        match recenter {
            Recenter::Liberal => mean[kk], // subtract full mean -> null mean 0
            Recenter::Conservative => 0.0, // keep sample mean (White RC)
            Recenter::Consistent => {
                // Recenter (subtract mean -> null 0) UNLESS demonstrably inferior.
                if std[kk] > MIN_STD && studentized(kk) <= threshold {
                    0.0 // demonstrably inferior: keep its (negative) sample mean
                } else {
                    mean[kk] // otherwise recenter to 0
                }
            }
        }
    };

    // Run the three bootstraps off the SAME resampled index stream so the
    // p-values are directly comparable (p_l ≤ p_c ≤ p_u by construction of the
    // offsets). Each bootstrap draws one set of block indices per replicate.
    let mut count = [0usize; 3]; // [liberal, consistent, conservative]
    let policies = [
        Recenter::Liberal,
        Recenter::Consistent,
        Recenter::Conservative,
    ];

    for _ in 0..n_boot {
        let idx = stationary_block_indices(n, block, rng);

        // Bootstrap resample means per model.
        let mut bmean = vec![0.0f64; k];
        for &t in idx.iter() {
            let row = &loss_diffs[t];
            for (kk, v) in row.iter().enumerate() {
                bmean[kk] += v;
            }
        }
        for m in bmean.iter_mut() {
            *m /= n_f;
        }

        for (p_i, &policy) in policies.iter().enumerate() {
            let mut tmax = 0.0f64;
            for kk in 0..k {
                if std[kk] <= MIN_STD {
                    continue;
                }
                let centered = bmean[kk] - offset(policy, kk);
                let z = (sqrt_n * centered / std[kk]).max(0.0);
                if z > tmax {
                    tmax = z;
                }
            }
            if tmax > statistic {
                count[p_i] += 1;
            }
        }
    }

    let pval = |c: usize| (c as f64) / (n_boot as f64);
    SpaReport {
        statistic,
        p_l: pval(count[0]),
        p_c: pval(count[1]),
        p_u: pval(count[2]),
    }
}

/// Draw `n` indices for one stationary-block-bootstrap resample (Politis–Romano):
/// start a new block at a uniform random position with probability `1/block`,
/// otherwise step to the next index (wrapping). Mean block length is `block`.
fn stationary_block_indices(n: usize, block: usize, rng: &mut impl SeededRng) -> Vec<usize> {
    let mut idx = Vec::with_capacity(n);
    // Geometric restart probability p = 1/block, compared against next_u64 scaled
    // to [0,1). Using the modulus `next_u64() % block == 0` gives exactly p=1/block.
    let mut cur = rng.gen_range(0, n);
    for _ in 0..n {
        idx.push(cur);
        let restart = block <= 1 || (rng.next_u64() as usize).is_multiple_of(block);
        if restart {
            cur = rng.gen_range(0, n);
        } else {
            cur = (cur + 1) % n;
        }
    }
    idx
}
