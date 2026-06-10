//! Order books and the fee-model contract: shared vocabulary for venues,
//! gates, exec, and state. Spec 5.2, 5.3.
//!
//! Lives in fortuna-core so the gate pipeline (which consumes books for
//! price sanity and fee models for the edge floor) never depends on the
//! venues crate (venues -> gates -> core is the dependency chain).

use crate::clock::UtcTimestamp;
use crate::market::{Action, ClientOrderId, Contracts, MarketId, Side, VenueOrderId};
use crate::money::{Cents, MoneyError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One aggregated price level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: Cents,
    pub qty: Contracts,
}

/// Canonical book form: YES bids (descending) and YES asks (ascending).
/// Adapters normalize venue-native forms (e.g. Kalshi's yes/no bid books)
/// into this. NO-side liquidity is derived: no_ask(p) == yes_bid(100-p).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderBook {
    pub market: MarketId,
    pub as_of: UtcTimestamp,
    pub yes_bids: Vec<PriceLevel>,
    pub yes_asks: Vec<PriceLevel>,
}

/// Book structural defects.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BookError {
    #[error("invalid book: {reason}")]
    Invalid { reason: String },
}

impl OrderBook {
    /// Structural hygiene: bids strictly descending, asks strictly ascending,
    /// positive quantities, prices within (0, 100) exclusive, not crossed.
    pub fn validate(&self) -> Result<(), BookError> {
        let check_levels = |levels: &[PriceLevel], descending: bool| -> Result<(), BookError> {
            for (i, l) in levels.iter().enumerate() {
                if l.qty.raw() <= 0 {
                    return Err(BookError::Invalid {
                        reason: format!("non-positive quantity at level {i}"),
                    });
                }
                if l.price.raw() <= 0 || l.price.raw() >= 100 {
                    return Err(BookError::Invalid {
                        reason: format!("price {} outside (0, 100) cents", l.price),
                    });
                }
                if i > 0 {
                    let prev = levels[i - 1].price;
                    let ordered = if descending {
                        l.price < prev
                    } else {
                        l.price > prev
                    };
                    if !ordered {
                        return Err(BookError::Invalid {
                            reason: format!("levels not strictly sorted at index {i}"),
                        });
                    }
                }
            }
            Ok(())
        };
        check_levels(&self.yes_bids, true)?;
        check_levels(&self.yes_asks, false)?;
        if let (Some(bid), Some(ask)) = (self.yes_bids.first(), self.yes_asks.first()) {
            if bid.price >= ask.price {
                return Err(BookError::Invalid {
                    reason: format!("crossed book: bid {} >= ask {}", bid.price, ask.price),
                });
            }
        }
        Ok(())
    }

    pub fn best_bid(&self) -> Option<&PriceLevel> {
        self.yes_bids.first()
    }

    pub fn best_ask(&self) -> Option<&PriceLevel> {
        self.yes_asks.first()
    }
}

/// Was the fill resting (maker) or aggressing (taker)?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRole {
    Maker,
    Taker,
}

/// Fee computation errors (venue-independent).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FeeError {
    /// Schedule config is malformed (fails at construction).
    #[error("fee config error: {reason}")]
    Config { reason: String },
    /// No fee schedule is effective at the requested time.
    #[error("no fee schedule effective at {at}")]
    NoEffectiveSchedule { at: String },
    /// Malformed inputs (price out of range, negative quantity).
    #[error("invalid fee input: {reason}")]
    Invalid { reason: String },
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// The fee interface venues expose (spec 5.2 `Venue::fee_model`) and the
/// gate pipeline consumes (spec 5.3 check 6, fee-adjusted edge floor).
pub trait FeeModel: Send + Sync {
    /// Modeled fee for a fill. `price` is the per-contract price in cents on
    /// a $1-payout binary contract; `category` selects multipliers. May be
    /// negative for documented maker rebates.
    fn fee(
        &self,
        role: FillRole,
        price: Cents,
        qty: Contracts,
        category: Option<&str>,
        at: UtcTimestamp,
    ) -> Result<Cents, FeeError>;
}

/// An execution. `fill_id` is the venue-unique dedup key (delivery is
/// at-least-once; consumers must dedup on it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fill {
    pub fill_id: String,
    pub venue_order_id: VenueOrderId,
    pub client_order_id: ClientOrderId,
    pub market: MarketId,
    pub side: Side,
    pub action: Action,
    pub price: Cents,
    pub qty: Contracts,
    pub fee: Cents,
    pub is_maker: bool,
    pub at: UtcTimestamp,
}

/// Opaque venue pagination cursor. `start()` reads from the beginning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(pub String);

impl Cursor {
    pub fn start() -> Cursor {
        Cursor(String::new())
    }
}

/// One page of fills plus the cursor to poll from next. The venue chooses
/// `next_cursor` so that nothing is ever permanently skipped; re-polling an
/// old cursor may re-deliver fills (at-least-once).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillPage {
    pub fills: Vec<Fill>,
    pub next_cursor: Cursor,
}
