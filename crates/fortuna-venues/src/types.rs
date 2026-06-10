//! Market-data structures shared by all venue adapters. Spec 5.2, 5.13.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{
    Action, ClientOrderId, Contracts, MarketId, Side, VenueId, VenueOrderId,
};
use fortuna_core::money::Cents;
use serde::{Deserialize, Serialize};

/// Market lifecycle states (spec 5.13: venue projection of an event).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketStatus {
    Listed,
    Trading,
    Halted,
    Expired,
    Determined,
    Settled,
    Voided,
}

/// Settlement metadata (spec 5.2): oracle-delay artifacts must be excludable
/// from edge scans, so every market carries its resolution mechanics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementMeta {
    pub oracle_type: String,
    pub resolution_source: String,
    pub expected_lag_hours: u32,
}

/// A venue market snapshot. `title` is UNTRUSTED external text (spec 5.11):
/// data, never instructions; it must never be interpolated into anything
/// executable and reaches model context only as quoted data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Market {
    pub id: MarketId,
    pub venue: VenueId,
    pub title: String,
    pub category: String,
    pub status: MarketStatus,
    pub close_at: Option<UtcTimestamp>,
    pub settlement: SettlementMeta,
    pub payout_per_contract: Cents,
}

/// Catalog filter for `Venue::markets`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarketFilter {
    pub category: Option<String>,
    pub status: Option<MarketStatus>,
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

/// A working order as the venue reports it (boot reconciliation input).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenOrder {
    pub venue_order_id: VenueOrderId,
    pub client_order_id: ClientOrderId,
    pub market: MarketId,
    pub side: Side,
    pub action: Action,
    pub limit_price: Cents,
    pub remaining_qty: Contracts,
}

/// Venue-reported position (venue truth; reconciled against local state).
/// `net_yes` > 0 is long YES, < 0 is long NO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenuePosition {
    pub market: MarketId,
    pub net_yes: i64,
    pub cost: Cents,
}
