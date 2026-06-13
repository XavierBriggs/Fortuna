//! `perp_event_basis` BASIS KERNEL tests — written BEFORE the implementation
//! per the TDD doctrine (design: docs/design/perp-strategies-and-scalar-
//! claims.md §3, §3.1; GAPS "TRACK C — slice 3 GROUNDED").
//!
//! What is under test (the DETERMINISTIC comparison logic only — NOT the
//! strategy, NOT the venue DTO, NOT daemon wiring; those are fixture-gated):
//!
//! - `bracket_implied_median`: the strike where cumulative implied probability
//!   (YES-mid, normalised) crosses 0.5, linearly interpolated within the
//!   crossing bin. Synthetic ladder VALUES use the REAL KXBTC15M structure
//!   (floor_strike/cap_strike + a YES bid/ask in cents) — grounded in
//!   docs/research/venue/kalshi-api-2026-06-10 (asyncapi.yaml:1688 ticker,
//!   :3174-3176 floor_strike/cap_strike; research.md:251-253 strike fields).
//! - `compute_basis`: `signed_basis = perp_mark − bracket_implied_median`,
//!   `is_tradeable` iff `|basis| > fee_floor + min_basis` (the FEE-TRAP rule,
//!   amendment C: the floor is a PASSED-IN assumed post-promo fee, NOT
//!   recomputed).
//!
//! Each test states the MUTATION it pins (the change to the kernel that makes
//! it red). Synthetic inputs prove the LOGIC only; the real-orderbook e2e is
//! fixture-gated (operator-queue #4 + the KalshiMarket floor/cap DTO).

use fortuna_cognition::basis::{bracket_implied_median, compute_basis, BasisSignal, BracketBin};
use fortuna_core::perp::PerpPrice;

// ─── helpers ────────────────────────────────────────────────────────────────

/// A bracket bin from a floor/cap (dollars) and a YES bid/ask (cents 0..=100).
fn bin(floor: f64, cap: f64, yes_bid: i64, yes_ask: i64) -> BracketBin {
    BracketBin {
        floor,
        cap,
        yes_bid,
        yes_ask,
    }
}

/// `PerpPrice` from whole dollars (ten-thousandths: $1 = 10_000).
fn perp_dollars(d: i64) -> PerpPrice {
    PerpPrice::new(d * 10_000)
}

/// A 5-bin KXBTC15M-style ladder $95k–$100k, $1k bins, each with the given
/// YES bid/ask in cents. Floors ascending; floor < cap per bin.
fn ladder(spreads: [(i64, i64); 5]) -> Vec<BracketBin> {
    let floors = [95_000.0, 96_000.0, 97_000.0, 98_000.0, 99_000.0];
    floors
        .iter()
        .zip(spreads.iter())
        .map(|(&f, &(b, a))| bin(f, f + 1_000.0, b, a))
        .collect()
}

// A uniform mid of 20 cents → p_i = 0.20 per bin → sum_p = 1.0 (already
// normalised). bid<ask so the spread is realistic.
const UNIFORM_20C: [(i64, i64); 5] = [(19, 21), (19, 21), (19, 21), (19, 21), (19, 21)];

// ─── 1. Nominal: interpolated median + signed basis ─────────────────────────

// Uniform p=0.2 ladder: cum reaches 0.4 after bins 0,1; bin 2 ([97000,98000])
// is the crossing bin → median = 97000 + (0.5−0.4)/0.2 · 1000 = 97500.
// perp_mark = $97,600 (inside bin 2) → signed_basis = +100.
//
// MUTATION: change the `0.5` crossing target to `0.0` → the very first bin
// crosses → median collapses to its floor (95000) → this assertion reds.
#[test]
fn nominal_median_and_signed_basis() {
    let bins = ladder(UNIFORM_20C);

    let median = bracket_implied_median(&bins).expect("uniform ladder has a median");
    assert!(
        (median - 97_500.0).abs() < 1e-9,
        "implied median should be 97500, got {median}"
    );

    // fee_floor + min_basis = 10 + 5 = 15 << |100|, so tradeable.
    let sig = compute_basis(&bins, perp_dollars(97_600), 10.0, 5.0)
        .expect("a median exists, so a signal exists");
    assert!((sig.bracket_implied_median - 97_500.0).abs() < 1e-9);
    assert!((sig.perp_mark - 97_600.0).abs() < 1e-9);
    assert!(
        (sig.signed_basis - 100.0).abs() < 1e-9,
        "signed_basis should be +100, got {}",
        sig.signed_basis
    );
    assert!(sig.is_tradeable, "|100| > 15 → tradeable");
}

