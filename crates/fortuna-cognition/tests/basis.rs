//! `perp_event_basis` BASIS KERNEL tests — written BEFORE the implementation
//! per the TDD doctrine (design: docs/design/perp-strategies-and-scalar-
//! claims.md §3, §3.1; GAPS "TRACK C — slice 3b: PAIRED-CYCLE FIXTURE …
//! basis VALIDATED").
//!
//! What is under test (the DETERMINISTIC comparison logic only — NOT the
//! strategy, NOT the venue DTO, NOT daemon wiring; those are fixture-gated):
//!
//! - `bracket_implied_median`: the BTC price where cumulative implied
//!   probability (YES-mid, normalised) crosses 0.5, linearly interpolated
//!   within the crossing `between` bin. The ladder is the REAL KXBTC partition
//!   of the BTC price axis: a `less` open BOTTOM tail (cap only), the `between`
//!   closed range bins (floor+cap), and a `greater` open TOP tail (floor only).
//!   Each bin carries its YES probability as `f64` (the caller parses the
//!   dollar-strings → `(bid+ask)/2`; the kernel is string-format-agnostic).
//!   If the 0.5 crossing falls in an OPEN tail (`less`/`greater`) there is no
//!   finite width to interpolate, so the median is undefined → `None`.
//!   Structure grounded in the LIVE fixture
//!   (fixtures/kinetics-perps/derived/paired_cycle_btc_perp_vs_kxbtc.json) + the Kalshi
//!   research (asyncapi.yaml:3174-3176 floor/cap, research.md:251-253
//!   strike_type). Synthetic VALUES prove the LOGIC only.
//! - `compute_basis`: `signed_basis = perp_mark_btc_dollars −
//!   bracket_implied_median`, `is_tradeable` iff `|basis| > fee_floor +
//!   min_basis` (the FEE-TRAP rule, amendment C: the floor is a PASSED-IN
//!   assumed post-promo fee, NOT recomputed). The perp mark is BTC-dollars
//!   (the caller's per-contract → BTC ×10000 boundary; the kernel never touches
//!   the money type).
//!
//! Each test states the MUTATION it pins (the change to the kernel that makes
//! it red). The real-orderbook e2e is in `basis_live_fixture.rs` (the live
//! co-recorded paired cycle), NOT here.

use fortuna_cognition::basis::{
    bracket_implied_median, compute_basis, BasisSignal, BracketBin, BracketStrike,
};

// ─── helpers ────────────────────────────────────────────────────────────────

/// A `between` bin from floor/cap (dollars) and a YES probability (0..=1).
fn between(floor: f64, cap: f64, prob: f64) -> BracketBin {
    BracketBin {
        kind: BracketStrike::Between { floor, cap },
        prob,
    }
}

/// The open BOTTOM tail: P(BTC ≤ cap). No finite width to interpolate.
fn less(cap: f64, prob: f64) -> BracketBin {
    BracketBin {
        kind: BracketStrike::Less { cap },
        prob,
    }
}

/// The open TOP tail: P(BTC > floor). No finite width to interpolate.
fn greater(floor: f64, prob: f64) -> BracketBin {
    BracketBin {
        kind: BracketStrike::Greater { floor },
        prob,
    }
}

/// A 5-bin `between` ladder $95k–$100k, $1k bins, each with the given YES
/// probability. Floors ascending; floor < cap per bin.
fn ladder(probs: [f64; 5]) -> Vec<BracketBin> {
    let floors = [95_000.0, 96_000.0, 97_000.0, 98_000.0, 99_000.0];
    floors
        .iter()
        .zip(probs.iter())
        .map(|(&f, &p)| between(f, f + 1_000.0, p))
        .collect()
}

// A uniform p=0.20 per bin → sum_p = 1.0 (already normalised).
const UNIFORM_20: [f64; 5] = [0.20, 0.20, 0.20, 0.20, 0.20];

// ─── 1. Nominal: interpolated median in a `between` bin + signed basis ───────

// Uniform p=0.2 ladder: cum reaches 0.4 after bins 0,1; bin 2 ([97000,98000])
// is the crossing bin → median = 97000 + (0.5−0.4)/0.2 · 1000 = 97500.
// perp_mark = $97,600 (inside bin 2) → signed_basis = +100.
//
// MUTATION: change the `0.5` crossing target to `0.0` → the very first bin
// crosses → median collapses to its floor (95000) → this assertion reds.
#[test]
fn nominal_median_and_signed_basis() {
    let bins = ladder(UNIFORM_20);

    let median = bracket_implied_median(&bins).expect("uniform ladder has a median");
    assert!(
        (median - 97_500.0).abs() < 1e-9,
        "implied median should be 97500, got {median}"
    );

    // fee_floor + min_basis = 10 + 5 = 15 << |100|, so tradeable.
    let sig =
        compute_basis(&bins, 97_600.0, 10.0, 5.0).expect("a median exists, so a signal exists");
    assert!((sig.bracket_implied_median - 97_500.0).abs() < 1e-9);
    assert!((sig.perp_mark - 97_600.0).abs() < 1e-9);
    assert!(
        (sig.signed_basis - 100.0).abs() < 1e-9,
        "signed_basis should be +100, got {}",
        sig.signed_basis
    );
    assert!(sig.is_tradeable, "|100| > 15 → tradeable");
}

