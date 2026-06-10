//! Shared market vocabulary: identifier newtypes, sides, quantities.
//!
//! Lives in fortuna-core so the dependency chain stays clean:
//! fortuna-venues -> fortuna-gates -> fortuna-core. Market *data* structures
//! (books, markets, fills) live in fortuna-venues; the gate pipeline and
//! exec only need this vocabulary.

use crate::ids::IntentId;
use crate::money::{Cents, MoneyError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors from vocabulary construction.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MarketTypeError {
    #[error("{what} must be a non-empty string")]
    EmptyId { what: &'static str },
}

macro_rules! define_string_id {
    ($(#[$meta:meta])* $name:ident, $what:literal) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(raw: impl Into<String>) -> Result<Self, MarketTypeError> {
                let raw = raw.into();
                if raw.trim().is_empty() {
                    return Err(MarketTypeError::EmptyId { what: $what });
                }
                Ok(Self(raw))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = MarketTypeError;
            fn try_from(s: String) -> Result<Self, Self::Error> {
                Self::new(s)
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }
    };
}

define_string_id!(
    /// A venue ("kalshi", "polymarket_us", "sim"). Lowercase snake by convention.
    VenueId,
    "venue id"
);
define_string_id!(
    /// A venue-scoped market identifier (ticker).
    MarketId,
    "market id"
);
define_string_id!(
    /// A strategy ("mech_structural", ...).
    StrategyId,
    "strategy id"
);
define_string_id!(
    /// A venue-assigned order identifier.
    VenueOrderId,
    "venue order id"
);

/// Binary-market side. YES pays out if the event resolves true.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Yes,
    No,
}

impl Side {
    pub const fn opposite(self) -> Side {
        match self {
            Side::Yes => Side::No,
            Side::No => Side::Yes,
        }
    }
}

/// Order action. Sell closes held contracts (venue-conservative semantics;
/// confirmed against fixtures in T1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Buy,
    Sell,
}

/// Contract count. Plain checked integer; sign conventions belong to the
/// position layer (positive = long the named side).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Contracts(i64);

impl Contracts {
    pub const ZERO: Contracts = Contracts(0);

    pub const fn new(raw: i64) -> Self {
        Contracts(raw)
    }

    pub const fn raw(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, rhs: Contracts) -> Result<Contracts, MoneyError> {
        self.0
            .checked_add(rhs.0)
            .map(Contracts)
            .ok_or(MoneyError::Overflow {
                op: "contracts add",
            })
    }

    pub fn checked_sub(self, rhs: Contracts) -> Result<Contracts, MoneyError> {
        self.0
            .checked_sub(rhs.0)
            .map(Contracts)
            .ok_or(MoneyError::Overflow {
                op: "contracts sub",
            })
    }
}

impl std::fmt::Display for Contracts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Order cost before fees: price x quantity, checked.
pub fn notional(price: Cents, qty: Contracts) -> Result<Cents, MoneyError> {
    price.checked_mul(qty.raw())
}

/// Client order id sent to venues for idempotency. Derived deterministically
/// from the intent id (spec 5.4): resubmission after a crash reuses the same
/// id by construction.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientOrderId(String);

impl ClientOrderId {
    pub fn from_intent(intent: IntentId) -> Self {
        ClientOrderId(format!("fortuna-{intent}"))
    }

    /// Adapter/test constructor. Production order flow derives ids via
    /// `from_intent` (spec 5.4 idempotency); this exists for venue-side
    /// bookkeeping and tests.
    pub fn new(raw: impl Into<String>) -> Result<Self, MarketTypeError> {
        let raw = raw.into();
        if raw.trim().is_empty() {
            return Err(MarketTypeError::EmptyId {
                what: "client order id",
            });
        }
        Ok(ClientOrderId(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ClientOrderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
