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
pub mod kinetics;
pub mod polymarket;
pub mod redact;
pub mod sim;
pub mod stream;
mod types;

use async_trait::async_trait;
use fortuna_core::market::{MarketId, VenueId, VenueOrderId};
use fortuna_core::money::{Cents, MoneyError};
use fortuna_gates::GatedOrder;
use thiserror::Error;

pub use fortuna_core::book::{
    Cursor, FeeError, FeeModel, Fill, FillPage, FillRole, OrderBook, PriceLevel,
};
pub use types::{
    Market, MarketFilter, MarketStatus, OpenOrder, SettlementMeta, SettlementNotice,
    SettlementOutcome, SettlementPage, VenuePosition,
};

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
    /// The adapter is a fixtures-gated stub: no operator-recorded
    /// fixtures exist and venue behavior is never invented, so every
    /// operation refuses (T3.4 discipline; GAPS has the unblock).
    #[error("venue {venue} is fixture-gated: no recorded fixtures, no invented behavior")]
    FixtureGated { venue: String },
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
    /// Optional pre-tick market-data refresh hook. Venues that are already
    /// authoritative on `book()` can keep the default no-op; composite paper
    /// venues can use it to ingest live reads into local execution state before
    /// strategies see the tick's quotes.
    async fn refresh_market_data(&self) -> Result<(), VenueError> {
        Ok(())
    }
    /// Scoped variant of the pre-tick refresh hook. Runners pass the exact
    /// market universe they will poll this tick; venues that can refresh only
    /// those markets should override this. The default preserves existing
    /// venue behavior.
    async fn refresh_market_data_for_markets(
        &self,
        _markets: &[MarketId],
    ) -> Result<(), VenueError> {
        self.refresh_market_data().await
    }
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
    /// Available cash AND the capital currently RESERVED (committed to working
    /// orders) — the drawdown monitor (I2) and the account dashboard read this.
    /// Default: `(balance(), 0)` for venues with no separate reservation-ledger
    /// API (every real venue + the fixture-gated stubs, which propagate their
    /// `balance()` refusal). `SimVenue` OVERRIDES this to surface its exact
    /// reserved capital, so the sim path's numbers stay byte-identical to the
    /// prior `inspect_totals` read (A3).
    async fn account(&self) -> Result<(Cents, Cents), VenueError> {
        Ok((self.balance().await?, Cents::ZERO))
    }
    /// Fills at or after `cursor`. Delivery is at-least-once: pages may
    /// re-deliver; consumers dedup on `fill_id`.
    async fn fills_since(&self, cursor: Cursor) -> Result<FillPage, VenueError>;
    /// Settlement notices at or after `cursor` (spec 5.13: settlement is
    /// asynchronous and adversarial; FORTUNA never assumes it, only
    /// reconciles this stream). At-least-once; dedup on `notice_id`.
    /// Corrections arrive as NEW notices for the same market, never edits.
    async fn settlements_since(&self, cursor: Cursor) -> Result<SettlementPage, VenueError>;
    fn fee_model(&self) -> &dyn fortuna_core::book::FeeModel;
}
