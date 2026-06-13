//! The Kinetics perps adapter (T5.B4 slice 3; spec 5.15).
//!
//! I1 at the type level: `place` accepts ONLY `GatedPerpOrder` — the
//! sealed product of the perp gate arm. No order reaches this venue
//! without passing the gates.
//!
//! Venue rules enforced HERE, fail-closed before the wire:
//!
//! - reduce_only requires IOC or FOK (fixture orders__reduce_only_gtc:
//!   400 invalid_order "reduce_only can only be used with IoC or FoK
//!   orders") — a reduce-only GTC never leaves the process.
//! - A duplicate client_order_id (409 order_already_exists) resolves via
//!   a first-page order-list scan for the matching client id: crash
//!   resubmission then returns `AlreadyExists { existing }` exactly like
//!   the event-API adapter. An unresolvable duplicate stays `Rejected`
//!   (first-page-only scan; pagination gap ledgered in GAPS).
//! - SYSTEM fills (order_source = "system" — the venue's liquidation
//!   executions, research §6/P1) are a DISTINCT class
//!   (`PerpFillClass::Liquidation`): callers must match on them; they
//!   are never silently absorbed into user-fill flow (spec 5.15 — the
//!   mandatory alert + halt evaluation is the caller's next move).
//! - Every fill reconciles charged fees against the venue's posted
//!   fee_tiers rates (maker/taker by is_taker, modeled CEIL against
//!   us); mismatches yield `FeeDiscrepancy` records. NOTE: the recorded
//!   demo fill (fees 0.0000) MISMATCHES the posted tiers (0.0012 taker)
//!   — that is the promo-$0 reality and exactly what the fee-trap rule
//!   (spec 5.15) exists to surface; the discrepancy on recorded data is
//!   correct output, not noise.
//!
//! Money conversions round against us at every boundary: charged fees
//! and margin numbers CEIL; unrealized/realized PnL FLOOR.

