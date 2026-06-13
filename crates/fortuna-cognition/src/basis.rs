//! The `perp_event_basis` BASIS KERNEL — the deterministic comparison between a
//! perp settlement mark and a KXBTC (price-level) bracket ladder's implied
//! central estimate (design: docs/design/perp-strategies-and-scalar-claims.md §3, §3.1,
//! §7; GAPS "TRACK C — slice 3 (perp_event_basis) GROUNDED").
//!
//! # What this is (and is not)
//!
//! This module is the deterministic, forecast-quality basis SIGNAL only: given
//! a bracket ladder and a perp mark, it computes the implied median of the
//! ladder, the signed basis (`perp_mark − implied_median`), and whether that
//! basis clears the FEE-TRAP floor. It is `f64`-cognition throughout (bracket
//! probabilities and the basis signal are forecast quantities) — **never
//! money**. The only cross-domain touch is the boundary read of a
//! [`PerpPrice`] (a `fortuna-core` money/price newtype) into `f64` dollars via
//! its own `to_dollars()`; no `PerpPrice`/`Cents` arithmetic happens here. This
//! placement is the money-discipline correction recorded in GAPS: the kernel
//! uses `f64`, and CLAUDE.md forbids `f64` for prices in `fortuna-core`, so the
//! kernel lives in `fortuna-cognition` (alongside [`crate::scoring`]'s
//! `f64`-forecast types), not `fortuna-core::perp`.
//!
//! The actual bracket-leg TRADE (maker-only `Cents` bracket legs, sized by the
//! harness toward the perp forecast) is the DEFERRED `perp_event_basis`
//! STRATEGY's exec-boundary money op (`fortuna-runner`), not part of this
//! kernel. The real-orderbook end-to-end (live KXBTC15M books) is fixture-gated
//! on operator-queue #4 (the paired KXBTCPERP1 + KXBTC cycle fixture) plus a
//! `KalshiMarket`/`Market` DTO extension carrying `floor_strike`/`cap_strike`
//! (track-A's Kalshi venue surface) — synthetic ladders here prove the LOGIC
//! only, never an e2e/calibration claim.
//!
//! # Bracket structure (grounded in the LIVE capture; never invented)
//!
//! CORRECTED 2026-06-13 against the live recorder capture (GAPS "LIVE
//! BRACKET-FORMAT INVESTIGATION"): the price-level ladder this kernel scores is
//! **KXBTC**, NOT KXBTC15M. A KXBTC market is a binary event contract with a
//! `strike_type`: `between` (a closed `[floor_strike, cap_strike]` range bin,
//! e.g. "$74,500 to 74,999.99") plus open tails `greater` (floor only) and
//! `less` (cap only); YES is quoted in DOLLAR-STRINGS on a $1 payout
//! (`yes_bid_dollars:"0.0100"` = 1¢). (KXBTC15M — this kernel's earlier guess —
//! is instead a single DIRECTIONAL "BTC up in 15 min?" binary, not a price
//! ladder; KXBTCD is a cumulative-threshold CDF ladder.) This kernel's
//! `BracketBin` models a CLOSED `between` bin (floor+cap); handling the open
//! `greater`/`less` tails + parsing the dollar-strings is a flagged refinement
//! (slice 3b) before the real-KXBTC e2e. Only synthetic test VALUES are
//! invented; the STRUCTURE is the venue's (live-captured, schema at
//! docs/research/venue/kalshi-api-2026-06-10 research.md:251-253).

use fortuna_core::perp::PerpPrice;

/// One bracket bin (one KXBTC15M market) in a ladder: the BTC price interval the
/// YES side pays on, plus its YES bid/ask.
///
/// Grounded 1:1 in the Kalshi market schema (the synthetic test values use this
/// real structure; the structure itself is never invented).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BracketBin {
    /// `floor_strike` (dollars): the lower edge of the YES interval. From the
    /// market schema (`floor_strike`, a `number`; asyncapi.yaml:3174,
    /// research.md:252). `f64` is correct here — this is a forecast-domain
    /// strike level used to interpolate a probability median, NOT a money/price
    /// amount the system trades at (those stay `Cents`/`PerpPrice`).
    pub floor: f64,
    /// `cap_strike` (dollars): the upper edge of the YES interval (`cap_strike`,
    /// a `number`; asyncapi.yaml:3175, research.md:252). Per-bin invariant:
    /// `floor < cap`.
    pub cap: f64,
    /// The YES BID in CENTS (0..=100): the best price a YES holder can sell at.
    /// Event-contract YES quotes are integer cents on a 0..100 scale (the
    /// bracket machinery's grain); the mid `(yes_bid + yes_ask)/2/100` is the
    /// bin's implied probability.
    pub yes_bid: i64,
    /// The YES ASK in CENTS (0..=100): the best price a YES buyer can pay at.
    pub yes_ask: i64,
}

impl BracketBin {
    /// The bin's implied YES probability from the bid/ask MID, mapped from cents
    /// to a unit probability: `(yes_bid + yes_ask) / 2 / 100`. An illiquid bin
    /// (`yes_bid == yes_ask == 0`) yields exactly `0.0` (no mass), which the
    /// median accumulator handles without dividing by it.
    fn implied_prob(&self) -> f64 {
        (self.yes_bid + self.yes_ask) as f64 / 2.0 / 100.0
    }
}

