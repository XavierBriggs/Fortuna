//! The `perp_event_basis` BASIS KERNEL — the deterministic comparison between a
//! perp settlement mark and a KXBTC (price-level) bracket ladder's implied
//! central estimate (design: docs/design/perp-strategies-and-scalar-claims.md §3, §3.1,
//! §7; GAPS "TRACK C — slice 3b: PAIRED-CYCLE FIXTURE … basis VALIDATED").
//!
//! # What this is (and is not)
//!
//! This module is the deterministic, forecast-quality basis SIGNAL only: given
//! a bracket ladder and a perp mark, it computes the implied median of the
//! ladder, the signed basis (`perp_mark − implied_median`), and whether that
//! basis clears the FEE-TRAP floor. It is `f64`-cognition throughout (bracket
//! probabilities and the basis signal are forecast quantities) — **never
//! money**. There is NO money-type touch in this kernel at all: the caller
//! passes the perp mark as an `f64` BTC-dollars value (see [`compute_basis`]),
//! so the price-domain → `f64` boundary lives in the CALLER, not here. This
//! placement is the money-discipline correction recorded in GAPS: the kernel
//! uses `f64`, and CLAUDE.md forbids `f64` for prices in `fortuna-core`, so the
//! kernel lives in `fortuna-cognition` (alongside [`crate::scoring`]'s
//! `f64`-forecast types), not `fortuna-core::perp`.
//!
//! The actual bracket-leg TRADE (maker-only `Cents` bracket legs, sized by the
//! harness toward the perp forecast) is the DEFERRED `perp_event_basis`
//! STRATEGY's exec-boundary money op (`fortuna-runner`), not part of this
//! kernel. Synthetic ladders prove the LOGIC; the real-orderbook end-to-end is
//! exercised by the committed LIVE paired-cycle fixture
//! (fixtures/perp-basis/paired_cycle_btc_perp_vs_kxbtc.json — the
//! `basis_live_fixture.rs` e2e), never an invented calibration claim.
//!
//! # Bracket structure (grounded in the LIVE capture; never invented)
//!
//! Grounded against the live recorder capture (GAPS "PAIRED-CYCLE FIXTURE")
//! and the Kalshi research: the price-level ladder this kernel scores is
//! **KXBTC**. A KXBTC ladder PARTITIONS the BTC price axis with markets of
//! three `strike_type`s (see [`BracketStrike`]):
//!
//! - **`between`** — a CLOSED `[floor_strike, cap_strike]` range bin (e.g.
//!   "$63,500 to 63,999.99"): `P(floor ≤ BTC < cap)`. Both strikes present.
//!   These are the workhorse bins the median interpolates within.
//! - **`greater`** — the OPEN top tail (e.g. "$75,000 or above"): `P(BTC >
//!   floor)`. `floor_strike` only; `cap_strike` is absent.
//! - **`less`** — the OPEN bottom tail (e.g. "$50,999.99 or below"): `P(BTC ≤
//!   cap)`. `cap_strike` only; `floor_strike` is absent.
//!
//! The bins are mutually exclusive and exhaustive, so the YES-mids sum to ≈ 1
//! (and are NORMALISED here to remove the bid/ask half-spread overcount). YES
//! is quoted in DOLLAR-STRINGS on a $1 payout (`yes_bid_dollars:"0.0600"` =
//! $0.06), so the YES mid `(bid+ask)/2` IS the bin's implied probability — but
//! the kernel is STRING-FORMAT-AGNOSTIC: [`BracketBin`] carries the already-
//! parsed `f64` probability, and the dollar-string → probability parse is the
//! caller's boundary (so a future quote format does not touch this kernel).
//! (Schema at docs/research/venue/kalshi-api-2026-06-10 research.md:251-253;
//! the live shape in fixtures/perp-basis/paired_cycle_btc_perp_vs_kxbtc.)