// ─── 2. Fee-trap: |basis| below the fee floor is NOT tradeable ──────────────

// median = 97500 (uniform ladder), perp_mark = $97,508 → |basis| = 8.
// fee_floor = 12, min_basis = 0 → 8 < 12 → NOT tradeable, even though the
// basis is non-zero. (Amendment C: promo-$0 never justifies GO; the floor is
// the assumed post-promo fee, passed in.)
//
// MUTATION: drop the fee-floor comparison (e.g. `is_tradeable = basis != 0`)
// → flips to true → this assertion reds.
#[test]
fn fee_trap_below_floor_not_tradeable() {
    let bins = ladder(UNIFORM_20C);
    let sig = compute_basis(&bins, perp_dollars(97_508), 12.0, 0.0).expect("median exists");
    assert!((sig.signed_basis - 8.0).abs() < 1e-9, "basis should be 8");
    assert!(
        !sig.is_tradeable,
        "|8| < fee_floor 12 → NOT tradeable (fee-trap)"
    );

    // And just past the combined floor it IS tradeable (boundary is strict `>`):
    let sig2 = compute_basis(&bins, perp_dollars(97_513), 12.0, 0.0).expect("median exists");
    assert!((sig2.signed_basis - 13.0).abs() < 1e-9);
    assert!(sig2.is_tradeable, "|13| > 12 → tradeable");
}

// ─── 3. Sign direction: perp above median → positive basis ──────────────────

// MUTATION: swap the subtraction to `median − perp_mark` → the sign flips on
// both arms → these assertions red.
#[test]
fn sign_direction_follows_perp_minus_median() {
    let bins = ladder(UNIFORM_20C); // median 97500

    // perp ABOVE median → positive basis.
    let above = compute_basis(&bins, perp_dollars(98_000), 1.0, 0.0).expect("median");
    assert!(
        above.signed_basis > 0.0,
        "perp above median → positive basis, got {}",
        above.signed_basis
    );
    assert!((above.signed_basis - 500.0).abs() < 1e-9);

    // perp BELOW median → negative basis.
    let below = compute_basis(&bins, perp_dollars(97_000), 1.0, 0.0).expect("median");
    assert!(
        below.signed_basis < 0.0,
        "perp below median → negative basis, got {}",
        below.signed_basis
    );
    assert!((below.signed_basis + 500.0).abs() < 1e-9);
}

// ─── 4. Normalization: YES-mids summing to ≠ 1.0 still give a valid median ──

// Mids: 25,25,25,25,10 cents → p = .25,.25,.25,.25,.10, sum_p = 1.10 (≠ 1).
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
    let bins = ladder([(24, 26), (24, 26), (24, 26), (24, 26), (9, 11)]);
    let median = bracket_implied_median(&bins).expect("median exists");
    assert!(
        (median - 97_200.0).abs() < 1e-9,
        "normalised median should be 97200, got {median}"
    );
}

// ─── 5. Illiquid bin (yes_bid==yes_ask==0) → graceful, finite, zero mass ────

// One zero-prob bin in an otherwise-liquid ladder. The illiquid bin must
// contribute ZERO mass (and never divide-by-zero), leaving the median finite
// and at the correct crossing.
// Mids: 20,0,20,20,20 → p=.2,0,.2,.2,.2, sum=.8 → normalised .25,0,.25,.25,.25.
//   bin0 → cum .25 (<.5)
//   bin1 (p=0, illiquid) → cum .25 (no mass added; must not crash)
//   bin2 → .50 (≥.5) crossing [97000,98000] at cum_prev=.25:
//   median = 97000 + (.5 − .25)/.25 · 1000 = 98000 (the bin's TOP).
//
// MUTATION (reachable): make `implied_prob` treat a (0,0) bin as carrying mass
// (e.g. `(yes_bid + yes_ask + 1)` or a min-floor prob) → the illiquid bin shifts
// the cumulative → the crossing moves off bin2 → the 98000 assertion reds.
//
// NOTE on the div-by-zero guard specifically: under the `>=` crossing a zero
// bin can NEVER be the crossing bin, so the `/ p` guard is DEFENSIVE and is not
// independently red-able by any synthetic ladder (verified during mutation
// testing; recorded in ASSUMPTIONS). This test pins the REACHABLE property —
// finiteness + correct zero-mass handling.
#[test]
fn illiquid_zero_prob_bin_stays_finite() {
    let bins = ladder([(19, 21), (0, 0), (19, 21), (19, 21), (19, 21)]);
    let median = bracket_implied_median(&bins).expect("median exists");
    assert!(median.is_finite(), "median must be finite, got {median}");
    assert!(
        (median - 98_000.0).abs() < 1e-9,
        "crossing-at-top median should be 98000 (zero bin adds no mass), got {median}"
    );
}

