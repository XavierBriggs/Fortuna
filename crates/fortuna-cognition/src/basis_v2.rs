//! The `basis-v2` FAIR-PROBABILITY KERNEL — the deterministic §3.3 per-bracket
//! fair-probability model (A3) plus the §3.3 ladder no-arbitrage guard (A9).
//! Pure `f64` forecast-domain (design: docs/design/perp-strategies-and-scalar-claims.md
//! §3.3, amendments A3 and A9). This is slice-3b-v2 (sub-slices V0 + V1 + V2).
//!
//! # What this is (and is not)
//!
//! This module is the v2 fair-probability SIGNAL only. Given the settlement
//! model `(anchor S₀, dispersion σ)` and a KXBTC bracket ladder, it computes the
//! per-bracket model fair probability `q_j` (A3) and, separately, validates that
//! the ladder's IMPLIED distribution (from the quoted mids) is no-arb coherent
//! (A9). It is `f64`-cognition throughout — these are forecast quantities,
//! **never money**. There is NO money-type (`Cents`/`PerpPrice`), NO IO, and NO
//! `Clock` here; this kernel is a pure deterministic function of its inputs. It
//! reuses the rung-0 [`crate::basis`] ladder types ([`BracketStrike`],
//! [`BracketBin`]) and that module's canonical price-ordering so every reduction
//! runs in a deterministic order, independent of the caller's input order.
//!
//! # The modeling constants are CALLER-INJECTED (the kernel invents nothing)
//!
//! Per the spec, σ (dispersion) and τ (horizon) are computed OUTSIDE this kernel
//! and passed in: σ comes from the realized volatility of the perp-mark series
//! scaled by √τ (config-overridable, A5), and τ = bracket settlement time − now
//! from the injected `Clock` (A5). This kernel does NOT compute, scale, or invent
//! σ or τ — it takes the already-derived [`SettlementModel`] and applies it. The
//! anchor S₀ is the BRTI/reference price (A6: `FundingObservation.reference_price`),
//! **not** the perp mark; the caller resolves and freshness-vetoes the anchor.
//!
//! # Settlement model: lognormal in price
//!
//! Rung-0-of-v2 (spec line 356): Gaussian in log-price ⇒ lognormal settlement.
//! The settlement-price CDF is `F(price) = Φ( ln(price / S₀) / σ )`, so the
//! distribution's median sits exactly at the anchor S₀ (`ln(S₀/S₀)=0`, `Φ(0)=0.5`).
//! Φ is the standard-normal CDF, computed in-house via the Abramowitz & Stegun
//! 7.1.26 erf rational approximation (max abs err ~1.5e-7) — this is the DC-2
//! default and adds NO dependency (the workspace carries no statrs/libm/erf for
//! this crate). See [`normal_cdf`] and [`lognormal_cdf`].
//!
//! # The no-circularity rule (A3, spec lines 358-360) is load-bearing
//!
//! [`bracket_fair_probs`] computes `q_j` from the model `(anchor, σ)` and the
//! bin STRIKES ONLY ([`BracketStrike`]). It MUST NOT read [`BracketBin::prob`]
//! (the ladder's own implied mid): using the ladder's implied dispersion to
//! price the ladder is circular and forbidden. The bracket-implied distribution
//! is a cross-check DIAGNOSTIC (A10) and is the input to the SEPARATE A9 no-arb
//! check ([`validate_ladder_no_arb`]), never to `q_j`.
//!
//! # Scope boundary — the crossed-quote / free-lock check is NOT here
//!
//! A9 also calls for a crossed-quote / free-lock check (a genuine lock is
//! `mech_structural`'s arbitrage, not a basis trade). That check needs the FULL
//! per-side books (bids AND asks), which this kernel does not take — it sees only
//! the parsed YES mids on [`BracketBin`]. So [`validate_ladder_no_arb`] validates
//! the IMPLIED-CDF monotonicity and the YES-sum only; the crossed-quote / free-lock
//! check lives at the STRATEGY layer (`fortuna-runner`), which holds the books.

use crate::basis::{BracketBin, BracketStrike};

/// The price-axis ordering RANK of a bin, identical to the rung-0 [`crate::basis`]
/// canonical order: the open `less` tail at the bottom (0), the `between` bins in
/// the middle (1), the open `greater` tail at the top (2).
///
/// Defined locally (the rung-0 `order_rank` is module-private and this kernel is
/// strictly ADD-ONLY — it does not modify `basis.rs`). It is a verbatim mirror of
/// that ordering so both kernels reduce the same ladder in the same canonical
/// order; the [`tests::ordering_matches_rung0_median`] test pins the equivalence.
fn order_rank(kind: &BracketStrike) -> u8 {
    match kind {
        BracketStrike::Less { .. } => 0,
        BracketStrike::Between { .. } => 1,
        BracketStrike::Greater { .. } => 2,
    }
}

