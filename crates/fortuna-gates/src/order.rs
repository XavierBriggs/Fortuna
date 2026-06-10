//! The sealed `GatedOrder` type (type-level I1).
//!
//! Fields are private. There is NO public constructor anywhere; the only
//! constructor is `assemble`, pub(crate), called exclusively by the gate
//! pipeline after every check passes (T0.5). `Venue::place` accepts only this
//! type, so no order reaches a venue without passing the gates.
//!
//! Deliberately `Serialize` ONLY (for audit records). Implementing
//! `Deserialize` would be a constructor bypass and is forbidden — adding it
//! weakens I1 and the invariant tests pin this.

use fortuna_core::ids::IntentId;
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use serde::Serialize;

/// An order that has passed the full gate pipeline. Constructible only by
/// fortuna-gates.
#[derive(Debug, Clone, Serialize)]
pub struct GatedOrder {
    intent_id: IntentId,
    strategy: StrategyId,
    venue: VenueId,
    market: MarketId,
    side: Side,
    action: Action,
    limit_price: Cents,
    qty: Contracts,
    client_order_id: ClientOrderId,
}

impl GatedOrder {
    /// THE ONLY CONSTRUCTOR. pub(crate): callable solely from the gate
    /// pipeline, after every check has passed. Widening this visibility or
    /// adding any other construction path (including Deserialize) is an I1
    /// violation.
    pub(crate) fn assemble(candidate: &crate::pipeline::CandidateOrder) -> GatedOrder {
        GatedOrder {
            intent_id: candidate.intent_id,
            strategy: candidate.strategy.clone(),
            venue: candidate.venue.clone(),
            market: candidate.market.clone(),
            side: candidate.side,
            action: candidate.action,
            limit_price: candidate.limit_price,
            qty: candidate.qty,
            client_order_id: candidate.client_order_id.clone(),
        }
    }

    pub fn intent_id(&self) -> IntentId {
        self.intent_id
    }

    pub fn strategy(&self) -> &StrategyId {
        &self.strategy
    }

    pub fn venue(&self) -> &VenueId {
        &self.venue
    }

    pub fn market(&self) -> &MarketId {
        &self.market
    }

    pub fn side(&self) -> Side {
        self.side
    }

    pub fn action(&self) -> Action {
        self.action
    }

    pub fn limit_price(&self) -> Cents {
        self.limit_price
    }

    pub fn qty(&self) -> Contracts {
        self.qty
    }

    pub fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }
}
