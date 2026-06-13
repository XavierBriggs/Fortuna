//! Perpetual-futures domain types (spec 5.15; T5.B2).
//!
//! Perp prices ride in `PerpPrice`, a venue-scoped integer newtype in
//! TEN-THOUSANDTHS of a dollar (the venue tick is $0.0001), with checked
//! arithmetic; `Decimal` appears only at venue payload boundaries. `Cents`
//! remains the only money type in the event-contract core: the perps domain
//! converts to `Cents` exclusively at notional/PnL/fee boundaries, with the
//! rounding direction explicit so the caller can always round against us
//! (gains and exposure-reducing values floor, costs and exposure ceil).
//! A `PerpPrice` must never carry an event-contract price nor vice versa —
//! the separation is type-level, not convention.
//!
//! Perp positions never resolve: `PerpPosition` carries NO settlement
//! lifecycle (spec 5.13 does not apply). In its place the domain carries
//! margin state (`MarginAccountView`), a mark-price feed (`PerpMarks`), and
//! funding accruals (`FundingAccrual`, an append-only periodic cash-flow
//! record reconciled against the venue's funding endpoints).
//!
//! Conservative marking (5.15 halt-math rule): if the venue settlement mark
//! and our conservative mark disagree, the worse-for-us number governs.
//!
//! Funding semantics (research §4, docs/research/venue/kinetics-perps-2026-06-10/):
//! the rate is a decimal fraction per 8h window; positive rate means longs
//! pay shorts. The venue applies its own cap (±2%) and zero threshold
//! (|rate| < 0.01% -> 0) BEFORE reporting — this module records reported
//! rates, it does not re-derive them. `FundingAccrual.rate` is `Decimal`
//! deliberately: the record is a venue-payload boundary artifact and must
//! reproduce the venue's reported rate exactly for reconciliation.

use crate::clock::UtcTimestamp;
use crate::market::{Contracts, MarketId};
use crate::money::{Cents, MoneyError};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Ten-thousandths of a dollar per whole cent.
const TT_PER_CENT: i64 = 100;

/// Errors from perps-domain operations.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PerpError {
    /// Ten-thousandths integer arithmetic overflowed i64.
    #[error("perp ten-thousandths arithmetic overflow during {op}")]
    Overflow { op: &'static str },
    /// An exact conversion was requested but the amount has a sub-tick part.
    #[error("dollar amount {amount} has a sub-tick remainder; pick a rounding direction")]
    SubTickRemainder { amount: String },
    /// The amount does not fit in i64 ten-thousandths.
    #[error("dollar amount {amount} is outside the representable i64 ten-thousandths range")]
    OutOfRange { amount: String },
    /// A funding window received more than its 480 one-minute candles: the
    /// caller did not roll the window at `next_funding_time`.
    #[error("funding window received more than 480 candles (roll at next_funding_time)")]
    FundingWindowOverfull,
    /// A cent-side operation failed.
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// Which family of instrument a market belongs to (spec 5.15). Code paths
/// that assume resolution (settlement watchdogs, outcome scoring) must
/// dispatch on kind, never guess from venue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentKind {
    BinaryEvent,
    Perp,
}

/// Perp price in integer TEN-THOUSANDTHS of a dollar ($0.0001 venue tick).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct PerpPrice(i64);

impl PerpPrice {
    pub const ZERO: PerpPrice = PerpPrice(0);

    pub const fn new(raw: i64) -> Self {
        PerpPrice(raw)
    }

    pub const fn raw(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, rhs: PerpPrice) -> Result<PerpPrice, PerpError> {
        self.0
            .checked_add(rhs.0)
            .map(PerpPrice)
            .ok_or(PerpError::Overflow { op: "price add" })
    }

    pub fn checked_sub(self, rhs: PerpPrice) -> Result<PerpPrice, PerpError> {
        self.0
            .checked_sub(rhs.0)
            .map(PerpPrice)
            .ok_or(PerpError::Overflow { op: "price sub" })
    }

    /// Price x quantity = a value still in ten-thousandths (`PerpValue`).
    /// Negative quantities arise from signed-position PnL math.
    pub fn checked_mul(self, qty: i64) -> Result<PerpValue, PerpError> {
        self.0
            .checked_mul(qty)
            .map(PerpValue)
            .ok_or(PerpError::Overflow { op: "price mul" })
    }

    /// Convert decimal dollars to ten-thousandths, rounding toward negative
    /// infinity. Venue payload boundary only.
    pub fn from_dollars_floor(dollars: Decimal) -> Result<PerpPrice, PerpError> {
        let tt = to_tt_decimal(dollars)?;
        decimal_to_i64(tt.floor(), dollars).map(PerpPrice)
    }