/// The implied central estimate of a bracket ladder, in DOLLARS: the strike
/// level where the cumulative (normalised) implied YES probability crosses 0.5,
/// linearly interpolated within the crossing bin.
///
/// Algorithm (design §3): per bin `p_i = (yes_bid + yes_ask)/2/100`;
/// `sum_p = Σ p_i`. If `sum_p == 0` (a degenerate/illiquid ladder — no implied
/// mass anywhere) return `None`. Otherwise NORMALISE every `p_i` by `sum_p`,
/// sort the bins by `floor` ascending, and accumulate cumulative probability;
/// at the first bin where `cum + p_i ≥ 0.5`, interpolate
/// `median = floor_i + (0.5 − cum)/p_i · (cap_i − floor_i)`. Returns `None` on
/// an empty ladder, and is guarded so a zero-probability bin (`p_i == 0`) is
/// never the crossing bin (it cannot be: `cum + 0 ≥ 0.5` only when `cum` is
/// already `≥ 0.5`, which would have crossed at an earlier positive bin), so the
/// `/ p_i` interpolation never divides by zero and never produces a `NaN`.
///
/// Because normalisation forces the total mass to exactly `1.0` for any
/// non-degenerate ladder, the 0.5 crossing is ALWAYS reached there; the only
/// `None` paths are the empty slice and the all-zero (`sum_p == 0`) ladder. The
/// loop nonetheless returns `None` if it somehow exits without crossing, so the
/// contract ("0.5 never reached → `None`") holds structurally.
pub fn bracket_implied_median(bins: &[BracketBin]) -> Option<f64> {
    if bins.is_empty() {
        return None;
    }

    let sum_p: f64 = bins.iter().map(BracketBin::implied_prob).sum();
    // Degenerate / illiquid ladder: no implied mass to find a median in.
    // (`<= 0.0` rather than `== 0.0` so a pathological negative — impossible
    // from non-negative cents, but defensive — is also treated as degenerate.)
    if sum_p <= 0.0 || !sum_p.is_finite() {
        return None;
    }

    // Sort a copy by floor ascending; the caller's slice is left untouched and
    // the median is order-independent (a pure function of the ladder).
    let mut sorted: Vec<BracketBin> = bins.to_vec();
    sorted.sort_by(|a, b| {
        a.floor
            .partial_cmp(&b.floor)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut cum = 0.0_f64;
    for binr in &sorted {
        let p = binr.implied_prob() / sum_p;
        if cum + p >= 0.5 {
            // DEFENSIVE no-divide-by-zero: the crossing bin always has `p > 0`
            // under the `>=` test (a zero-prob bin cannot be the first to reach
            // 0.5 — `cum + 0 >= 0.5` needs `cum >= 0.5`, which an earlier
            // POSITIVE bin would already have crossed on), so this branch is the
            // only place a `/ p` could occur and `p` is provably positive here.
            // The guard keeps the NaN-safety explicit and total regardless: a
            // (documented-unreachable) zero at the crossing degrades to `None`,
            // never a `NaN` median. See ASSUMPTIONS (the guard is belt-and-
            // suspenders, not a reachable mutation under `>=`).
            if p <= 0.0 {
                return None;
            }
            let frac = (0.5 - cum) / p;
            return Some(binr.floor + frac * (binr.cap - binr.floor));
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
    /// The perp settlement mark, in dollars (the `PerpPrice → f64` boundary read
    /// of the venue's settlement mark — design §3: the point forecast of the
    /// fixing).
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
/// Returns `None` exactly when `bracket_implied_median` does (an empty or
/// all-zero-probability ladder) — the absence of an implied median propagates
/// to the absence of a signal (and therefore "not tradeable", since there is
/// nothing to compare). The `perp_mark` is read into `f64` dollars at the
/// `PerpPrice` boundary (the kernel's only cross-domain touch).
///
/// `fee_floor_dollars` is the FEE-TRAP floor (amendment C, passed in, never
/// recomputed); `min_basis_dollars` is the additional configured edge margin in
/// underlying price units. The trade is gated when `|basis|` exceeds BOTH
/// combined.
pub fn compute_basis(
    bins: &[BracketBin],
    perp_mark: PerpPrice,
    fee_floor_dollars: f64,
    min_basis_dollars: f64,
) -> Option<BasisSignal> {
    let median = bracket_implied_median(bins)?;
    // Boundary read: PerpPrice (ten-thousandths) → f64 dollars via its own exact
    // Decimal view. This is the ONLY PerpPrice touch — no arithmetic on the
    // money type, and the f64 lands in the forecast domain.
    let perp_mark_dollars = perp_dollars_f64(perp_mark);
    let signed_basis = perp_mark_dollars - median;
    let is_tradeable = signed_basis.abs() > (fee_floor_dollars + min_basis_dollars);

    Some(BasisSignal {
        bracket_implied_median: median,
        perp_mark: perp_mark_dollars,
        signed_basis,
        fee_floor_dollars,
        is_tradeable,
    })
}

/// Ten-thousandths of a dollar per whole dollar (the `PerpPrice` $0.0001 tick).
const TEN_THOUSANDTHS_PER_DOLLAR: f64 = 10_000.0;

/// Read a [`PerpPrice`] into `f64` dollars at the cognition boundary, from its
/// raw ten-thousandths integer (`PerpPrice::raw`). Kept in one place so the
/// single cross-domain conversion is auditable, and done straight off the
/// integer accessor so this crate takes on NO `rust_decimal` dependency: the
/// `PerpPrice` semantics ($0.0001 tick) are exactly `raw / 10_000`. This
/// `i64 → f64` step is the forecast boundary, not a money boundary — a
/// BTC-scale dollar value is well within `f64` precision for a comparison
/// signal, and no money arithmetic occurs (the integer is only divided into the
/// forecast domain).
fn perp_dollars_f64(mark: PerpPrice) -> f64 {
    mark.raw() as f64 / TEN_THOUSANDTHS_PER_DOLLAR
}