// ─── 6. Degenerate ladder: all bins zero-prob → None, not tradeable ─────────

// MUTATION: return `Some(0.0)` (or any value) on sum_p == 0 instead of `None`
// → `bracket_implied_median` would be Some → compute_basis would be Some →
// these `is_none()` assertions red.
#[test]
fn degenerate_all_zero_prob_is_none() {
    let bins = ladder([(0, 0), (0, 0), (0, 0), (0, 0), (0, 0)]);
    assert!(
        bracket_implied_median(&bins).is_none(),
        "all-zero ladder has no implied median"
    );
    // None propagates: no median → no basis signal → cannot be tradeable.
    assert!(
        compute_basis(&bins, perp_dollars(97_000), 1.0, 0.0).is_none(),
        "no median → compute_basis returns None"
    );
}

// ─── extra: empty slice → None (the `None`-safe contract on no bins) ────────

// `bracket_implied_median` is `None`-safe on an empty ladder (no panic, no
// NaN). Note the "0.5 never reached" branch is otherwise unreachable for a
// NON-empty ladder: normalisation forces total mass to exactly 1.0, so 0.5 is
// always crossed there; the genuine non-crossing cases are the empty slice
// (here) and the all-zero `sum_p == 0` ladder (the degenerate test above).
#[test]
fn empty_slice_is_none() {
    let bins: Vec<BracketBin> = Vec::new();
    assert!(
        bracket_implied_median(&bins).is_none(),
        "empty ladder → None (no panic, no NaN)"
    );
    assert!(
        compute_basis(&bins, perp_dollars(97_000), 1.0, 0.0).is_none(),
        "empty ladder → compute_basis None"
    );
}

// ─── determinism: identical ladder → byte-identical result ──────────────────

#[test]
fn determinism_same_ladder_same_result() {
    let bins = ladder([(18, 22), (24, 26), (30, 34), (10, 14), (4, 8)]);
    let a = bracket_implied_median(&bins);
    let b = bracket_implied_median(&bins);
    assert_eq!(a, b, "median is a pure function of the ladder");

    let sa = compute_basis(&bins, perp_dollars(97_250), 7.0, 3.0);
    let sb = compute_basis(&bins, perp_dollars(97_250), 7.0, 3.0);
    assert_eq!(sa, sb, "compute_basis is a pure function of its inputs");
}

// ─── ordering: an UNSORTED ladder is sorted by floor before accumulating ────

// Same uniform-0.2 ladder as the nominal test but presented out of floor order.
// The kernel must sort by floor → identical median (97500). This pins the
// "sort bins by floor ascending" step.
//
// MUTATION: remove the sort → cumulative probability accumulates in input
// order → the crossing bin is wrong → median ≠ 97500 → reds.
#[test]
fn unsorted_ladder_is_sorted_by_floor() {
    let f = |floor: f64| bin(floor, floor + 1_000.0, 19, 21);
    // Deliberately shuffled input order.
    let bins = vec![
        f(98_000.0),
        f(95_000.0),
        f(99_000.0),
        f(97_000.0),
        f(96_000.0),
    ];
    let median = bracket_implied_median(&bins).expect("median exists");
    assert!(
        (median - 97_500.0).abs() < 1e-9,
        "sorted-by-floor median should be 97500, got {median}"
    );
}

// ─── construction sanity: BasisSignal fields carry the fee floor through ────

#[test]
fn signal_carries_the_passed_fee_floor() {
    let bins = ladder(UNIFORM_20C);
    let sig: BasisSignal = compute_basis(&bins, perp_dollars(97_600), 8.0, 2.0).expect("median");
    assert!(
        (sig.fee_floor_dollars - 8.0).abs() < 1e-9,
        "the signal reports the passed-in fee floor"
    );
}