    /// Convert decimal dollars to ten-thousandths, rounding toward positive
    /// infinity. Venue payload boundary only.
    pub fn from_dollars_ceil(dollars: Decimal) -> Result<PerpPrice, PerpError> {
        let tt = to_tt_decimal(dollars)?;
        decimal_to_i64(tt.ceil(), dollars).map(PerpPrice)
    }

    /// Convert decimal dollars to ten-thousandths, requiring a whole-tick
    /// amount (the venue quotes fixed-point strings at $0.0001 tick, so this
    /// is the normal payload path).
    pub fn from_dollars_exact(dollars: Decimal) -> Result<PerpPrice, PerpError> {
        let tt = to_tt_decimal(dollars)?;
        if !tt.fract().is_zero() {
            return Err(PerpError::SubTickRemainder {
                amount: dollars.to_string(),
            });
        }
        decimal_to_i64(tt, dollars).map(PerpPrice)
    }

    /// Exact decimal-dollar view (boundary use only).
    pub fn to_dollars(self) -> Decimal {
        Decimal::new(self.0, 4)
    }
}

impl fmt::Display for PerpPrice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // i128 so i64::MIN renders without overflow.
        let v = i128::from(self.0);
        let (sign, abs) = if v < 0 { ("-", -v) } else { ("", v) };
        write!(f, "{sign}${}.{:04}", abs / 10_000, abs % 10_000)
    }
}

/// A signed amount in integer ten-thousandths of a dollar: the product of a
/// `PerpPrice` and a quantity, before the explicit conversion to `Cents`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct PerpValue(i64);

impl PerpValue {
    pub const ZERO: PerpValue = PerpValue(0);

    pub const fn new(raw: i64) -> Self {
        PerpValue(raw)
    }

    pub const fn raw(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, rhs: PerpValue) -> Result<PerpValue, PerpError> {
        self.0
            .checked_add(rhs.0)
            .map(PerpValue)
            .ok_or(PerpError::Overflow { op: "value add" })
    }

    pub fn checked_sub(self, rhs: PerpValue) -> Result<PerpValue, PerpError> {
        self.0
            .checked_sub(rhs.0)
            .map(PerpValue)
            .ok_or(PerpError::Overflow { op: "value sub" })
    }

    /// To cents, rounding toward negative infinity (use for gains/PnL: a
    /// sub-cent gain is never counted; a sub-cent loss becomes a full cent).
    /// Infallible: dividing by 100 always shrinks into range.
    pub fn to_cents_floor(self) -> Cents {
        Cents::new(self.0.div_euclid(TT_PER_CENT))
    }

    /// To cents, rounding toward positive infinity (use for costs/exposure:
    /// never understated). Infallible: dividing by 100 always shrinks into
    /// range, and the +1 on a truncated quotient cannot overflow.
    pub fn to_cents_ceil(self) -> Cents {
        let q = self.0.div_euclid(TT_PER_CENT);
        let r = self.0.rem_euclid(TT_PER_CENT);
        Cents::new(if r != 0 { q + 1 } else { q })
    }

    /// Exact decimal-dollar view (boundary use only).
    pub fn to_dollars(self) -> Decimal {
        Decimal::new(self.0, 4)
    }
}

fn to_tt_decimal(dollars: Decimal) -> Result<Decimal, PerpError> {
    dollars
        .checked_mul(Decimal::new(10_000, 0))
        .ok_or_else(|| PerpError::OutOfRange {
            amount: dollars.to_string(),
        })
}

fn decimal_to_i64(tt: Decimal, original_dollars: Decimal) -> Result<i64, PerpError> {
    tt.to_i64().ok_or_else(|| PerpError::OutOfRange {
        amount: original_dollars.to_string(),
    })
}

/// Append-only periodic funding cash-flow record (spec 5.15; research §4).
/// One row per (market, funding_time) the account held a position across;
/// reconciled against the venue's funding_history endpoint. Never updated
/// in place.
///
/// `amount` is OUR signed cash flow in cents: positive = received,
/// negative = paid (the venue's funding_history convention), floored toward
/// negative infinity so receipts are never overstated and payments never
/// understated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FundingAccrual {
    pub market: MarketId,
    pub funding_time: UtcTimestamp,
    /// Venue-reported decimal fraction per 8h window (already capped and
    /// zero-thresholded by the venue). Positive = longs pay.
    pub rate: Decimal,
    /// The venue's settlement mark price, which funding accrues on.
    pub settlement_mark: PerpPrice,
    /// SIGNED position at the funding tick: positive = long.
    pub position_qty: Contracts,
    /// Our signed flow in cents: positive = received, negative = paid.
    pub amount: Cents,
}