// ─── 2. Full partition with open tails: median still lands in a `between` ────

// The REAL KXBTC shape: a `less` bottom tail + `between` core + a `greater`
// top tail, with the 0.5 crossing in a `between` bin (the live-fixture case).
// Probs (already summing to 1.0): less .05, between [95k,96k] .15, [96k,97k]
// .25, [97k,98k] .30, [98k,99k] .20, greater(99k) .05.
//   ordered low→high: less .05 → cum .05
//     [95k,96k] .15 → cum .20
//     [96k,97k] .25 → cum .45
//     [97k,98k] .30 → cum .75 (≥.5) ← crossing bin, cum_prev=.45
//   median = 97000 + (.5 − .45)/.30 · 1000 = 97000 + 166.6667 = 97166.6667.
//
// MUTATION: drop the `less`/`greater` tails from the cumulative accumulation
// (treat the partition as the `between` bins alone, re-normalised) → the
// cumulative shifts (the .05 bottom-tail mass disappears) → the crossing math
// changes → this 97166.67 assertion reds.
#[test]
fn full_partition_with_tails_median_in_between() {
    let bins = vec![
        less(95_000.0, 0.05),
        between(95_000.0, 96_000.0, 0.15),
        between(96_000.0, 97_000.0, 0.25),
        between(97_000.0, 98_000.0, 0.30),
        between(98_000.0, 99_000.0, 0.20),
        greater(99_000.0, 0.05),
    ];
    let median = bracket_implied_median(&bins).expect("crossing is in a between bin");
    assert!(
        (median - 97_166.666_666_666_66).abs() < 1e-6,
        "median should be ~97166.667 (crossing in [97k,98k]), got {median}"
    );
}

// ─── 3. Median falls in an OPEN tail → None (cannot interpolate) ─────────────

// Mass piled into the `greater` TOP tail: less .05, three between .10 each,
// greater(99k) .65. Cumulative: less .05 → between .15,.25,.35 → greater +.65
// = 1.0 crosses 0.5 INSIDE the open `greater` tail (cum_prev=.35 < .5 ≤ 1.0).
// An open tail has no finite [floor,cap] to interpolate → the median is
// outside the resolved range → `None` (conservative; no fabricated point).
//
// Symmetric `less`-tail case below: mass in the BOTTOM tail crosses there.
//
// MUTATION: clamp/interpolate the open tail to its single strike (e.g. return
// `Some(floor)` for a `greater` crossing instead of `None`) → these
// `is_none()` assertions red.
#[test]
fn median_in_open_tail_is_none() {
    // Crossing in the GREATER top tail.
    let top_heavy = vec![
        less(95_000.0, 0.05),
        between(95_000.0, 96_000.0, 0.10),
        between(96_000.0, 97_000.0, 0.10),
        between(97_000.0, 98_000.0, 0.10),
        greater(98_000.0, 0.65),
    ];
    assert!(
        bracket_implied_median(&top_heavy).is_none(),
        "0.5 crossing in the open `greater` tail → None (no width to interpolate)"
    );
    // None propagates through compute_basis.
    assert!(
        compute_basis(&top_heavy, 97_000.0, 1.0, 0.0).is_none(),
        "no median → compute_basis None"
    );

    // Crossing in the LESS bottom tail (symmetric): the bottom tail carries the
    // first .65, which already crosses 0.5 at the very first (open) bin.
    let bottom_heavy = vec![
        less(96_000.0, 0.65),
        between(96_000.0, 97_000.0, 0.10),
        between(97_000.0, 98_000.0, 0.10),
        between(98_000.0, 99_000.0, 0.10),
        greater(99_000.0, 0.05),
    ];
    assert!(
        bracket_implied_median(&bottom_heavy).is_none(),
        "0.5 crossing in the open `less` tail → None"
    );
}

// ─── 4. Fee-trap: |basis| below the fee floor is NOT tradeable ──────────────