use crate::kinetics::client::{BookSide, CreateOrderRequest, KineticsClient, TimeInForce};
use crate::kinetics::dto;
use crate::VenueError;
use fortuna_core::market::{Action, Contracts, MarketId, VenueOrderId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{PerpPosition, PerpPrice};
use fortuna_gates::perp::GatedPerpOrder;
use rust_decimal::Decimal;

/// One accepted placement.
#[derive(Debug, Clone)]
pub struct PerpPlacement {
    pub venue_order_id: VenueOrderId,
    pub filled: Contracts,
    pub remaining: Contracts,
}

/// A typed fill with integer-money conversions applied.
#[derive(Debug, Clone)]
pub struct PerpFill {
    pub fill_id: String,
    pub order_id: String,
    pub market: MarketId,
    /// bid -> Buy, ask -> Sell.
    pub action: Action,
    pub price: PerpPrice,
    pub count: Contracts,
    /// Charged fee, CEILED (never understated).
    pub fee: Cents,
    /// Realized PnL, FLOORED (never overstated).
    pub realized_pnl: Cents,
    pub is_taker: bool,
    pub created_time: String,
}

/// User fills vs venue-originated (liquidation) fills — a DISTINCT class
/// the caller must handle explicitly (spec 5.15).
#[derive(Debug, Clone)]
pub enum PerpFillClass {
    User(PerpFill),
    /// order_source = "system": the clearinghouse executed this. The
    /// margin model was wrong somewhere; alert + halt evaluation follow.
    Liquidation(PerpFill),
}

/// A charged-vs-modeled fee mismatch (writes a discrepancy upstream).
#[derive(Debug, Clone)]
pub struct FeeDiscrepancy {
    pub fill_id: String,
    pub modeled: Cents,
    pub charged: Cents,
}

/// A typed position read with conservative money conversions.
#[derive(Debug, Clone)]
pub struct KineticsPosition {
    pub position: PerpPosition,
    /// Fees paid to date, CEILED.
    pub fees_paid: Cents,
    /// Venue-reported margin in use, CEILED (exposure never understated).
    pub margin_used: Cents,
    /// Venue-reported unrealized PnL, FLOORED.
    pub unrealized_pnl: Cents,
}

pub struct KineticsAdapter {
    client: KineticsClient,
}

impl KineticsAdapter {
    pub fn new(client: KineticsClient) -> Self {
        KineticsAdapter { client }
    }

    /// Place a gated perp order. Time-in-force and post_only are
    /// execution policy (I6: timing and order type belong to the
    /// harness), supplied by the exec layer — but venue RULES bind here:
    /// reduce_only with GTC is refused before the wire.
    pub async fn place(
        &self,
        order: &GatedPerpOrder,
        time_in_force: TimeInForce,
        post_only: Option<bool>,
    ) -> Result<PerpPlacement, VenueError> {
        if order.reduce_only() && time_in_force == TimeInForce::GoodTillCanceled {
            return Err(VenueError::Invalid {
                reason: "reduce_only requires IOC or FOK (venue rule, fixture \
                         orders__reduce_only_gtc); refusing before the wire"
                    .into(),
            });
        }
        let request = CreateOrderRequest {
            ticker: order.market().as_str().to_string(),
            side: match order.action() {
                Action::Buy => BookSide::Bid,
                Action::Sell => BookSide::Ask,
            },
            price: order.limit_price(),
            count: order.qty().raw(),
            client_order_id: order.client_order_id().as_str().to_string(),
            time_in_force,
            post_only,
            reduce_only: order.reduce_only().then_some(true),
            order_group_id: None,
        };
        match self.client.create_order(&request).await {
            Ok(resp) => Ok(PerpPlacement {
                venue_order_id: VenueOrderId::new(&resp.order_id).map_err(|e| {
                    VenueError::Invalid {
                        reason: format!("venue order id: {e}"),
                    }
                })?,
                filled: dto::parse_whole_count(&resp.fill_count)?,
                remaining: dto::parse_whole_count(&resp.remaining_count)?,
            }),
            Err(VenueError::Rejected { reason }) if reason.starts_with("order_already_exists") => {
                self.resolve_duplicate(order, reason).await
            }
            Err(e) => Err(e),
        }
    }

    /// Crash-resubmission support: a duplicate client id resolves to the
    /// EXISTING order via a first-page list scan (the 409 body carries no
    /// order id). Not found on the first page -> the original rejection
    /// stands (pagination gap ledgered).
    async fn resolve_duplicate(
        &self,
        order: &GatedPerpOrder,
        original_reason: String,
    ) -> Result<PerpPlacement, VenueError> {
        let listing = self.client.list_orders(None, Some(100)).await?;
        let wanted = order.client_order_id().as_str();
        for existing in &listing.orders {
            if existing.client_order_id == wanted {
                return Err(VenueError::AlreadyExists {
                    existing: VenueOrderId::new(&existing.order_id).map_err(|e| {
                        VenueError::Invalid {
                            reason: format!("venue order id: {e}"),
                        }
                    })?,
                });
            }
        }
        Err(VenueError::Rejected {
            reason: format!("{original_reason}; client id not on the first list page"),
        })
    }

    pub async fn cancel(&self, order_id: &str) -> Result<Contracts, VenueError> {
        let resp = self.client.cancel_order(order_id).await?;
        dto::parse_whole_count(&resp.reduced_by)
    }

    /// Fills with system-fill classification and per-fill fee
    /// reconciliation against the posted tiers (caller supplies tiers;
    /// they change rarely and the fee-trap rule wants them EXPLICIT).
    pub async fn fills_reconciled(
        &self,
        ticker: Option<&str>,
        limit: Option<i64>,
        tiers: &dto::FeeTiersResponse,
    ) -> Result<(Vec<PerpFillClass>, Vec<FeeDiscrepancy>), VenueError> {
        let resp = self.client.fills(ticker, limit).await?;
        let mut fills = Vec::with_capacity(resp.fills.len());
        let mut discrepancies = Vec::new();
        for raw in &resp.fills {
            let fill = typed_fill(raw)?;
            if let Some(d) = reconcile_fee(raw, &fill, tiers)? {
                discrepancies.push(d);
            }
            if raw.order_source == "system" {
                fills.push(PerpFillClass::Liquidation(fill));
            } else {
                fills.push(PerpFillClass::User(fill));
            }
        }
        Ok((fills, discrepancies))
    }

    pub async fn positions(&self) -> Result<Vec<KineticsPosition>, VenueError> {
        let resp = self.client.positions().await?;
        resp.positions.iter().map(typed_position).collect()
    }

    pub fn client(&self) -> &KineticsClient {
        &self.client
    }
}

fn typed_fill(raw: &dto::Fill) -> Result<PerpFill, VenueError> {
    Ok(PerpFill {
        fill_id: raw.fill_id.clone(),
        order_id: raw.order_id.clone(),
        market: MarketId::new(&raw.ticker).map_err(|e| VenueError::Invalid {
            reason: format!("fill ticker: {e}"),
        })?,
        action: match raw.side.as_str() {
            "bid" => Action::Buy,
            "ask" => Action::Sell,
            other => {
                return Err(VenueError::Invalid {
                    reason: format!("fill side {other:?} is neither bid nor ask"),
                })
            }
        },
        price: dto::parse_perp_price(&raw.price)?,
        count: dto::parse_whole_count(&raw.count)?,
        fee: Cents::from_dollars_ceil(dto::parse_dollars(&raw.fees)?).map_err(|e| {
            VenueError::Invalid {
                reason: format!("fill fee: {e}"),
            }
        })?,
        realized_pnl: Cents::from_dollars_floor(dto::parse_dollars(&raw.realized_pnl)?).map_err(
            |e| VenueError::Invalid {
                reason: format!("fill realized pnl: {e}"),
            },
        )?,
        is_taker: raw.is_taker,
        created_time: raw.created_time.clone(),
    })
}

/// Modeled fee = notional dollars x posted tier rate (maker/taker by
/// is_taker), CEILED to cents. Charged != modeled -> discrepancy. An
/// unknown ticker in the tier tables is itself a discrepancy-grade
/// failure: fail closed.
fn reconcile_fee(
    raw: &dto::Fill,
    fill: &PerpFill,
    tiers: &dto::FeeTiersResponse,
) -> Result<Option<FeeDiscrepancy>, VenueError> {
    let table = if raw.is_taker {
        &tiers.taker_fee_rates
    } else {
        &tiers.maker_fee_rates
    };
    let rate = table.get(&raw.ticker).ok_or_else(|| VenueError::Invalid {
        reason: format!("no posted fee tier for {}: cannot reconcile", raw.ticker),
    })?;
    let rate = Decimal::try_from(*rate).map_err(|e| VenueError::Invalid {
        reason: format!("fee rate {rate} not exact: {e}"),
    })?;
    let notional_dollars = dto::parse_dollars(&raw.price)?
        .checked_mul(Decimal::from(fill.count.raw()))
        .ok_or_else(|| VenueError::Invalid {
            reason: "fee notional overflow".into(),
        })?;
    let modeled_dollars =
        notional_dollars
            .checked_mul(rate)
            .ok_or_else(|| VenueError::Invalid {
                reason: "fee model overflow".into(),
            })?;
    let modeled = Cents::from_dollars_ceil(modeled_dollars).map_err(|e| VenueError::Invalid {
        reason: format!("modeled fee: {e}"),
    })?;
    if modeled != fill.fee {
        return Ok(Some(FeeDiscrepancy {
            fill_id: fill.fill_id.clone(),
            modeled,
            charged: fill.fee,
        }));
    }
    Ok(None)
}

fn typed_position(raw: &dto::Position) -> Result<KineticsPosition, VenueError> {
    Ok(KineticsPosition {
        position: PerpPosition {
            market: MarketId::new(&raw.market_ticker).map_err(|e| VenueError::Invalid {
                reason: format!("position ticker: {e}"),
            })?,
            qty: dto::parse_whole_count(&raw.position)?,
            avg_entry: dto::parse_perp_price(&raw.entry_price)?,
        },
        fees_paid: Cents::from_dollars_ceil(dto::parse_dollars(&raw.fees)?).map_err(|e| {
            VenueError::Invalid {
                reason: format!("position fees: {e}"),
            }
        })?,
        margin_used: Cents::from_dollars_ceil(dto::parse_dollars(&raw.margin_used)?).map_err(
            |e| VenueError::Invalid {
                reason: format!("margin used: {e}"),
            },
        )?,
        unrealized_pnl: Cents::from_dollars_floor(dto::parse_dollars(&raw.unrealized_pnl)?)
            .map_err(|e| VenueError::Invalid {
                reason: format!("unrealized pnl: {e}"),
            })?,
    })
}
