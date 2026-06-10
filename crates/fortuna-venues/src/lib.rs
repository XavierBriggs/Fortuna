//! fortuna-venues: the Venue trait and adapters. Spec 5.2.
//!
//! `Venue::place` accepts only `GatedOrder` (type-level I1). `FeeModel` is a
//! config-schedule interpreter (quadratic p(1-p) | flat bps | tiered, with
//! maker/taker variants, category multipliers, effective_date versioning);
//! per-fill reconciliation of charged vs modeled fee writes a discrepancy on
//! mismatch. Market carries settlement metadata (oracle type, resolution
//! source, expected lag, benchmark anchoring inputs).
//!
//! Adapters: sim/ (seeded fault injection: ack delay, dropped/dup fills,
//! crash hooks - the DST workhorse), kalshi/ (built ONLY against
//! fixtures/kalshi/), polymarket/ (fixtures-gated; stub + GAPS entry if absent).

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

pub mod fees;
pub mod kalshi;
pub mod sim;
mod types;

use async_trait::async_trait;
use fortuna_core::market::{MarketId, VenueId, VenueOrderId};
use fortuna_core::money::{Cents, MoneyError};
use fortuna_gates::GatedOrder;
use thiserror::Error;

pub use fortuna_core::book::{
    Cursor, FeeError, FeeModel, Fill, FillPage, FillRole, OrderBook, PriceLevel,
};
pub use types::{Market, MarketFilter, MarketStatus, OpenOrder, SettlementMeta, VenuePosition};

/// Venue adapter errors.
#[derive(Debug, Error)]
pub enum VenueError {
    /// The venue (or its API) is unreachable. No effect can be assumed.
    #[error("venue {venue} unavailable: {reason}")]
    Outage { venue: String, reason: String },
    /// CRITICAL SEMANTICS: the operation may or may not have taken effect.
    /// Callers must reconcile (poll orders/fills) before assuming either.
    #[error("operation {operation} timed out; effect unknown")]
    Timeout { operation: String },
    /// The venue refused the request; no effect.
    #[error("rejected: {reason}")]
    Rejected { reason: String },
    /// An order with this client order id already exists (Kalshi returns
    /// ORDER_ALREADY_EXISTS). For crash resubmission this is
    /// success-equivalent: the original order is live under `existing`.
    #[error("order already exists as {existing}")]
    AlreadyExists { existing: VenueOrderId },
    #[error("not found: {what}")]
    NotFound { what: String },
    #[error("rate limited")]
    RateLimited,
    /// Malformed request or data; no effect.
    #[error("invalid: {reason}")]
    Invalid { reason: String },
    #[error(transparent)]
    Fee(#[from] fortuna_core::book::FeeError),
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// The venue adapter contract (spec 5.2). `place` accepts only `GatedOrder`:
/// no order reaches a venue without passing the gate pipeline (I1).
///
/// `balance` is an addition to the spec's abbreviated trait sketch: spec 5.4
/// and 5.14 make venue balances authoritative for reconciliation, so the
/// trait must surface them (see ASSUMPTIONS.md).
#[async_trait]
pub trait Venue: Send + Sync {
    fn id(&self) -> VenueId;
    async fn markets(&self, filter: MarketFilter) -> Result<Vec<Market>, VenueError>;
    async fn book(&self, market: &MarketId) -> Result<OrderBook, VenueError>;
    async fn place(&self, order: GatedOrder) -> Result<VenueOrderId, VenueError>;
    async fn cancel(&self, id: &VenueOrderId) -> Result<(), VenueError>;
    async fn positions(&self) -> Result<Vec<VenuePosition>, VenueError>;
    /// Open (working) orders as the venue sees them. Spec 5.4 boot
    /// reconciliation requires this; the 5.2 sketch omits it (ASSUMPTIONS).
    async fn open_orders(&self) -> Result<Vec<OpenOrder>, VenueError>;
    /// Available (unreserved) cash as reported by the venue.
    async fn balance(&self) -> Result<Cents, VenueError>;
    /// Fills at or after `cursor`. Delivery is at-least-once: pages may
    /// re-deliver; consumers dedup on `fill_id`.
    async fn fills_since(&self, cursor: Cursor) -> Result<FillPage, VenueError>;
    fn fee_model(&self) -> &dyn fortuna_core::book::FeeModel;
}