// median = 97500 (uniform ladder), perp_mark = $97,508 → |basis| = 8.
// fee_floor = 12, min_basis = 0 → 8 < 12 → NOT tradeable, even though the
// basis is non-zero. (Amendment C: promo-$0 never justifies GO; the floor is
// the assumed post-promo fee, passed in.)
//
// MUTATION: drop the fee-floor comparison (e.g. `is_tradeable = basis != 0`)
// → flips to true → this assertion reds.
#[test]
fn fee_trap_below_floor_not_tradeable() {
    let bins = ladder(UNIFORM_20);
    let sig = compute_basis(&bins, 97_508.0, 12.0, 0.0).expect("median exists");
    assert!((sig.signed_basis - 8.0).abs() < 1e-9, "basis should be 8");
    assert!(
        !sig.is_tradeable,
        "|8| < fee_floor 12 → NOT tradeable (fee-trap)"
    );

    // And just past the combined floor it IS tradeable (boundary is strict `>`):
    let sig2 = compute_basis(&bins, 97_513.0, 12.0, 0.0).expect("median exists");
    assert!((sig2.signed_basis - 13.0).abs() < 1e-9);
    assert!(sig2.is_tradeable, "|13| > 12 → tradeable");
}

// ─── 5. Sign direction: perp above median → positive basis ──────────────────

// MUTATION: swap the subtraction to `median − perp_mark` → the sign flips on
// both arms → these assertions red.
#[test]
fn sign_direction_follows_perp_minus_median() {
    let bins = ladder(UNIFORM_20); // median 97500

    // perp ABOVE median → positive basis.
    let above = compute_basis(&bins, 98_000.0, 1.0, 0.0).expect("median");
    assert!(
        above.signed_basis > 0.0,
        "perp above median → positive basis, got {}",
        above.signed_basis
    );
    assert!((above.signed_basis - 500.0).abs() < 1e-9);

    // perp BELOW median → negative basis.
    let below = compute_basis(&bins, 97_000.0, 1.0, 0.0).expect("median");
    assert!(
        below.signed_basis < 0.0,
        "perp below median → negative basis, got {}",
        below.signed_basis
    );
    assert!((below.signed_basis + 500.0).abs() < 1e-9);
}

// ─── 6. Normalization: YES-mids summing to ≠ 1.0 still give a valid median ──

// Probs: .25,.25,.25,.25,.10 → sum_p = 1.10 (≠ 1).
// Normalised p = .2273,.2273,.2273,.2273,.0909. Cumulative (sorted by floor):
//   bin0 cum 0 → +.2273 = .2273 (<.5)
//   bin1       → .4545 (<.5)
//   bin2       → .6818 (≥.5) ← crossing bin [97000,98000]
//   median = 97000 + (.5 − .4545)/.2273 · 1000 = 97000 + 0.2 · 1000 = 97200.
// (0.04545.../0.22727... = exactly 0.2.)
//
// MUTATION: remove the `/ sum_p` normalisation → cumulative uses raw .25 each
// → crossing math shifts (bin1: .25+.25=.50 crosses at its TOP, median 97000)
// → this 97200 assertion reds.
#[test]
fn normalization_handles_probs_summing_above_one() {
    let bins = ladder([0.25, 0.25, 0.25, 0.25, 0.10]);
    let median = bracket_implied_median(&bins).expect("median exists");
    assert!(
        (median - 97_200.0).abs() < 1e-9,
        "normalised median should be 97200, got {median}"
    );
}

// ─── 7. Illiquid bin (prob == 0) → graceful, finite, zero mass ──────────────

// One zero-prob bin in an otherwise-liquid ladder. The illiquid bin must
// contribute ZERO mass (and never divide-by-zero), leaving the median finite
// and at the correct crossing.
// Probs: .2,0,.2,.2,.2, sum=.8 → normalised .25,0,.25,.25,.25.
//   bin0 → cum .25 (<.5)
//   bin1 (p=0, illiquid) → cum .25 (no mass added; must not crash)
//   bin2 → .50 (≥.5) crossing [97000,98000] at cum_prev=.25:
//   median = 97000 + (.5 − .25)/.25 · 1000 = 98000 (the bin's TOP).
//
// MUTATION (reachable): make a (prob==0) bin carry mass (e.g. add a min-floor
// prob, or `prob + epsilon`) → the illiquid bin shifts the cumulative → the
// crossing moves off bin2 → the 98000 assertion reds.
//
// NOTE on the div-by-zero guard specifically: under the `>=` crossing a zero
// bin can NEVER be the crossing bin, so the `/ p` guard is DEFENSIVE and is not
// independently red-able by any synthetic ladder (verified during mutation
// testing; recorded in ASSUMPTIONS). This test pins the REACHABLE property —
// finiteness + correct zero-mass handling.
#[test]
fn illiquid_zero_prob_bin_stays_finite() {
    let bins = ladder([0.20, 0.0, 0.20, 0.20, 0.20]);
    let median = bracket_implied_median(&bins).expect("median exists");
    assert!(median.is_finite(), "median must be finite, got {median}");
    assert!(
        (median - 98_000.0).abs() < 1e-9,
        "crossing-at-top median should be 98000 (zero bin adds no mass), got {median}"
    );
}