/// Which `strike_type` a KXBTC market is, and the BTC price strike(s) it
/// carries. Each variant maps 1:1 to a live `strike_type`; the structure is the
/// venue's (live-captured), never invented.
///
/// `f64` strikes are correct here — these are forecast-domain price LEVELS used
/// to interpolate a probability median, NOT money/price amounts the system
/// trades at (those stay `Cents`/`PerpPrice` in the exec path).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BracketStrike {
    /// A CLOSED range bin: `P(floor ≤ BTC < cap)`. Per-bin invariant:
    /// `floor < cap`. The only kind the median can INTERPOLATE within.
    Between {
        /// `floor_strike` (dollars): the lower edge of the YES interval.
        floor: f64,
        /// `cap_strike` (dollars): the upper edge of the YES interval.
        cap: f64,
    },
    /// The OPEN top tail: `P(BTC > floor)`. No upper edge → no finite width to
    /// interpolate, so a 0.5 crossing landing here yields `None`.
    Greater {
        /// `floor_strike` (dollars): the lower edge of the open top tail.
        floor: f64,
    },
    /// The OPEN bottom tail: `P(BTC ≤ cap)`. No lower edge → no finite width to
    /// interpolate, so a 0.5 crossing landing here yields `None`.
    Less {
        /// `cap_strike` (dollars): the upper edge of the open bottom tail.
        cap: f64,
    },
}

/// One bracket bin (one active KXBTC market) in a ladder: where it sits on the
/// BTC price axis ([`BracketStrike`]) and its YES-implied probability.
///
/// Grounded 1:1 in the Kalshi market schema + the live fixture (the structure
/// is never invented). The probability is `f64` in the forecast domain — NOT a
/// money/price amount.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BracketBin {
    /// The bin's price-axis position + strike(s).
    pub kind: BracketStrike,
    /// The bin's implied YES probability (0..=1 for a well-formed quote), the
    /// YES-mid `(yes_bid + yes_ask)/2` the CALLER parsed from the dollar-strings.
    /// An illiquid bin (`yes_bid == yes_ask == 0`) is exactly `0.0` (no mass),
    /// which the median accumulator handles without dividing by it. The kernel
    /// is string-format-agnostic: parsing the quote strings is the caller's
    /// boundary.
    pub prob: f64,
}

impl BracketBin {
    /// The bin's implied YES probability (already the caller-supplied mid).
    fn implied_prob(&self) -> f64 {
        self.prob
    }
}

/// The price-axis ordering rank of a bin: the open `less` tail is at the bottom
/// (0), the `between` bins in the middle (1), the open `greater` tail at the
/// top (2). Within a rank the secondary key (`order_key`) breaks ties.
fn order_rank(kind: &BracketStrike) -> u8 {
    match kind {
        BracketStrike::Less { .. } => 0,
        BracketStrike::Between { .. } => 1,
        BracketStrike::Greater { .. } => 2,
    }
}

/// The within-rank ordering key: a `between` bin sorts by its `floor`; the open
/// tails sort by their single strike (there is at most one of each in a
/// well-formed ladder, so the tail key only matters for total ordering, not
/// disambiguation).
fn order_key(kind: &BracketStrike) -> f64 {
    match kind {
        BracketStrike::Less { cap } => *cap,
        BracketStrike::Between { floor, .. } => *floor,
        BracketStrike::Greater { floor } => *floor,
    }
}

