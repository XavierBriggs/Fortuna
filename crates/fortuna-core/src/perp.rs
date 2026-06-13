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