impl FundingAccrual {
    /// Build the record from venue-reported inputs, computing our signed
    /// flow: `-(rate x mark x qty)` dollars (positive rate + long position
    /// means we pay), floored toward negative infinity (against us).
    pub fn accrue(
        market: MarketId,
        funding_time: UtcTimestamp,
        rate: Decimal,
        settlement_mark: PerpPrice,
        position_qty: Contracts,
    ) -> Result<FundingAccrual, PerpError> {
        let flow = rate
            .checked_mul(settlement_mark.to_dollars())
            .and_then(|x| x.checked_mul(Decimal::from(position_qty.raw())))
            .ok_or_else(|| PerpError::OutOfRange {
                amount: rate.to_string(),
            })?;
        let amount = Cents::from_dollars_floor(-flow)?;
        Ok(FundingAccrual {
            market,
            funding_time,
            rate,
            settlement_mark,
            position_qty,
            amount,
        })
    }
}

/// One-minute candles in an 8h funding window (research §4: 480 candles).
pub const FUNDING_CANDLES_PER_WINDOW: usize = 480;

/// The venue clamps the finalized funding rate to +/-2% per 8h window
/// (research §4 "Cap"). `Decimal::from_parts(2, 0, 0, false, 2)` = 0.02.
pub const FUNDING_RATE_CLAMP: Decimal = Decimal::from_parts(2, 0, 0, false, 2);

/// Below this absolute value the venue zeroes the rate and pays nothing
/// (research §4 "Zero threshold": |rate| < 0.01%). 0.0001 = 0.01%.
pub const FUNDING_ZERO_THRESHOLD: Decimal = Decimal::from_parts(1, 0, 0, false, 4);

/// The venue's funding finalization (research §4): clamp the raw rate to
/// +/-`FUNDING_RATE_CLAMP`, then zero it if its absolute value is STRICTLY
/// below `FUNDING_ZERO_THRESHOLD` (exactly at the threshold is kept). This
/// is the deterministic kernel a `funding_forecast` strategy (T5.B7)
/// applies to its forecast and `perp_event_basis` applies to the perp
/// point forecast; it matches the rate the venue actually pays.
pub fn finalize_funding_rate(raw: Decimal) -> Decimal {
    let clamped = raw.clamp(-FUNDING_RATE_CLAMP, FUNDING_RATE_CLAMP);
    if clamped.abs() < FUNDING_ZERO_THRESHOLD {
        Decimal::ZERO
    } else {
        clamped
    }
}

/// The in-progress estimator for one 8h funding window (research §4;
/// T5.B7 foundation). The venue computes funding as the time-weighted
/// average of 1-minute candlestick premiums over the window's 480 candles,
/// continuously estimated over `[last_funding_time, now)` and finalized at
/// `next_funding_time`. This reproduces that estimate deterministically
/// from observed premiums — the reconcilable baseline a `funding_forecast`
/// strategy emits as its scalar claim and `perp_event_basis` uses as the
/// perp point forecast.
///
/// The premium PER CANDLE is taken as INPUT, never re-derived: the exact
/// premium-index formula (which mark vs which index) is venue-unpublished
/// (research §11), the same not-re-deriving discipline as `FundingAccrual`.
/// Premiums are `Decimal` (the venue-payload rate domain). Equal 1-minute
/// candles make the time-weighted average the arithmetic mean; gap/uneven-
/// candle weighting is a strategy refinement (deferred, see GAPS).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FundingWindow {
    sum: Decimal,
    count: usize,
}

impl FundingWindow {
    pub fn new() -> FundingWindow {
        FundingWindow::default()
    }

    /// Observe one 1-minute candle's premium. The 481st candle belongs to
    /// the next window: it is rejected so the caller rolls at
    /// `next_funding_time` rather than silently blending two windows.
    pub fn observe(&mut self, premium: Decimal) -> Result<(), PerpError> {
        if self.count >= FUNDING_CANDLES_PER_WINDOW {
            return Err(PerpError::FundingWindowOverfull);
        }
        self.sum = self
            .sum
            .checked_add(premium)
            .ok_or(PerpError::Overflow { op: "funding sum" })?;
        self.count += 1;
        Ok(())
    }

    /// Candles observed so far in this window.
    pub fn observed(&self) -> usize {
        self.count
    }

    /// Candles left before the window is full (forecast confidence scales
    /// with the elapsed fraction).
    pub fn remaining(&self) -> usize {
        FUNDING_CANDLES_PER_WINDOW - self.count
    }

    /// The venue's in-progress estimate: the equal-weight mean of observed
    /// premiums (reconcilable against the estimate endpoint). `None` until
    /// the first candle.
    pub fn running_estimate(&self) -> Result<Option<Decimal>, PerpError> {
        if self.count == 0 {
            return Ok(None);
        }
        let mean = self
            .sum
            .checked_div(Decimal::from(self.count as u64))
            .ok_or(PerpError::Overflow { op: "funding twap" })?;
        Ok(Some(mean))
    }

