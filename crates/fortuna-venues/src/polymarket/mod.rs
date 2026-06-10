//! Polymarket US adapter slot (T3.4): FIXTURES-GATED STUB.
//!
//! No operator-recorded Polymarket fixtures exist (fixtures/ holds
//! kalshi only) and venue API behavior is NEVER invented (CLAUDE.md:
//! "Never invent venue API behavior. Build adapters against
//! fixtures/... recordings. If a fixture is missing, stub behind the
//! trait, record the need in GAPS.md, move on").
//!
//! This stub is the trait slot the composition can name; every
//! operation refuses with `VenueError::FixtureGated` — it never
//! fabricates market data, never accepts an order, and its fee model
//! refuses every computation (a fabricated zero fee would inflate every
//! edge through the gates). The unblock (operator fixture recording +
//! a venue research loop under docs/research/venue/) is in GAPS.md.

use crate::{
    Cursor, FillPage, Market, MarketFilter, OpenOrder, SettlementPage, Venue, VenueError,
    VenuePosition,
};
use async_trait::async_trait;
use fortuna_core::book::{FeeError, FeeModel, FillRole, OrderBook};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Contracts, MarketId, VenueId, VenueOrderId};
use fortuna_core::money::Cents;
use fortuna_gates::GatedOrder;

const VENUE: &str = "polymarket_us";

fn gate<T>() -> Result<T, VenueError> {
    Err(VenueError::FixtureGated {
        venue: VENUE.to_string(),
    })
}

/// A fee model that refuses every computation: a stub venue must never
/// feed a fabricated fee into sizing or the gate pipeline.
#[derive(Debug)]
pub struct RefusingFeeModel;

impl FeeModel for RefusingFeeModel {
    fn fee(
        &self,
        _role: FillRole,
        _price: Cents,
        _qty: Contracts,
        _category: Option<&str>,
        _at: UtcTimestamp,
    ) -> Result<Cents, FeeError> {
        Err(FeeError::Config {
            reason: format!("{VENUE} is fixture-gated: no fee schedule may be invented"),
        })
    }
}

/// The fixtures-gated Polymarket US slot.
#[derive(Debug)]
pub struct PolymarketUsStub {
    id: VenueId,
    fees: RefusingFeeModel,
}

impl PolymarketUsStub {
    /// Construction validates the static id once (no panic path exists
    /// anywhere in this adapter, stub or not).
    pub fn new() -> Result<PolymarketUsStub, VenueError> {
        let id = VenueId::new(VENUE).map_err(|e| VenueError::Invalid {
            reason: e.to_string(),
        })?;
        Ok(PolymarketUsStub {
            id,
            fees: RefusingFeeModel,
        })
    }
}

#[async_trait]
impl Venue for PolymarketUsStub {
    fn id(&self) -> VenueId {
        self.id.clone()
    }
    async fn markets(&self, _filter: MarketFilter) -> Result<Vec<Market>, VenueError> {
        gate()
    }
    async fn book(&self, _market: &MarketId) -> Result<OrderBook, VenueError> {
        gate()
    }
    async fn place(&self, _order: GatedOrder) -> Result<VenueOrderId, VenueError> {
        gate()
    }
    async fn cancel(&self, _id: &VenueOrderId) -> Result<(), VenueError> {
        gate()
    }
    async fn positions(&self) -> Result<Vec<VenuePosition>, VenueError> {
        gate()
    }
    async fn open_orders(&self) -> Result<Vec<OpenOrder>, VenueError> {
        gate()
    }
    async fn balance(&self) -> Result<Cents, VenueError> {
        gate()
    }
    async fn fills_since(&self, _cursor: Cursor) -> Result<FillPage, VenueError> {
        gate()
    }
    async fn settlements_since(&self, _cursor: Cursor) -> Result<SettlementPage, VenueError> {
        gate()
    }
    fn fee_model(&self) -> &dyn FeeModel {
        &self.fees
    }
}