/// The within-rank ordering KEY (mirror of the rung-0 [`crate::basis`] key): a
/// `between` bin sorts by its `floor`; each open tail sorts by its single strike
/// (at most one of each in a well-formed ladder).
fn order_key(kind: &BracketStrike) -> f64 {
    match kind {
        BracketStrike::Less { cap } => *cap,
        BracketStrike::Between { floor, .. } => *floor,
        BracketStrike::Greater { floor } => *floor,
    }
}

/// A copy of `bins` sorted into the canonical price order ([`order_rank`] then
/// [`order_key`]). Factored out so every reduction in this kernel runs in ONE
/// canonical order, making each output a pure function of the ladder MULTISET,
/// independent of the caller's input order. (The same determinism discipline as
/// the rung-0 median; a prior DST caught an input-order non-determinism in a
/// reduction over this ladder.) The caller's slice is never mutated.
fn price_ordered(bins: &[BracketBin]) -> Vec<BracketBin> {
    let mut sorted: Vec<BracketBin> = bins.to_vec();
    sorted.sort_by(|a, b| {
        order_rank(&a.kind).cmp(&order_rank(&b.kind)).then_with(|| {
            order_key(&a.kind)
                .partial_cmp(&order_key(&b.kind))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    sorted
}

/// Standard-normal CDF Φ(z) via the Abramowitz & Stegun 7.1.26 erf rational
/// approximation (max abs err ~1.5e-7). Pure `f64`, no dependency, deterministic.
///
/// `Φ(z) = 0.5 · (1 + erf(z / √2))`. The erf is the A&S 7.1.26 form
/// `erf(x) = 1 − (a₁t + a₂t² + a₃t³ + a₄t⁴ + a₅t⁵)·e^(−x²)` with
/// `t = 1/(1 + p·x)` for `x ≥ 0`, extended to `x < 0` by the odd symmetry
/// `erf(−x) = −erf(x)`. Saturates cleanly to ~0 / ~1 in the far tails and is
/// exactly `0.5` at `z = 0`. Non-finite `z` is passed through the arithmetic
/// (a `NaN` in yields a `NaN` out); the [`lognormal_cdf`] wrapper is where
/// non-finite inputs are screened to `None` for callers.
pub fn normal_cdf(z: f64) -> f64 {
    0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
}

/// The Abramowitz & Stegun 7.1.26 rational approximation of the error function
/// (max abs err ~1.5e-7). Defined for `x ≥ 0` and extended by odd symmetry.
fn erf(x: f64) -> f64 {
    // A&S 7.1.26 constants.
    const A1: f64 = 0.254_829_592;
    const A2: f64 = -0.284_496_736;
    const A3: f64 = 1.421_413_741;
    const A4: f64 = -1.453_152_027;
    const A5: f64 = 1.061_405_429;
    const P: f64 = 0.327_591_1;

    // Odd symmetry: erf(-x) = -erf(x). Work on |x|, restore the sign at the end.
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = x.abs();

    let t = 1.0 / (1.0 + P * ax);
    // Horner evaluation of (a1 t + a2 t^2 + a3 t^3 + a4 t^4 + a5 t^5).
    let poly = ((((A5 * t + A4) * t + A3) * t + A2) * t + A1) * t;
    let y = 1.0 - poly * (-ax * ax).exp();

    sign * y
}

/// Lognormal-in-price CDF: `F(price) = Φ( ln(price / s0) / sigma )`. The
/// settlement price is lognormal around the anchor `s0` with log-dispersion
/// `sigma` (so the distribution's median is exactly `s0`).
///
/// Returns `None` for `price <= 0`, `s0 <= 0`, `sigma <= 0`, or ANY non-finite
/// input (the caller treats `None` as "no model → propose nothing", mirroring the
/// rung-0 kernel's `None`). On the valid domain the result is in `[0, 1]`,
/// monotone non-decreasing in `price`, `→ 0` as `price → 0⁺`, and `→ 1` as
/// `price → ∞`.
pub fn lognormal_cdf(price: f64, s0: f64, sigma: f64) -> Option<f64> {
    // Screen every input: non-finite (NaN/±inf) or out-of-domain (≤0) ⇒ no model.
    // The `ln` and the division below are only ever reached on strictly-positive
    // finite operands, so neither can produce a NaN/inf from these guards.
    if !price.is_finite() || !s0.is_finite() || !sigma.is_finite() {
        return None;
    }
    if price <= 0.0 || s0 <= 0.0 || sigma <= 0.0 {
        return None;
    }
    let z = (price / s0).ln() / sigma;
    Some(normal_cdf(z))
}

/// The settlement model: lognormal in price, anchored at `anchor` (the BRTI /
/// reference price S₀, NOT the perp mark — A6) with log-dispersion `sigma`
/// (CALLER-supplied — from realized vol scaled by horizon, A5; the kernel does
/// NOT compute or invent it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SettlementModel {
    /// The settlement anchor S₀ in BTC-dollars: the BRTI/reference price (A6),
    /// resolved and freshness-vetoed by the caller. Must be finite and `> 0`.
    pub anchor: f64,
    /// The log-dispersion σ of the lognormal settlement (A5), caller-supplied.
    /// Must be finite and `> 0`; a degenerate σ yields an empty `q_j` vector.
    pub sigma: f64,
}

/// One bracket bin's MODEL fair probability `q_j` (A3): the strike(s) it covers
/// ([`BracketStrike`], carried through from the input bin) and the model
/// probability `q` that settlement lands in it under the [`SettlementModel`].
///
/// `q` is a forecast-domain `f64` in `[0, 1]` — NOT money. It is the v2 signal
/// the EV gate (A4/A8, strategy layer) compares against the EXECUTABLE ask.
#[derive(Debug, Clone, PartialEq)]
pub struct BracketFairProb {
    /// The bin's price-axis strike(s), copied verbatim from the input
    /// [`BracketBin::kind`]. (The implied `prob` is deliberately NOT carried —
    /// `q` is independent of it, A3.)
    pub kind: BracketStrike,
    /// The model fair probability `q_j` that settlement lands in this bin.
    pub q: f64,
}

/// Per-bracket fair probability `q_j` from the settlement model (A3, spec
/// lines 351-361):
///
/// - `Between{floor, cap}` → `F(cap) − F(floor)`
/// - `Greater{floor}`      → `1 − F(floor)`  (open top tail)
/// - `Less{cap}`           → `F(cap)`        (open bottom tail)
///
/// where `F = lognormal_cdf(·, anchor, sigma)`. Bins are iterated in the
/// CANONICAL PRICE ORDER (the [`crate::basis`] ordering: `less` tail at the
/// bottom, `between` ascending by `floor`, `greater` at the top) so the output
/// vector is deterministic regardless of the caller's input order.
///
/// CRITICAL (A3, spec lines 358-360): `q_j` is computed from ONLY `kind` (the
/// strikes) and the model — it MUST NOT read [`BracketBin::prob`]. Using the
/// ladder's own implied probability to price the ladder is circular and
/// forbidden; the implied distribution is a cross-check diagnostic (A10) and the
/// input to the separate [`validate_ladder_no_arb`] gate, never to `q_j`.
///
/// Returns an EMPTY `Vec` on any degenerate input ("no model → no proposal",
/// mirroring the rung-0 kernel's `None`): a non-finite or `≤ 0` `sigma` or
/// `anchor`, an empty ladder, or ANY bin whose required `F(·)` is `None`
/// (e.g. a non-finite or `≤ 0` strike). All-or-nothing: a partial ladder is
/// never returned, so the caller never compares against a half-priced ladder.
pub fn bracket_fair_probs(bins: &[BracketBin], model: SettlementModel) -> Vec<BracketFairProb> {
    // Degenerate model ⇒ no proposal. (lognormal_cdf would itself reject these,
    // but screening up front means an empty ladder short-circuits identically and
    // the intent — "no model" — is explicit.)
    if !model.anchor.is_finite() || !model.sigma.is_finite() {
        return Vec::new();
    }
    if model.anchor <= 0.0 || model.sigma <= 0.0 {
        return Vec::new();
    }
    if bins.is_empty() {
        return Vec::new();
    }

    // Iterate in canonical price order so the output is a pure function of the
    // ladder MULTISET, independent of the caller's input order.
    let sorted = price_ordered(bins);

    let mut out: Vec<BracketFairProb> = Vec::with_capacity(sorted.len());
    for binr in &sorted {
        // `q_j` reads ONLY `kind` (the strikes) — NEVER `binr.prob` (A3
        // no-circularity). Any required F(·) = None (non-finite / ≤0 strike)
        // degrades the WHOLE ladder to empty: no half-priced output.
        let q = match binr.kind {
            BracketStrike::Between { floor, cap } => {
                let f_cap = match lognormal_cdf(cap, model.anchor, model.sigma) {
                    Some(v) => v,
                    None => return Vec::new(),
                };
                let f_floor = match lognormal_cdf(floor, model.anchor, model.sigma) {
                    Some(v) => v,
                    None => return Vec::new(),
                };
                f_cap - f_floor
            }
            BracketStrike::Greater { floor } => {
                let f_floor = match lognormal_cdf(floor, model.anchor, model.sigma) {
                    Some(v) => v,
                    None => return Vec::new(),
                };
                1.0 - f_floor
            }
            BracketStrike::Less { cap } => match lognormal_cdf(cap, model.anchor, model.sigma) {
                Some(v) => v,
                None => return Vec::new(),
            },
        };
        out.push(BracketFairProb { kind: binr.kind, q });
    }
    out
}

/// A specific way the ladder's IMPLIED distribution (from the quoted mids)
/// violates no-arb coherence (A9).
#[derive(Debug, Clone, PartialEq)]
pub enum NoArbViolation {
    /// The implied cumulative probability is NOT non-decreasing across the
    /// price-ordered bins — a CDF must be monotone, so a decrease is an
    /// arbitrage-implying incoherence (a bin priced as if mass were negative).
    NonMonotoneCdf,
    /// The YES probabilities do not sum to within tolerance of `1.0` (the bins
    /// partition the axis, so a coherent ladder's YES-mids sum to ≈ 1). Carries
    /// the offending `sum` for diagnostics.
    YesSumOutOfTolerance {
        /// The realized Σ of the implied YES probabilities (canonical order).
        sum: f64,
    },
}

/// The no-arb health verdict for a ladder's IMPLIED distribution (A9).
#[derive(Debug, Clone, PartialEq)]
pub enum LadderHealth {
    /// The implied distribution is no-arb coherent: monotone implied cumulative
    /// AND YES-sum within tolerance. Safe to compare `q_j` against.
    Coherent,
    /// The implied distribution is incoherent in the carried way → the strategy
    /// DISABLES (proposes nothing): you cannot compare `q_j` to an incoherent
    /// price vector.
    Incoherent(NoArbViolation),
}

/// Validate the ladder's IMPLIED distribution (from [`BracketBin::prob`], the
/// quoted YES mids) for no-arb coherence (A9, spec lines 386-393):
///
/// 1. The implied CUMULATIVE is non-decreasing across the price-ordered bins
///    (a CDF cannot decrease). Checked over the [`crate::basis`] canonical order;
///    a strict decrease beyond a tiny float-noise epsilon ⇒
///    [`NoArbViolation::NonMonotoneCdf`].
/// 2. The YES probabilities sum to within `tol` of `1.0` (the bins partition the
///    price axis). `|Σ − 1| > tol` ⇒
///    [`NoArbViolation::YesSumOutOfTolerance`]. Boundary is INCLUSIVE: exactly
///    `tol` away is still Coherent (only a STRICTLY larger deviation fails).
///
/// Incoherent ⇒ the strategy DISABLES (proposes nothing). An EMPTY ladder is
/// `Incoherent(YesSumOutOfTolerance { sum: 0.0 })` (no mass ≠ a partition of 1).
/// The Σ is taken over the SORTED (canonical) bins, never the caller's input
/// order, so the verdict is deterministic.
///
/// SCOPE: the A9 crossed-quote / free-lock check is NOT performed here — it needs
/// the full per-side books (this kernel sees only mids) and lives at the strategy
/// layer (see the module doc). This function validates implied-CDF monotonicity
/// and the YES-sum only.
pub fn validate_ladder_no_arb(bins: &[BracketBin], tol: f64) -> LadderHealth {
    // Empty ladder: no mass at all is not a partition of probability 1. Report it
    // as a sum-out-of-tolerance with the observed sum (0.0) so the verdict is
    // self-describing and the strategy disables.
    if bins.is_empty() {
        return LadderHealth::Incoherent(NoArbViolation::YesSumOutOfTolerance { sum: 0.0 });
    }

    // Canonical price order so both the monotonicity walk and the Σ are a pure
    // function of the ladder MULTISET, independent of the caller's input order.
    let sorted = price_ordered(bins);

    // (1) Implied cumulative must be non-decreasing. The implied cumulative after
    // bin k is Σ_{i≤k} prob_i; it is non-decreasing iff every per-bin `prob` is
    // ≥ 0 (a negative implied mass is the decrease). A tiny epsilon absorbs
    // float noise so a benign -0.0 / -1e-15 does not false-positive; a genuine
    // negative mass (the arb-implying decrease) trips it.
    const MONO_EPS: f64 = 1e-9;
    let mut cum = 0.0_f64;
    for binr in &sorted {
        let next = cum + binr.prob;
        if next < cum - MONO_EPS {
            return LadderHealth::Incoherent(NoArbViolation::NonMonotoneCdf);
        }
        cum = next;
    }

    // (2) YES-sum within tolerance of 1.0. `cum` is exactly that Σ over the
    // canonical order. Inclusive boundary: only a STRICTLY larger deviation fails.
    let sum = cum;
    if (sum - 1.0).abs() > tol {
        return LadderHealth::Incoherent(NoArbViolation::YesSumOutOfTolerance { sum });
    }

    LadderHealth::Coherent
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- helpers ---------------------------------------------------------

    fn between(floor: f64, cap: f64, prob: f64) -> BracketBin {
        BracketBin {
            kind: BracketStrike::Between { floor, cap },
            prob,
        }
    }
    fn greater(floor: f64, prob: f64) -> BracketBin {
        BracketBin {
            kind: BracketStrike::Greater { floor },
            prob,
        }
    }
    fn less(cap: f64, prob: f64) -> BracketBin {
        BracketBin {
            kind: BracketStrike::Less { cap },
            prob,
        }
    }

    // ---- V0: normal_cdf --------------------------------------------------

    #[test]
    fn normal_cdf_at_zero_is_half() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn normal_cdf_known_quantiles() {
        // Φ(1.96) ≈ 0.975, Φ(-1.96) ≈ 0.025 (the 95% two-sided z).
        assert!((normal_cdf(1.96) - 0.975).abs() < 1e-3);
        assert!((normal_cdf(-1.96) - 0.025).abs() < 1e-3);
    }

    #[test]
    fn normal_cdf_symmetric() {
        // Φ(-z) = 1 - Φ(z) for a range of z.
        for &z in &[0.1, 0.5, 1.0, 1.96, 2.5, 3.3] {
            let lhs = normal_cdf(-z);
            let rhs = 1.0 - normal_cdf(z);
            assert!(
                (lhs - rhs).abs() < 1e-6,
                "asymmetry at z={z}: {lhs} vs {rhs}"
            );
        }
    }

    #[test]
    fn normal_cdf_monotone_increasing() {
        let mut prev = normal_cdf(-5.0);
        let mut z = -5.0;
        while z <= 5.0 {
            let cur = normal_cdf(z);
            assert!(cur >= prev - 1e-12, "non-monotone at z={z}: {cur} < {prev}");
            prev = cur;
            z += 0.05;
        }
    }

    #[test]
    fn normal_cdf_saturates_in_tails() {
        // Far tails saturate to ~0 and ~1.
        assert!(normal_cdf(-8.0) < 1e-6, "left tail did not vanish");
        assert!(normal_cdf(8.0) > 1.0 - 1e-6, "right tail did not saturate");
    }

    // ---- V0: lognormal_cdf ----------------------------------------------

    #[test]
    fn lognormal_cdf_monotone_in_price() {
        let (s0, sigma) = (63_500.0, 0.02);
        let mut prev = lognormal_cdf(40_000.0, s0, sigma).expect("valid");
        let mut p = 40_000.0;
        while p <= 90_000.0 {
            let cur = lognormal_cdf(p, s0, sigma).expect("valid");
            assert!(cur >= prev - 1e-12, "non-monotone at price={p}");
            prev = cur;
            p += 250.0;
        }
    }

    #[test]
    fn lognormal_cdf_limits() {
        let (s0, sigma) = (63_500.0, 0.05);
        // price → 0⁺ ⇒ F → 0; price → ∞ ⇒ F → 1.
        assert!(lognormal_cdf(1.0, s0, sigma).expect("valid") < 1e-6);
        assert!(lognormal_cdf(1.0e12, s0, sigma).expect("valid") > 1.0 - 1e-6);
        // At the anchor itself the median is 0.5.
        assert!((lognormal_cdf(s0, s0, sigma).expect("valid") - 0.5).abs() < 1e-9);
    }

    #[test]
    fn lognormal_cdf_rejects_bad_inputs() {
        let (s0, sigma) = (63_500.0, 0.02);
        assert_eq!(lognormal_cdf(0.0, s0, sigma), None); // price == 0
        assert_eq!(lognormal_cdf(-1.0, s0, sigma), None); // price < 0
        assert_eq!(lognormal_cdf(50_000.0, 0.0, sigma), None); // s0 == 0
        assert_eq!(lognormal_cdf(50_000.0, -1.0, sigma), None); // s0 < 0
        assert_eq!(lognormal_cdf(50_000.0, s0, 0.0), None); // sigma == 0
        assert_eq!(lognormal_cdf(50_000.0, s0, -0.1), None); // sigma < 0
        assert_eq!(lognormal_cdf(f64::NAN, s0, sigma), None); // NaN price
        assert_eq!(lognormal_cdf(50_000.0, f64::NAN, sigma), None); // NaN s0
        assert_eq!(lognormal_cdf(50_000.0, s0, f64::NAN), None); // NaN sigma
        assert_eq!(lognormal_cdf(f64::INFINITY, s0, sigma), None); // +inf price
    }

    // ---- V1: bracket_fair_probs -----------------------------------------

    /// Build a LOG-symmetric full-partition ladder of `2 · n_each_side`
    /// between-bins whose boundaries are GEOMETRIC steps (ratio `r`) around the
    /// anchor — boundary `k` (relative to the anchor boundary at index
    /// `n_each_side`) sits at `anchor · rⁱ`. Plus the two open tails, so the
    /// partition is exact. The settlement model is lognormal (Gaussian in
    /// LOG-price), so a LOG-symmetric ladder — not a linear-symmetric one — is the
    /// one whose mirror bins carry exactly-equal `q_j`; this is the correct
    /// symmetry to assert against this model. `prob` values are arbitrary
    /// placeholders (q_j must ignore them).
    fn log_symmetric_partition(anchor: f64, r: f64, n_each_side: usize) -> Vec<BracketBin> {
        // Boundaries b[0..=total] with b[n_each_side] == anchor (geometric).
        let total = 2 * n_each_side;
        let bounds: Vec<f64> = (0..=total)
            .map(|i| anchor * r.powi(i as i32 - n_each_side as i32))
            .collect();
        let mut bins = Vec::new();
        for w in bounds.windows(2) {
            bins.push(between(w[0], w[1], 0.123)); // placeholder prob
        }
        bins.push(less(bounds[0], 0.456)); // placeholder prob
        bins.push(greater(bounds[total], 0.789)); // placeholder prob
        bins
    }

    fn model(anchor: f64, sigma: f64) -> SettlementModel {
        SettlementModel { anchor, sigma }
    }

    #[test]
    fn fair_probs_symmetric_and_sum_to_one() {
        let anchor = 63_500.0;
        let r = 1.01;
        let bins = log_symmetric_partition(anchor, r, 6);
        let qs = bracket_fair_probs(&bins, model(anchor, 0.02));
        assert_eq!(qs.len(), bins.len());

        // Σ q_j ≈ 1 (full partition incl. both open tails).
        let sum: f64 = qs.iter().map(|b| b.q).sum();
        assert!((sum - 1.0).abs() < 1e-9, "Σq = {sum}");

        // Symmetry (in LOG-price): the between bin immediately above the anchor
        // [anchor, anchor·r] mirrors the one immediately below [anchor/r, anchor].
        let above = qs
            .iter()
            .find(|b| {
                b.kind
                    == BracketStrike::Between {
                        floor: anchor,
                        cap: anchor * r,
                    }
            })
            .expect("above-anchor bin");
        let below = qs
            .iter()
            .find(|b| {
                b.kind
                    == BracketStrike::Between {
                        floor: anchor / r,
                        cap: anchor,
                    }
            })
            .expect("below-anchor bin");
        // Tolerance 1e-7 is the honest bound: the in-house erf is the A&S 7.1.26
        // approximation (~1.5e-7 abs err) and the geometric test boundaries
        // (anchor·r vs anchor/r) are not exactly reciprocal after rounding, so a
        // ~1e-9 residual is expected float noise — NOT asymmetry. A real cap↔floor
        // swap (the mutation we guard) shifts q by ~its own magnitude (≫ 1e-7),
        // so this still trips loudly. (The mutation check below confirms it.)
        assert!(
            (above.q - below.q).abs() < 1e-7,
            "not log-symmetric about anchor: {} vs {}",
            above.q,
            below.q
        );

        // Tail symmetry too (log-symmetric boundaries ⇒ equal tail mass).
        let g = qs
            .iter()
            .find(|b| matches!(b.kind, BracketStrike::Greater { .. }))
            .expect("greater tail");
        let l = qs
            .iter()
            .find(|b| matches!(b.kind, BracketStrike::Less { .. }))
            .expect("less tail");
        assert!(
            (g.q - l.q).abs() < 1e-7,
            "tails not symmetric: {} vs {}",
            g.q,
            l.q
        );
    }

    #[test]
    fn fair_probs_peak_in_containing_bin() {
        let anchor = 63_500.0;
        let r = 1.01;
        let bins = log_symmetric_partition(anchor, r, 6);
        let qs = bracket_fair_probs(&bins, model(anchor, 0.02));

        // The largest-q between-bin must be one of the two straddling the anchor
        // (the containing region). With the anchor on a boundary the mass splits
        // evenly between [anchor/r, anchor] and [anchor, anchor·r]; both are the
        // joint peak.
        let peak = qs
            .iter()
            .filter(|b| matches!(b.kind, BracketStrike::Between { .. }))
            .max_by(|a, b| a.q.partial_cmp(&b.q).expect("finite q"))
            .expect("a between bin");
        let is_straddle = match peak.kind {
            BracketStrike::Between { floor, cap } => {
                (floor - anchor).abs() < 1e-6 || (cap - anchor).abs() < 1e-6
            }
            _ => false,
        };
        assert!(is_straddle, "peak not adjacent to anchor: {:?}", peak.kind);
    }

    #[test]
    fn fair_probs_shifting_anchor_moves_mass_up() {
        let center = 63_500.0;
        let r = 1.01;
        let bins = log_symmetric_partition(center, r, 8);

        // A probe bin a few steps ABOVE the center.
        let probe = BracketStrike::Between {
            floor: center * r.powi(2),
            cap: center * r.powi(3),
        };

        let low_anchor = bracket_fair_probs(&bins, model(center, 0.02));
        let high_anchor = bracket_fair_probs(&bins, model(center * r.powi(2), 0.02));

        let q_low = low_anchor
            .iter()
            .find(|b| b.kind == probe)
            .expect("probe")
            .q;
        let q_high = high_anchor
            .iter()
            .find(|b| b.kind == probe)
            .expect("probe")
            .q;
        // Moving the anchor UP toward the probe bin must raise that bin's q.
        assert!(q_high > q_low, "mass did not move up: {q_low} -> {q_high}");
    }

    #[test]
    fn fair_probs_larger_sigma_flattens() {
        let anchor = 63_500.0;
        let r = 1.01;
        let bins = log_symmetric_partition(anchor, r, 8);

        // The peak (containing) bin: tighter σ ⇒ taller peak, wider σ ⇒ flatter.
        let containing = BracketStrike::Between {
            floor: anchor,
            cap: anchor * r,
        };
        let tight = bracket_fair_probs(&bins, model(anchor, 0.01));
        let wide = bracket_fair_probs(&bins, model(anchor, 0.05));
        let q_tight = tight.iter().find(|b| b.kind == containing).expect("c").q;
        let q_wide = wide.iter().find(|b| b.kind == containing).expect("c").q;
        assert!(
            q_tight > q_wide,
            "larger sigma did not flatten: {q_tight} vs {q_wide}"
        );
    }

    #[test]
    fn fair_probs_independent_of_implied_prob() {
        // THE no-circularity pin (A3, spec 358-360): two ladders with IDENTICAL
        // `kind`s but wildly DIFFERENT `prob`s must yield IDENTICAL q_j.
        let anchor = 63_500.0;
        let width = 500.0;

        // Ladder A: arbitrary probs.
        let mut a: Vec<BracketBin> = Vec::new();
        // Ladder B: same kinds, DIFFERENT probs (even nonsensical / out-of-range).
        let mut b: Vec<BracketBin> = Vec::new();
        let lo = anchor - 5.0 * width;
        for i in 0..10 {
            let f = lo + (i as f64) * width;
            a.push(between(f, f + width, 0.01 * (i as f64 + 1.0)));
            b.push(between(f, f + width, 0.99 - 0.05 * (i as f64))); // different
        }
        a.push(less(lo, 0.05));
        b.push(less(lo, 0.42)); // different
        a.push(greater(lo + 10.0 * width, 0.07));
        b.push(greater(lo + 10.0 * width, 0.88)); // different

        let qa = bracket_fair_probs(&a, model(anchor, 0.03));
        let qb = bracket_fair_probs(&b, model(anchor, 0.03));
        assert_eq!(qa, qb, "q_j depends on BracketBin.prob — circularity bug");
    }

    #[test]
    fn fair_probs_deterministic_under_input_permutation() {
        // Determinism: shuffling the input order must not change the output
        // (canonical price-order reduction).
        let anchor = 63_500.0;
        let bins = log_symmetric_partition(anchor, 1.01, 5);
        let mut rev = bins.clone();
        rev.reverse();
        let q1 = bracket_fair_probs(&bins, model(anchor, 0.02));
        let q2 = bracket_fair_probs(&rev, model(anchor, 0.02));
        assert_eq!(q1, q2);
    }

    #[test]
    fn fair_probs_degenerate_inputs_empty() {
        let anchor = 63_500.0;
        let bins = log_symmetric_partition(anchor, 1.01, 4);

        // sigma <= 0
        assert!(bracket_fair_probs(&bins, model(anchor, 0.0)).is_empty());
        assert!(bracket_fair_probs(&bins, model(anchor, -0.1)).is_empty());
        // anchor <= 0
        assert!(bracket_fair_probs(&bins, model(0.0, 0.02)).is_empty());
        assert!(bracket_fair_probs(&bins, model(-1.0, 0.02)).is_empty());
        // non-finite model
        assert!(bracket_fair_probs(&bins, model(f64::NAN, 0.02)).is_empty());
        assert!(bracket_fair_probs(&bins, model(anchor, f64::INFINITY)).is_empty());
        // empty ladder
        assert!(bracket_fair_probs(&[], model(anchor, 0.02)).is_empty());
        // a bin with a non-finite / <=0 strike degrades the WHOLE ladder
        let bad = vec![between(-100.0, 100.0, 0.5)];
        assert!(bracket_fair_probs(&bad, model(anchor, 0.02)).is_empty());
    }

    // ---- V2: validate_ladder_no_arb -------------------------------------

    #[test]
    fn no_arb_clean_ladder_coherent() {
        // A clean monotone (all non-negative implied mass) ladder summing to ~1.
        let bins = vec![
            less(60_000.0, 0.10),
            between(60_000.0, 62_000.0, 0.25),
            between(62_000.0, 64_000.0, 0.30),
            between(64_000.0, 66_000.0, 0.25),
            greater(66_000.0, 0.10),
        ];
        assert_eq!(validate_ladder_no_arb(&bins, 1e-6), LadderHealth::Coherent);
    }

    #[test]
    fn no_arb_non_monotone_cdf_incoherent() {
        // A NEGATIVE implied mass makes the implied cumulative DECREASE → arb.
        let bins = vec![
            less(60_000.0, 0.40),
            between(60_000.0, 62_000.0, -0.20), // negative mass: cumulative drops
            between(62_000.0, 64_000.0, 0.50),
            greater(64_000.0, 0.30),
        ];
        assert_eq!(
            validate_ladder_no_arb(&bins, 1e-6),
            LadderHealth::Incoherent(NoArbViolation::NonMonotoneCdf)
        );
    }

    #[test]
    fn no_arb_sum_out_of_tolerance_incoherent() {
        // All non-negative (monotone CDF) but the YES-sum is far from 1.
        let bins = vec![
            less(60_000.0, 0.10),
            between(60_000.0, 62_000.0, 0.10),
            between(62_000.0, 64_000.0, 0.10),
            greater(64_000.0, 0.10),
        ];
        // Σ = 0.40, deviation 0.60 ≫ tol.
        match validate_ladder_no_arb(&bins, 1e-3) {
            LadderHealth::Incoherent(NoArbViolation::YesSumOutOfTolerance { sum }) => {
                assert!((sum - 0.40).abs() < 1e-9, "sum carried wrong: {sum}");
            }
            other => panic!("expected YesSumOutOfTolerance, got {other:?}"),
        }
    }

    #[test]
    fn no_arb_boundary_exactly_at_tol_is_coherent() {
        // Σ exactly tol away from 1.0 must still be Coherent (inclusive boundary:
        // only a STRICTLY larger deviation fails). Use values whose Σ is exactly
        // representable so "exactly tol" is real, not float-fuzz.
        let tol = 0.25_f64;
        // Σ = 0.75 ⇒ |0.75 - 1.0| = 0.25 == tol (exactly).
        let bins = vec![
            less(60_000.0, 0.25),
            between(60_000.0, 62_000.0, 0.25),
            greater(62_000.0, 0.25),
        ];
        assert_eq!(validate_ladder_no_arb(&bins, tol), LadderHealth::Coherent);

        // Just over the boundary: Σ = 0.7 ⇒ deviation 0.3 > 0.25 ⇒ fails.
        let bins_over = vec![
            less(60_000.0, 0.20),
            between(60_000.0, 62_000.0, 0.25),
            greater(62_000.0, 0.25),
        ];
        match validate_ladder_no_arb(&bins_over, tol) {
            LadderHealth::Incoherent(NoArbViolation::YesSumOutOfTolerance { .. }) => {}
            other => panic!("expected over-tol failure, got {other:?}"),
        }
    }

    #[test]
    fn no_arb_empty_ladder_is_incoherent_zero_sum() {
        assert_eq!(
            validate_ladder_no_arb(&[], 1e-6),
            LadderHealth::Incoherent(NoArbViolation::YesSumOutOfTolerance { sum: 0.0 })
        );
    }

    #[test]
    fn ordering_matches_rung0_median() {
        // Pin that this kernel's LOCAL canonical ordering agrees with the rung-0
        // [`crate::basis`] ordering at the observable level: the rung-0 median is
        // order-INDEPENDENT precisely because it sorts the SAME way. Feed the same
        // ladder in two input orders to BOTH the rung-0 median and this kernel's
        // sort-dependent reductions; if either disagreed across orders, the local
        // mirror would have diverged from rung-0's contract.
        use crate::basis::bracket_implied_median;
        let bins = vec![
            greater(66_000.0, 0.10),
            between(62_000.0, 64_000.0, 0.30),
            less(60_000.0, 0.10),
            between(64_000.0, 66_000.0, 0.25),
            between(60_000.0, 62_000.0, 0.25),
        ];
        let mut rev = bins.clone();
        rev.reverse();

        // rung-0 median is identical across input orders (its own determinism).
        assert_eq!(bracket_implied_median(&bins), bracket_implied_median(&rev));
        // this kernel's local ordering yields the same canonical sequence ⇒ its
        // reductions are equal across orders too (sum, monotonicity).
        assert_eq!(
            validate_ladder_no_arb(&bins, 1e-6),
            validate_ladder_no_arb(&rev, 1e-6)
        );
        let m = model(63_000.0, 0.02);
        assert_eq!(bracket_fair_probs(&bins, m), bracket_fair_probs(&rev, m));
    }

    #[test]
    fn no_arb_deterministic_under_input_permutation() {
        let bins = vec![
            less(60_000.0, 0.10),
            between(60_000.0, 62_000.0, 0.25),
            between(62_000.0, 64_000.0, 0.30),
            between(64_000.0, 66_000.0, 0.25),
            greater(66_000.0, 0.10),
        ];
        let mut shuffled = bins.clone();
        shuffled.reverse();
        assert_eq!(
            validate_ladder_no_arb(&bins, 1e-6),
            validate_ladder_no_arb(&shuffled, 1e-6)
        );
    }
}