    /// The deterministic final-rate forecast under the stationary-mean
    /// assumption (the remaining candles carry the running average): the
    /// running estimate, finalized (clamp + zero threshold). This is the
    /// rate the venue would pay if the window closed now; strategy-level
    /// extrapolation (premium persistence, trend) layers on top.
    pub fn forecast_final(&self) -> Result<Option<Decimal>, PerpError> {
        Ok(self.running_estimate()?.map(finalize_funding_rate))
    }
}

/// A SIGNED perp position: positive qty = long, negative = short. Carries
/// no settlement lifecycle (perp positions never resolve, spec 5.15).
/// Position-update math (fills, flips, realized PnL) belongs to the state
/// and paper layers, not this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerpPosition {
    pub market: MarketId,
    /// Signed contract count: positive = long, negative = short.
    pub qty: Contracts,
    /// Volume-weighted average entry price (venue-reported, whole ticks).
    pub avg_entry: PerpPrice,
}

impl PerpPosition {
    pub fn is_long(&self) -> bool {
        self.qty.raw() > 0
    }

    pub fn is_short(&self) -> bool {
        self.qty.raw() < 0
    }

    pub fn is_flat(&self) -> bool {
        self.qty.raw() == 0
    }

    /// Signed unrealized PnL at `mark`: `(mark - entry) x qty`, floored
    /// toward negative infinity (sub-cent gains drop, sub-cent losses round
    /// to a full cent against us).
    pub fn unrealized_pnl(&self, mark: PerpPrice) -> Result<Cents, PerpError> {
        let diff = mark.checked_sub(self.avg_entry)?;
        Ok(diff.checked_mul(self.qty.raw())?.to_cents_floor())
    }

    /// Gross notional at `mark`: `|qty| x mark`, ceiled so exposure is never
    /// understated. Direction-blind.
    pub fn notional_at(&self, mark: PerpPrice) -> Result<Cents, PerpError> {
        let abs_qty = self
            .qty
            .raw()
            .checked_abs()
            .ok_or(PerpError::Overflow { op: "qty abs" })?;
        Ok(mark.checked_mul(abs_qty)?.to_cents_ceil())
    }
}

/// The two marks available for a perp position. The venue settlement mark
/// always exists (it is what funding and liquidation run on); our
/// independent conservative mark may not (degraded feed) — that absence is
/// flagged, never papered over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerpMarks {
    pub venue_settlement: PerpPrice,
    /// Our independently derived conservative mark; `None` = unavailable.
    pub conservative: Option<PerpPrice>,
}

/// Margin-account view with conservative marking (spec 5.15): the margin
/// ACCOUNT — not the position — is the exposure unit, and when the venue
/// mark and our conservative mark disagree, the worse-for-us unrealized
/// number governs halt math.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarginAccountView {
    /// Settled margin cash (venue-confirmed; settled funding included).
    pub balance: Cents,
    /// Sum over positions of the worse-for-us unrealized PnL.
    pub unrealized: Cents,
    /// Signed accrued-but-unsettled funding for the in-progress window.
    pub pending_funding: Cents,
    /// balance + unrealized + pending_funding: the I2 drawdown input.
    pub equity: Cents,
    /// True when any position lacked an independent conservative mark, so
    /// `unrealized` rests on the venue's number alone.
    pub unmarked_flag: bool,
}

impl MarginAccountView {
    /// Compute the view. Per position the unrealized PnL is evaluated at
    /// BOTH marks and the smaller (worse-for-us) value is taken; a missing
    /// conservative mark uses the venue mark and sets `unmarked_flag`.
    pub fn compute(
        balance: Cents,
        positions: &[(PerpPosition, PerpMarks)],
        pending_funding: Cents,
    ) -> Result<MarginAccountView, PerpError> {
        let mut unrealized = Cents::ZERO;
        let mut unmarked_flag = false;
        for (position, marks) in positions {
            let venue_upnl = position.unrealized_pnl(marks.venue_settlement)?;
            let upnl = match marks.conservative {
                Some(ours) => venue_upnl.min(position.unrealized_pnl(ours)?),
                None => {
                    unmarked_flag = true;
                    venue_upnl
                }
            };
            unrealized = unrealized.checked_add(upnl).map_err(PerpError::Money)?;
        }
        let equity = balance
            .checked_add(unrealized)
            .and_then(|e| e.checked_add(pending_funding))
            .map_err(PerpError::Money)?;
        Ok(MarginAccountView {
            balance,
            unrealized,
            pending_funding,
            equity,
            unmarked_flag,
        })
    }
}
