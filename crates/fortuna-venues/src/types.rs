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
    /// Lifetime contracts traded as the venue reports it (Kalshi
    /// `volume_fp`, ceil-parsed: over-stating volume keeps sub-volume
    /// market filters conservative). `None` = the venue did not say —
    /// volume-capped strategies must SKIP, never assume small.
    #[serde(default)]
    pub volume_contracts: Option<i64>,
}

/// Catalog filter for `Venue::markets`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarketFilter {
    pub category: Option<String>,
    pub status: Option<MarketStatus>,
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
/// YES and NO lots are tracked SEPARATELY: a YES+NO pair is worth $1 at
/// settlement regardless of outcome, so netting them away would destroy
/// real value (the sum-arb strategies depend on exactly this).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenuePosition {
    pub market: MarketId,
    pub yes: i64,
    pub no: i64,
    pub cost: Cents,
}