// ─── 8. Degenerate ladder: all bins zero-prob → None, not tradeable ─────────

// MUTATION: return `Some(0.0)` (or any value) on sum_p == 0 instead of `None`
// → `bracket_implied_median` would be Some → compute_basis would be Some →
// these `is_none()` assertions red.
#[test]
fn degenerate_all_zero_prob_is_none() {
    let bins = ladder([0.0, 0.0, 0.0, 0.0, 0.0]);
    assert!(
        bracket_implied_median(&bins).is_none(),
        "all-zero ladder has no implied median"
    );
    // None propagates: no median → no basis signal → cannot be tradeable.
    assert!(
        compute_basis(&bins, 97_000.0, 1.0, 0.0).is_none(),
        "no median → compute_basis returns None"
    );
}

// ─── 9. empty slice → None (the `None`-safe contract on no bins) ────────────

// `bracket_implied_median` is `None`-safe on an empty ladder (no panic, no
// NaN). The genuine non-crossing cases are the empty slice (here), the
// all-zero `sum_p == 0` ladder (test 8), and the open-tail crossing (test 3).
#[test]
fn empty_slice_is_none() {
    let bins: Vec<BracketBin> = Vec::new();
    assert!(
        bracket_implied_median(&bins).is_none(),
        "empty ladder → None (no panic, no NaN)"
    );
    assert!(
        compute_basis(&bins, 97_000.0, 1.0, 0.0).is_none(),
        "empty ladder → compute_basis None"
    );
}

// ─── 10. NaN/non-finite prob → degenerate None (guarded sum) ────────────────

// A non-finite probability (NaN/inf can arise from a malformed caller parse)
// must not poison the median into a NaN; the finite-sum guard rejects it.
//
// MUTATION: drop the `!sum_p.is_finite()` guard → sum_p becomes NaN → the
// normalised cumulative is NaN → `cum + p >= 0.5` is always false → the loop
// falls through to `None` anyway in THIS case, BUT a NaN in a single bin amid
// finite mass could yield a NaN median; the guard makes the contract explicit.
// (Pinned: a NaN prob never produces a Some(NaN) median.)
#[test]
fn non_finite_prob_is_none() {
    let bins = vec![
        between(95_000.0, 96_000.0, 0.30),
        between(96_000.0, 97_000.0, f64::NAN),
        between(97_000.0, 98_000.0, 0.30),
    ];
    assert!(
        bracket_implied_median(&bins).is_none(),
        "a non-finite prob → degenerate None, never a NaN median"
    );
}

// ─── determinism: identical ladder → byte-identical result ──────────────────

#[test]
fn determinism_same_ladder_same_result() {
    let bins = ladder([0.20, 0.25, 0.32, 0.12, 0.06]);
    let a = bracket_implied_median(&bins);
    let b = bracket_implied_median(&bins);
    assert_eq!(a, b, "median is a pure function of the ladder");

    let sa = compute_basis(&bins, 97_250.0, 7.0, 3.0);
    let sb = compute_basis(&bins, 97_250.0, 7.0, 3.0);
    assert_eq!(sa, sb, "compute_basis is a pure function of its inputs");
}

// ─── ordering: an UNSORTED ladder is sorted by price position first ─────────

// Same uniform-0.2 ladder as the nominal test but presented out of price
// order, AND with the tails interleaved. The kernel must order by price
// position (less at bottom, between ascending by floor, greater at top) →
// identical median (97500). This pins the ordering step.
//
// MUTATION: remove the price-position ordering → cumulative probability
// accumulates in input order → the crossing bin is wrong → median ≠ 97500 → reds.
#[test]
fn unsorted_ladder_is_ordered_by_price_position() {
    // Deliberately shuffled input order (tails NOT at the ends).
    let bins = vec![
        between(98_000.0, 99_000.0, 0.20),
        between(95_000.0, 96_000.0, 0.20),
        between(99_000.0, 100_000.0, 0.20),
        between(97_000.0, 98_000.0, 0.20),
        between(96_000.0, 97_000.0, 0.20),
    ];
    let median = bracket_implied_median(&bins).expect("median exists");
    assert!(
        (median - 97_500.0).abs() < 1e-9,
        "ordered median should be 97500, got {median}"
    );
}

// ─── construction sanity: BasisSignal fields carry the fee floor through ────

#[test]
fn signal_carries_the_passed_fee_floor() {
    let bins = ladder(UNIFORM_20);
    let sig: BasisSignal = compute_basis(&bins, 97_600.0, 8.0, 2.0).expect("median");
    assert!(
        (sig.fee_floor_dollars - 8.0).abs() < 1e-9,
        "the signal reports the passed-in fee floor"
    );
}