/// The implied central estimate (MEDIAN) of a bracket ladder, in DOLLARS: the
/// BTC price level where the cumulative (normalised) implied YES probability
/// crosses 0.5, linearly interpolated within the crossing `between` bin.
///
/// Algorithm (design §3), over the FULL partition (`less` tail + `between` bins
/// + `greater` tail):
/// 1. `sum_p = Σ prob_i`. If `sum_p ≤ 0` or non-finite (a degenerate/illiquid
///    ladder, or a malformed non-finite prob) return `None`.
/// 2. Otherwise NORMALISE every `prob_i` by `sum_p`, ORDER the bins by price
///    position (the `less` tail at the bottom, the `between` bins ascending by
///    `floor`, the `greater` tail at the top), and accumulate cumulative
///    probability.
/// 3. At the first bin where `cum + p_i ≥ 0.5`:
///    - if it is a `between` bin, interpolate
///      `median = floor + (0.5 − cum)/p_i · (cap − floor)`;
///    - if it is an OPEN tail (`less`/`greater`), there is no finite width to
///      interpolate — the median is outside the resolved range, so return
///      `None` (conservative; no fabricated point).
///
/// Returns `None` on an empty ladder, an all-zero (`sum_p == 0`) ladder, a
/// non-finite prob, and the open-tail-crossing case. The interpolation is
/// guarded so a zero-probability bin (`p_i == 0`) is never the crossing bin (it
/// cannot be: `cum + 0 ≥ 0.5` only when `cum` is already `≥ 0.5`, which would
/// have crossed at an earlier positive bin), so the `/ p_i` step never divides
/// by zero and never produces a `NaN`.
pub fn bracket_implied_median(bins: &[BracketBin]) -> Option<f64> {
    if bins.is_empty() {
        return None;
    }

    // Order a copy by price position FIRST; the caller's slice is left untouched
    // and — critically — every downstream float reduction runs in this canonical
    // order, so the median is a pure function of the ladder MULTISET, INDEPENDENT
    // of the caller's input order.
    let mut sorted: Vec<BracketBin> = bins.to_vec();
    sorted.sort_by(|a, b| {
        order_rank(&a.kind).cmp(&order_rank(&b.kind)).then_with(|| {
            order_key(&a.kind)
                .partial_cmp(&order_key(&b.kind))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    // sum_p over the SORTED (canonical) order, NOT the caller's input order.
    // Float addition is non-associative, so summing in input order let two
    // callers passing the SAME bins in DIFFERENT orders get sum_p values that
    // differ by an ULP — enough to flip the 0.5 crossing at an exact
    // cumulative-equals-0.5-at-a-bin-boundary tie (a `between` top vs the next
    // open tail), so one caller saw a finite median and another saw `None`.
    // Canonicalising the reduction order removes that order-dependence entirely.
    // (DST-found, TRACK C slice 3b: a propose-only strategy and its independent
    // DST oracle passed identical bins in BTreeMap vs Vec order and diverged on
    // exactly one seed at the B5/greater boundary.)
    let sum_p: f64 = sorted.iter().map(BracketBin::implied_prob).sum();
    // Degenerate / illiquid ladder, or a non-finite (NaN/inf) prob from a
    // malformed caller parse: no usable implied mass → no median. (`<= 0.0`
    // rather than `== 0.0` so a pathological negative is also degenerate; the
    // `is_finite` guard keeps a NaN prob from ever yielding a NaN median.)
    if sum_p <= 0.0 || !sum_p.is_finite() {
        return None;
    }

    let mut cum = 0.0_f64;
    for binr in &sorted {
        let p = binr.implied_prob() / sum_p;
        if cum + p >= 0.5 {
            match binr.kind {
                BracketStrike::Between { floor, cap } => {
                    // DEFENSIVE no-divide-by-zero: the crossing bin always has
                    // `p > 0` under the `>=` test (a zero-prob bin cannot be the
                    // first to reach 0.5 — `cum + 0 >= 0.5` needs `cum >= 0.5`,
                    // which an earlier POSITIVE bin would already have crossed
                    // on), so this is the only place a `/ p` could occur and `p`
                    // is provably positive here. The guard keeps the NaN-safety
                    // explicit and total: a (documented-unreachable) zero at the
                    // crossing degrades to `None`, never a `NaN` median. See
                    // ASSUMPTIONS (belt-and-suspenders, not a reachable mutation).
                    if p <= 0.0 {
                        return None;
                    }
                    let frac = (0.5 - cum) / p;
                    return Some(floor + frac * (cap - floor));
                }
                // The 0.5 crossing fell in an OPEN tail: no finite [floor,cap]
                // to interpolate, so the median is outside the resolved range.
                // Conservative: `None`, never a fabricated single-strike point.
                BracketStrike::Greater { .. } | BracketStrike::Less { .. } => {
                    return None;
                }
            }
        }
        cum += p;
    }

    // Unreached for a normalised non-degenerate ladder; the structural fallback
    // for the "0.5 never crossed" contract.
    None
}

/// The deterministic basis signal: a forecast-quality comparison of a perp mark
/// against a bracket ladder's implied median, plus the FEE-TRAP verdict.
///
/// All fields are `f64` dollars (forecast-domain), NOT money — see the module
/// doc. The tradeable verdict here is the kernel's signal; the actual sized,
/// gated bracket-leg order is the deferred strategy's `Cents` exec op (I6/I7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BasisSignal {
    /// The bracket ladder's implied median, in dollars (`bracket_implied_median`).
    pub bracket_implied_median: f64,
    /// The perp settlement mark, in BTC-dollars (the value the caller passed —
    /// design §3: the point forecast of the fixing).
    pub perp_mark: f64,
    /// `perp_mark − bracket_implied_median`, in dollars. Positive = the perp
    /// sits above the bracket-implied central estimate.
    pub signed_basis: f64,
    /// The FEE-TRAP floor in dollars that was PASSED IN (amendment C: the
    /// assumed post-promo round-trip bracket fee, ~5–12 bps — NOT recomputed
    /// from a `FeeModel`, and promo-$0 never lowers it). Reported so the
    /// verdict is self-describing.
    pub fee_floor_dollars: f64,
    /// True iff `|signed_basis| > (fee_floor_dollars + min_basis_dollars)` — the
    /// basis clears BOTH the assumed fees AND the configured edge margin. A
    /// strict `>` so a basis exactly at the combined floor is NOT tradeable.
    pub is_tradeable: bool,
}

/// Compute the basis signal for a bracket ladder against a perp mark.
///
/// Returns `None` exactly when `bracket_implied_median` does (an empty,
/// all-zero-probability, non-finite-prob, or open-tail-crossing ladder) — the
/// absence of an implied median propagates to the absence of a signal (and
/// therefore "not tradeable", since there is nothing to compare).
///
/// `perp_mark_btc_dollars` is the perp settlement mark as a BTC-spot `f64`
/// value. The KXBTCPERP contract is BTC/10000, so the caller converts the
/// venue's per-contract mark to BTC dollars (`per_contract × 10_000`, e.g.
/// `$6.3906 × 10_000 = $63,906`) at its own price-domain boundary and passes
/// the BTC value in — this kernel has NO money-type touch (the boundary is the
/// caller's, mirroring how the deferred strategy converts `PerpPrice` once).
///
/// `fee_floor_dollars` is the FEE-TRAP floor (amendment C, passed in, never
/// recomputed); `min_basis_dollars` is the additional configured edge margin in
/// underlying price units. The trade is gated when `|basis|` exceeds BOTH
/// combined.
pub fn compute_basis(
    bins: &[BracketBin],
    perp_mark_btc_dollars: f64,
    fee_floor_dollars: f64,
    min_basis_dollars: f64,
) -> Option<BasisSignal> {
    let median = bracket_implied_median(bins)?;
    let signed_basis = perp_mark_btc_dollars - median;
    let is_tradeable = signed_basis.abs() > (fee_floor_dollars + min_basis_dollars);

    Some(BasisSignal {
        bracket_implied_median: median,
        perp_mark: perp_mark_btc_dollars,
        signed_basis,
        fee_floor_dollars,
        is_tradeable,
    })
}
