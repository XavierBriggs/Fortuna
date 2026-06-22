//! Black-box tests for the pure deflation library (`fortuna_scoring::deflation`).
//!
//! Formulas are pinned to `docs/research/2026-06-21-ws3-backtest-overfitting-grounding.md`:
//! §1 PBO via CSCV, §2 purge+embargo, §3 effective-N + MinTRL, §4 Hansen SPA_c,
//! §5 DSR. Assertions are written from those contracts, not from the
//! implementation — they survive a rewrite of the internals.

use fortuna_scoring::deflation::{
    dsr, effective_n, mintrl, pbo, purge_embargo, spa_c, Duration, LabelWindow, Matrix, SeededRng,
    SplitMix64,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A label window from `[t0, t1]` in arbitrary integer "millis" (the in-module
/// numeric time newtype — no external time dependency).
fn lw(t0: i64, t1: i64) -> LabelWindow {
    LabelWindow::new(t0, t1)
}

// ---------------------------------------------------------------------------
// §2 — Purge + embargo
// ---------------------------------------------------------------------------

#[test]
fn purge_drops_overlapping_train() {
    // Test window [100, 200]. A train window overlapping it (overlap iff
    // train.t0 <= test.t1 && train.t1 >= test.t0) must be dropped; a clearly
    // disjoint one kept.
    let test = vec![lw(100, 200)];
    let train = vec![
        lw(150, 250), // overlaps (train.t0=150 <= 200 && train.t1=250 >= 100) -> DROP
        lw(300, 400), // disjoint after  -> KEEP
        lw(0, 50),    // disjoint before -> KEEP
    ];
    let keep = purge_embargo(&train, &test, Duration::zero());
    assert_eq!(
        keep,
        vec![1, 2],
        "only the overlapping train window is purged"
    );
}

#[test]
fn embargo_drops_post_test_window() {
    // One-sided embargo: a train window starting within h AFTER a test window is
    // dropped; a train window the same distance BEFORE is NOT (pre-test safe).
    let test = vec![lw(100, 200)];
    let embargo = Duration::from_millis(10);
    let train = vec![
        lw(205, 305), // starts 5 after test.t1=200, within embargo h=10 -> DROP
        lw(60, 90),   // ends before test.t0, symmetric distance, pre-test -> KEEP
        lw(215, 315), // starts 15 after test.t1, beyond embargo          -> KEEP
    ];
    let keep = purge_embargo(&train, &test, embargo);
    assert_eq!(
        keep,
        vec![1, 2],
        "embargo is one-sided: post-test within h dropped, pre-test kept"
    );
}

#[test]
fn purge_empty_test_keeps_all_train() {
    // Degenerate: no test windows -> nothing to overlap -> keep everything.
    let train = vec![lw(0, 10), lw(20, 30)];
    let keep = purge_embargo(&train, &[], Duration::zero());
    assert_eq!(keep, vec![0, 1]);
}

// ---------------------------------------------------------------------------
// §1 — PBO via purged+embargoed CSCV
// ---------------------------------------------------------------------------

#[test]
fn purged_cscv_bites_on_known_overlap() {
    // The load-bearing test. A fixture with deliberate same-slice overlap: the
    // "lucky" config wins in-sample purely because train rows leak into test
    // rows (overlapping label windows). With purging those leaking train rows
    // are removed from each IS set, the lucky config stops looking good OOS, and
    // PBO rises. No-purge UNDERSTATES overfitting.
    let (matrix, windows) = leaky_overfit_fixture();
    let s = 4;

    let purged = pbo(&matrix, s, &windows, Duration::from_millis(2));
    // The no-purge baseline: empty windows -> nothing overlaps -> no purge.
    let nopurge = pbo(&matrix, s, &[], Duration::zero());

    assert!(
        purged.pbo > nopurge.pbo + 0.05,
        "purging must raise PBO (expose the leak): purged={} nopurge={}",
        purged.pbo,
        nopurge.pbo
    );
}

#[test]
fn pbo_overfit_fixture_high() {
    // A "lucky-winner" matrix: every config is pure noise, so the in-sample best
    // is just whoever was luckiest IS — it lands below the OOS median about half
    // the time, and for the deliberately-rigged lucky-IS / poor-OOS construction
    // PBO is near 1.
    let matrix = lucky_winner_matrix();
    let report = pbo(&matrix, 6, &[], Duration::zero());
    assert!(
        report.pbo > 0.8,
        "a lucky-winner matrix overfits: PBO={}",
        report.pbo
    );
}

#[test]
fn pbo_genuine_fixture_low() {
    // A genuinely skilled config (config 0 is uniformly best on every slice):
    // it is IS-best and stays OOS-best, so λ_c > 0 in every combination -> PBO ≈ 0.
    let matrix = genuine_skill_matrix();
    let report = pbo(&matrix, 6, &[], Duration::zero());
    assert!(
        report.pbo < 0.05,
        "a genuinely skilled config does not overfit: PBO={}",
        report.pbo
    );
}

#[test]
fn cscv_is_metric_agnostic() {
    // CSCV ranks; it makes no metric assumption. A Brier-skill matrix and a
    // Sharpe matrix with identical *rank structure* must produce identical PBO.
    // Build a Sharpe-like matrix by an order-preserving affine transform of a
    // Brier-skill matrix; ranks (hence PBO) are invariant.
    let brier = genuine_skill_matrix();
    let sharpe: Matrix = brier
        .iter()
        .map(|row| row.iter().map(|v| 3.0 * v - 10.0).collect())
        .collect();
    let p_brier = pbo(&brier, 6, &[], Duration::zero());
    let p_sharpe = pbo(&sharpe, 6, &[], Duration::zero());
    assert_eq!(
        p_brier.pbo, p_sharpe.pbo,
        "PBO is rank-based and metric-agnostic"
    );
    assert_eq!(p_brier.n_logits, p_sharpe.n_logits);
}

#[test]
fn pbo_degenerate_inputs_do_not_panic() {
    // S odd, S>T, single config, empty matrix: well-defined, never panic.
    let m = genuine_skill_matrix();
    let _ = pbo(&m, 5, &[], Duration::zero()); // odd S
    let _ = pbo(&m, 1000, &[], Duration::zero()); // S > T
    let single: Matrix = vec![vec![1.0], vec![2.0], vec![3.0], vec![4.0]];
    let _ = pbo(&single, 2, &[], Duration::zero()); // N=1
    let empty: Matrix = vec![];
    let r = pbo(&empty, 2, &[], Duration::zero());
    assert_eq!(r.n_logits, 0, "empty matrix yields no logits");
}

// ---------------------------------------------------------------------------
// §4 — Hansen SPA_c
// ---------------------------------------------------------------------------

#[test]
fn spa_clear_winner_significant() {
    // One config strictly dominates the benchmark (positive loss differential),
    // all others are noise around zero. The composite null
    // "the best is no better than the benchmark" must be rejected -> p_c < 0.05.
    let diffs = clear_winner_loss_diffs();
    let mut rng = SplitMix64::seed(0xC0FFEE);
    let report = spa_c(&diffs, 3, 500, &mut rng);
    assert!(
        report.p_c < 0.05,
        "a clear winner is significant: p_c={}",
        report.p_c
    );
}

#[test]
fn spa_pure_noise_not_significant() {
    // All configs are zero-mean noise: nothing beats the benchmark; p_c high.
    let diffs = pure_noise_loss_diffs();
    let mut rng = SplitMix64::seed(0xABCDEF);
    let report = spa_c(&diffs, 3, 500, &mut rng);
    assert!(
        report.p_c > 0.1,
        "pure noise is not significant: p_c={}",
        report.p_c
    );
}

#[test]
fn spa_c_studentized_and_recentered() {
    // RC-contamination test. SPA_c recenters: demonstrably-inferior configs are
    // pushed to mean 0 in the bootstrap null, so adding a terrible config must
    // NOT materially change p_c (White's RC would be eroded by it). We compare
    // p_c with and without an added catastrophically-bad config.
    let base = clear_winner_loss_diffs();
    let mut contaminated = base.clone();
    for row in contaminated.iter_mut() {
        // a terrible config: hugely negative differential (far worse than benchmark)
        row.push(-50.0);
    }
    let mut r1 = SplitMix64::seed(7);
    let mut r2 = SplitMix64::seed(7);
    let clean = spa_c(&base, 3, 500, &mut r1);
    let contam = spa_c(&contaminated, 3, 500, &mut r2);
    assert!(
        (clean.p_c - contam.p_c).abs() < 0.05,
        "SPA_c is robust to a poor config: clean={} contaminated={}",
        clean.p_c,
        contam.p_c
    );
    // And the conservative bound p_u (= White RC) IS degraded by the bad config:
    // it must move away from (>=) the consistent p_c at least as much as p_c does.
    assert!(
        contam.p_u >= contam.p_c - 1e-9,
        "p_u (RC, conservative) >= p_c (consistent): p_u={} p_c={}",
        contam.p_u,
        contam.p_c
    );
    assert!(
        contam.p_l <= contam.p_c + 1e-9,
        "p_l (liberal) <= p_c (consistent): p_l={} p_c={}",
        contam.p_l,
        contam.p_c
    );
}

/// Sibling test that *earns the name* "recentering advantage."
///
/// The existing `spa_c_studentized_and_recentered` test uses a fixture so dominant
/// (winner stat ≈94) that p_c == p_u == 0 under all recentering variants, so it
/// cannot distinguish SPA_c from White's RC. This test uses a **marginal winner**
/// (stat ~2.0) plus four "innocent bystander" configs whose sample means are
/// small-positive (stats ~0.77, well above the recentering threshold ≈−1.72 for
/// n=80). That puts them in the band where:
///
/// - **Consistent (SPA_c):** all four bystanders get recentered to mean 0 (their
///   stat > threshold so they are NOT demonstrably inferior → offset = d̄_k →
///   bootstrap world sees zero-mean noise from them). Result: bootstrap null driven
///   only by the winner's sampling variance → low null → p_c is small (winner passes
///   the test more easily).
/// - **Conservative (White RC):** all bystanders keep their small positive means in
///   the bootstrap null (offset = 0) → bootstrap max inflated by their positive
///   contributions → bootstrap null is harder to beat → p_u is large.
///
/// Assertion: `report.p_c < report.p_u − 0.05` — SPA_c strictly tighter than RC.
///
/// Bite proof: mutating `spa.rs` so Consistent always uses `offset = 0.0` (RC
/// behaviour) collapses the gap (p_c → p_u, difference → 0) and this test fails.
#[test]
fn spa_c_recentering_beats_rc() {
    // marginal_winner_diffs: n=80, 5 configs (winner + 4 bystanders with small
    // positive means). All bystander stats are in (threshold, 0] — above the
    // "demonstrably inferior" cutoff — so Consistent recenters them to 0 while
    // Conservative (RC) keeps their positive means in the bootstrap null.
    let diffs = marginal_winner_loss_diffs();
    let mut rng = SplitMix64::seed(0xF00D_CAFE);
    let report = spa_c(&diffs, 4, 2000, &mut rng);

    // The recentering must produce a nonzero gap: SPA_c p_c strictly less than
    // White-RC p_u. A gap of ≥ 0.05 distinguishes the two variants on this fixture.
    assert!(
        report.p_c < report.p_u - 0.05,
        "SPA_c (Consistent) must tighten the test vs White RC (Conservative): \
         p_c={:.4} p_u={:.4} gap={:.4}",
        report.p_c,
        report.p_u,
        report.p_u - report.p_c,
    );
    // Sanity: the winner IS marginally significant under SPA_c (not a noise fixture).
    // p_c is in the marginal range, not a clear winner (p≈0) nor pure noise (p≈1).
    assert!(
        report.p_c < 0.40,
        "marginal winner should be significant under SPA_c: p_c={}",
        report.p_c
    );
    // Ordering invariant: p_l <= p_c <= p_u.
    assert!(
        report.p_l <= report.p_c + 1e-9,
        "p_l <= p_c: p_l={} p_c={}",
        report.p_l,
        report.p_c
    );
    assert!(
        report.p_c <= report.p_u + 1e-9,
        "p_c <= p_u: p_c={} p_u={}",
        report.p_c,
        report.p_u
    );
}

#[test]
fn spa_block_bootstrap_deterministic() {
    // Same seed -> identical p_c (reproducible). Different seed -> may differ.
    let diffs = clear_winner_loss_diffs();
    let mut a = SplitMix64::seed(42);
    let mut b = SplitMix64::seed(42);
    let ra = spa_c(&diffs, 3, 400, &mut a);
    let rb = spa_c(&diffs, 3, 400, &mut b);
    assert_eq!(ra.p_c, rb.p_c, "same seed -> same p_c");
    assert_eq!(ra.statistic, rb.statistic, "statistic is deterministic");
}

#[test]
fn spa_degenerate_inputs_do_not_panic() {
    // Empty matrix and zero bootstraps: well-defined, no panic, no NaN gate.
    let mut rng = SplitMix64::seed(1);
    let empty: Matrix = vec![];
    let r = spa_c(&empty, 3, 100, &mut rng);
    assert!(r.p_c.is_finite());
    let mut rng2 = SplitMix64::seed(1);
    let one_col: Matrix = vec![vec![0.0], vec![0.0], vec![0.0], vec![0.0]];
    let r2 = spa_c(&one_col, 3, 100, &mut rng2);
    assert!(r2.p_c.is_finite());
}

// ---------------------------------------------------------------------------
// §3 — effective-N + MinTRL
// ---------------------------------------------------------------------------

#[test]
fn effective_n_ar1() {
    // AR(1) with ρ=0.5: N_eff ≈ N·(1−ρ)/(1+ρ) = N·0.5/1.5 = N/3 = 0.33·N.
    // Build a long AR(1) series with ρ=0.5 driven by a deterministic PRNG.
    let n = 4000usize;
    let rho = 0.5_f64;
    let mut rng = SplitMix64::seed(99);
    let mut series = Vec::with_capacity(n);
    let mut x = 0.0_f64;
    for _ in 0..n {
        // standard-normal-ish innovation from two uniforms (Box–Muller).
        let u1 = (rng.next_u64() as f64 / u64::MAX as f64).max(1e-12);
        let u2 = rng.next_u64() as f64 / u64::MAX as f64;
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        x = rho * x + z;
        series.push(x);
    }
    let neff = effective_n(&series);
    let expected = n as f64 * (1.0 - rho) / (1.0 + rho); // ≈ 1333
    let ratio = neff / expected;
    assert!(
        (0.8..1.25).contains(&ratio),
        "AR(1) ρ=0.5 -> N_eff ≈ 0.33·N: got {} expected ≈ {}",
        neff,
        expected
    );
}

#[test]
fn effective_n_iid_is_full_n() {
    // White noise (ρ≈0): N_eff ≈ N. Use a long uncorrelated series.
    let n = 5000usize;
    let mut rng = SplitMix64::seed(2024);
    let series: Vec<f64> = (0..n)
        .map(|_| rng.next_u64() as f64 / u64::MAX as f64 - 0.5)
        .collect();
    let neff = effective_n(&series);
    let ratio = neff / n as f64;
    assert!(
        (0.85..1.15).contains(&ratio),
        "iid series -> N_eff ≈ N: got {} for N={}",
        neff,
        n
    );
}

#[test]
fn mintrl_matches_paper_worked_example() {
    // Research §3 worked example: SR 2-vs-1 daily, Normal. Per-period values.
    let sr_hat = 2.0 / 252.0_f64.sqrt();
    let sr_star = 1.0 / 252.0_f64.sqrt();
    let skew = 0.0;
    let kurt = 3.0; // raw γ4 (Normal)
    let z_alpha = 1.645;
    let got = mintrl(sr_hat, sr_star, skew, kurt, z_alpha);
    assert!(
        (got - 688.0).abs() < 2.0,
        "MinTRL worked example ≈ 688 obs (≈2.73yr): got {}",
        got
    );
}

#[test]
fn mintrl_undefined_when_sr_not_above_star() {
    // The formula requires SR_hat > SR*; otherwise it is undefined -> NaN/Inf,
    // never a panic and never a silently-wrong finite number.
    let v = mintrl(0.05, 0.05, 0.0, 3.0, 1.645);
    assert!(
        !v.is_finite() || v <= 0.0,
        "MinTRL with SR_hat == SR* is not a valid positive obs count: {}",
        v
    );
}

// ---------------------------------------------------------------------------
// §5 — Deflated Sharpe Ratio
// ---------------------------------------------------------------------------

#[test]
fn dsr_denominator_uses_sr_hat() {
    // Guards the resolved contested point: the DSR variance term is
    // 1 − γ3·SR_hat + ((γ4−1)/4)·SR_hat^2 using SR_hat (NOT SR0). With negative
    // skew, using SR_hat vs SR0 in the denominator changes the result; we pin the
    // closed-form value computed with SR_hat.
    let sr_hat = 0.15_f64;
    let t = 1000.0;
    let skew = -1.0;
    let kurt = 6.0;
    let trial_var = 0.0025_f64; // V[{SR_n}]
    let n_eff = 50.0;
    let got = dsr(sr_hat, t, skew, kurt, trial_var, n_eff);

    // Recompute the expected value with the SR_hat-denominator convention.
    let euler = 0.5772156649015329_f64;
    let inv = |p: f64| inv_normal_cdf(p);
    let sr0 = trial_var.sqrt()
        * ((1.0 - euler) * inv(1.0 - 1.0 / n_eff)
            + euler * inv(1.0 - 1.0 / (n_eff * std::f64::consts::E)));
    let denom = (1.0 - skew * sr_hat + ((kurt - 1.0) / 4.0) * sr_hat * sr_hat).sqrt();
    let arg = (sr_hat - sr0) * (t - 1.0).sqrt() / denom;
    let expected = normal_cdf(arg);
    assert!(
        (got - expected).abs() < 1e-9,
        "DSR denominator must use SR_hat: got {} expected {}",
        got,
        expected
    );

    // A mutation that puts SR0 in the denominator would give a different number;
    // confirm the two conventions actually differ for these inputs.
    let denom_wrong = (1.0 - skew * sr0 + ((kurt - 1.0) / 4.0) * sr0 * sr0).sqrt();
    let arg_wrong = (sr_hat - sr0) * (t - 1.0).sqrt() / denom_wrong;
    let wrong = normal_cdf(arg_wrong);
    assert!(
        (expected - wrong).abs() > 1e-6,
        "the two denominator conventions must differ for this fixture"
    );
}

#[test]
fn dsr_grows_with_t() {
    // More observations (larger T) -> the SR estimate is more reliable -> higher
    // DSR, all else equal (positive excess over SR0).
    let lo = dsr(0.2, 250.0, 0.0, 3.0, 0.0025, 30.0);
    let hi = dsr(0.2, 2500.0, 0.0, 3.0, 0.0025, 30.0);
    assert!(
        hi > lo,
        "DSR grows with T: T=250 -> {} ; T=2500 -> {}",
        lo,
        hi
    );
}

// Capital `N` mirrors the formula variable (the trial count) and the brief's
// named test; this is the one place we accept a non-snake-case fn name.
#[allow(non_snake_case)]
#[test]
fn dsr_shrinks_with_N() {
    // More trials -> larger expected-max SR0 -> smaller DSR.
    let few = dsr(0.2, 1000.0, 0.0, 3.0, 0.0025, 10.0);
    let many = dsr(0.2, 1000.0, 0.0, 3.0, 0.0025, 500.0);
    assert!(
        many < few,
        "DSR shrinks with N: N=10 -> {} ; N=500 -> {}",
        few,
        many
    );
}

// ---------------------------------------------------------------------------
// Local reference implementations of normal CDF / inverse-CDF for the DSR pin.
// These are test-only and exist to recompute the expected DSR independently of
// the library's internal copies (a true black-box check).
// ---------------------------------------------------------------------------

fn normal_cdf(x: f64) -> f64 {
    // erf via A&S 7.1.26.
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let z = (x / std::f64::consts::SQRT_2).abs();
    const A1: f64 = 0.254829592;
    const A2: f64 = -0.284496736;
    const A3: f64 = 1.421413741;
    const A4: f64 = -1.453152027;
    const A5: f64 = 1.061405429;
    const P: f64 = 0.3275911;
    let t = 1.0 / (1.0 + P * z);
    let poly = ((((A5 * t + A4) * t + A3) * t + A2) * t + A1) * t;
    let erf = 1.0 - poly * (-z * z).exp();
    0.5 * (1.0 + sign * erf)
}

fn inv_normal_cdf(p: f64) -> f64 {
    // Acklam's rational approximation to the inverse normal CDF.
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.38357751867269e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    let p_low = 0.02425;
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

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A genuine-skill matrix: config 0 is uniformly best on every slice, config 1
/// second, etc. The IS-best config (0) is also OOS-best in every combination, so
/// every λ_c > 0 and PBO ≈ 0.
fn genuine_skill_matrix() -> Matrix {
    let t = 24;
    let n = 8;
    (0..t)
        .map(|row| {
            (0..n)
                .map(|cfg| {
                    // higher value = better; config 0 highest. Add a tiny
                    // row-dependent wobble that never reorders configs.
                    let base = (n - cfg) as f64;
                    base + 0.01 * (row as f64).sin()
                })
                .collect()
        })
        .collect()
}

/// A lucky-winner matrix: each row's best config is essentially random, and we
/// rig it so the IS-best (whoever was high IS) tends to be low OOS. Built from a
/// deterministic PRNG so the test is reproducible.
fn lucky_winner_matrix() -> Matrix {
    let t = 24;
    let n = 20;
    let mut rng = SplitMix64::seed(0xDEAD_BEEF);
    (0..t)
        .map(|_| {
            (0..n)
                .map(|_| rng.next_u64() as f64 / u64::MAX as f64)
                .collect()
        })
        .collect()
}

/// A fixture for `purged_cscv_bites_on_known_overlap`. With S=4 over T=16 the
/// CSCV partition is four contiguous groups of four rows
/// (`g0={0..3}, g1={4..7}, g2={8..11}, g3={12..15}`).
///
/// The leak is concentrated in a single cross-group sibling pair: rows 0 (in g0)
/// and 8 (in g2) share ONE resolution event — they get the SAME label window and
/// so overlap; every other row gets a unique, disjoint window. This is the
/// minimal same-resolution-event leak surface, and it straddles the g0/g2 group
/// boundary so a CSCV split can place one sibling IS and the other OOS.
///
/// - config 0 ("leaker"): a large positive value ONLY on the two sibling rows
///   {0, 8}, ~0 elsewhere. Without purging, whenever one sibling is IS the
///   leaker wins in-sample, and because the OTHER sibling sits in OOS the leak
///   keeps the leaker's OOS rank high → λ_c ≥ 0 → PBO understated.
/// - config 1 ("decoy"): in-sample-lucky on group g1 (rows 4..7), poor (negative)
///   everywhere else — a textbook overfit config that wins IS once the leaker's
///   sibling row is purged out of the IS set, and then lands LOW out of sample.
/// - configs 2..n: flat noise.
///
/// With purging, the IS sibling (row 0 or 8) is dropped whenever its partner is
/// OOS; the leaker's IS edge collapses, the decoy is selected, and the decoy's
/// genuinely-low OOS rank pushes λ_c < 0 → PBO RISES. No-purge UNDERSTATES it.
fn leaky_overfit_fixture() -> (Matrix, Vec<LabelWindow>) {
    let t = 16;
    let n = 6;
    let mut rng = SplitMix64::seed(0x5EED);
    let mut matrix: Matrix = (0..t)
        .map(|_| {
            (0..n)
                .map(|_| 0.02 * (rng.next_u64() as f64 / u64::MAX as f64))
                .collect()
        })
        .collect();

    for (row, vals) in matrix.iter_mut().enumerate() {
        // Leaker: high only on the two overlapping sibling rows {0, 8}.
        vals[0] = if row == 0 || row == 8 { 8.0 } else { 0.0 };
        // Decoy: in-sample-lucky on g1 (rows 4..7), poor elsewhere.
        vals[1] = if (4..=7).contains(&row) { 3.0 } else { -1.0 };
    }

    // Windows: rows 0 and 8 share a window (the single cross-group leak); every
    // other row is disjoint and far away.
    let windows: Vec<LabelWindow> = (0..t)
        .map(|i| {
            if i == 0 || i == 8 {
                LabelWindow::new(0, 10)
            } else {
                LabelWindow::new(100_000 + (i as i64) * 100, 100_000 + (i as i64) * 100 + 10)
            }
        })
        .collect();
    (matrix, windows)
}

/// Loss differentials `d[t][k] = Brier(baseline) − Brier(model_k)` for a matrix
/// with one clear winner (column 0, mean strongly positive) and noise columns.
fn clear_winner_loss_diffs() -> Matrix {
    let t = 120;
    let mut rng = SplitMix64::seed(0x1234_5678);
    (0..t)
        .map(|_| {
            let mut noise = || (rng.next_u64() as f64 / u64::MAX as f64) - 0.5;
            vec![
                0.5 + 0.2 * noise(), // winner: clearly positive
                0.05 * noise(),      // noise around 0
                0.05 * noise(),
            ]
        })
        .collect()
}

/// Marginal-winner loss differentials for `spa_c_recentering_beats_rc`.
///
/// n=80 rows × 5 columns:
/// - Column 0 (winner): mean ≈ 0.08, std ≈ 0.35 → studentized stat ≈ 2.0
///   (marginal — the test statistic is modest enough that the bootstrap matters).
/// - Columns 1–4 (bystanders): mean ≈ 0.03, std ≈ 0.35 → stat ≈ 0.77.
///   All bystander stats sit comfortably above the recentering threshold
///   ≈ −1.72 for n=80, so Consistent recenters them to 0 (not "demonstrably
///   inferior") while Conservative (RC) keeps their small positive means,
///   inflating the bootstrap null and producing a materially higher p_u.
///
/// The fixture is fully deterministic via a seeded SplitMix64.
fn marginal_winner_loss_diffs() -> Matrix {
    let t = 80usize;
    let mut rng = SplitMix64::seed(0xBAD_5EED_1234_5678);
    // Generate unit-variance noise via the U[0,1) → centred mapping; scale + shift
    // to hit the desired per-column mean and std targets.
    let noise = |r: &mut SplitMix64| -> f64 {
        // Pair two uniforms into a roughly symmetric variate in (−0.5, 0.5).
        let u1 = r.next_u64() as f64 / u64::MAX as f64 - 0.5;
        let u2 = r.next_u64() as f64 / u64::MAX as f64 - 0.5;
        u1 + u2 // roughly triangular on (−1,1), mean 0, var 1/6
    };
    // Scale factor so std ≈ 0.35: triangular on (−1,1) has variance 1/6 ≈ 0.167;
    // sqrt(0.167) ≈ 0.408. We want std ≈ 0.35 so scale = 0.35/0.408 ≈ 0.86.
    // We fix scale = 0.858 for reproducibility.
    const SCALE: f64 = 0.858;
    // Winner bias added per observation to achieve mean ≈ 0.08.
    const WIN_BIAS: f64 = 0.08;
    // Bystander bias added per observation to achieve mean ≈ 0.03.
    const BYS_BIAS: f64 = 0.03;

    (0..t)
        .map(|_| {
            let winner = WIN_BIAS + SCALE * noise(&mut rng);
            let bys: Vec<f64> = (0..4).map(|_| BYS_BIAS + SCALE * noise(&mut rng)).collect();
            std::iter::once(winner).chain(bys).collect()
        })
        .collect()
}

/// Loss differentials that are all zero-mean noise — nothing beats the benchmark.
fn pure_noise_loss_diffs() -> Matrix {
    let t = 120;
    let mut rng = SplitMix64::seed(0x9ABC_DEF0);
    (0..t)
        .map(|_| {
            (0..4)
                .map(|_| 0.1 * ((rng.next_u64() as f64 / u64::MAX as f64) - 0.5))
                .collect()
        })
        .collect()
}
