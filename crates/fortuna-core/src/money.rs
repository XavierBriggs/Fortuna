//! `Cents(i64)` newtype, checked arithmetic, fee rounding always against us.
//!
//! Money is integer cents end to end. All arithmetic is checked: overflow is
//! an error value, never a panic and never a silent wrap. `Decimal` appears
//! only at conversion boundaries (venue payloads); the conversion primitives
//! are explicit about rounding direction so fee math can always round against
//! us (costs up via `from_dollars_ceil`, proceeds down via
//! `from_dollars_floor`).

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Errors from integer-cent money operations.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MoneyError {
    /// Integer-cent arithmetic overflowed i64.
    #[error("integer-cent arithmetic overflow during {op}")]
    Overflow { op: &'static str },
    /// An exact conversion was requested but the amount has a sub-cent part.
    #[error("dollar amount {amount} has a sub-cent remainder; pick a rounding direction")]
    SubCentRemainder { amount: String },
    /// The amount does not fit in i64 cents.
    #[error("dollar amount {amount} is outside the representable i64 cent range")]
    OutOfRange { amount: String },
}

/// Integer cents. The only money type in the core.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Cents(i64);

impl Cents {
    pub const ZERO: Cents = Cents(0);

    pub const fn new(raw: i64) -> Self {
        Cents(raw)
    }

    pub const fn raw(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, rhs: Cents) -> Result<Cents, MoneyError> {
        self.0
            .checked_add(rhs.0)
            .map(Cents)
            .ok_or(MoneyError::Overflow { op: "add" })
    }

    pub fn checked_sub(self, rhs: Cents) -> Result<Cents, MoneyError> {
        self.0
            .checked_sub(rhs.0)
            .map(Cents)
            .ok_or(MoneyError::Overflow { op: "sub" })
    }

    /// Multiply by a unitless quantity (e.g. contract count).
    pub fn checked_mul(self, qty: i64) -> Result<Cents, MoneyError> {
        self.0
            .checked_mul(qty)
            .map(Cents)
            .ok_or(MoneyError::Overflow { op: "mul" })
    }

    pub fn checked_neg(self) -> Result<Cents, MoneyError> {
        self.0
            .checked_neg()
            .map(Cents)
            .ok_or(MoneyError::Overflow { op: "neg" })
    }

    pub fn checked_abs(self) -> Result<Cents, MoneyError> {
        self.0
            .checked_abs()
            .map(Cents)
            .ok_or(MoneyError::Overflow { op: "abs" })
    }

    /// Sum a sequence, failing on the first overflow.
    pub fn checked_sum(iter: impl IntoIterator<Item = Cents>) -> Result<Cents, MoneyError> {
        iter.into_iter()
            .try_fold(Cents::ZERO, |acc, c| acc.checked_add(c))
    }

    /// Convert decimal dollars to cents, rounding toward negative infinity.
    pub fn from_dollars_floor(dollars: Decimal) -> Result<Cents, MoneyError> {
        let cents = to_cents_decimal(dollars)?;
        decimal_to_i64(cents.floor(), dollars).map(Cents)
    }

    /// Convert decimal dollars to cents, rounding toward positive infinity.
    pub fn from_dollars_ceil(dollars: Decimal) -> Result<Cents, MoneyError> {
        let cents = to_cents_decimal(dollars)?;
        decimal_to_i64(cents.ceil(), dollars).map(Cents)
    }

    /// Convert decimal dollars to cents, requiring a whole-cent amount.
    pub fn from_dollars_exact(dollars: Decimal) -> Result<Cents, MoneyError> {
        let cents = to_cents_decimal(dollars)?;
        if !cents.fract().is_zero() {
            return Err(MoneyError::SubCentRemainder {
                amount: dollars.to_string(),
            });
        }
        decimal_to_i64(cents, dollars).map(Cents)
    }

    /// Convert decimal dollars to cents with banker's rounding (round half
    /// to even). Only for venues that DOCUMENT this rounding (e.g.
    /// Polymarket US); the default modeling choice is floor/ceil against us.
    pub fn from_dollars_half_even(dollars: Decimal) -> Result<Cents, MoneyError> {
        let rounded =
            dollars.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointNearestEven);
        Self::from_dollars_exact(rounded)
    }

    /// Exact decimal-dollar view (boundary use only).
    pub fn to_dollars(self) -> Decimal {
        Decimal::new(self.0, 2)
    }
}

fn to_cents_decimal(dollars: Decimal) -> Result<Decimal, MoneyError> {
    dollars
        .checked_mul(Decimal::ONE_HUNDRED)
        .ok_or_else(|| MoneyError::OutOfRange {
            amount: dollars.to_string(),
        })
}

fn decimal_to_i64(cents: Decimal, original_dollars: Decimal) -> Result<i64, MoneyError> {
    cents.to_i64().ok_or_else(|| MoneyError::OutOfRange {
        amount: original_dollars.to_string(),
    })
}

impl fmt::Display for Cents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // i128 so i64::MIN renders without overflow.
        let v = i128::from(self.0);
        let (sign, abs) = if v < 0 { ("-", -v) } else { ("", v) };
        write!(f, "{sign}${}.{:02}", abs / 100, abs % 100)
    }
}
