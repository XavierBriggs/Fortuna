//! fortuna-paper: the paper-fill engine. Spec Section 11.
//!
//! Paper trading exists to make GO/NO-GO numbers honest. The two rules,
//! verbatim from the spec, are load-bearing:
//!
//! - "maker fills in paper count ONLY when the market trades through the
//!   limit price (not touches), with a configurable quantity haircut" —
//!   touch-fill optimism is the classic paper-trading inflation and would
//!   corrupt every promotion gate downstream.
//! - "taker paper fills assume crossing the visible book at displayed
//!   depth, never mid."
//!
//! `PaperVenue` implements the same `Venue` trait as live adapters, so the
//! Strategy/exec interface cannot tell the difference (parity requirement).
//! Market data is PUSHED in (`apply_book`, `apply_public_trade`) from
//! recorded or streamed feeds; the engine is deterministic and fault-free
//! (fault injection is the sim venue's job).
//!
//! One print's haircut budget is shared FIFO across our resting orders on
//! that market: two of our orders cannot both absorb the same public volume.

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

use async_trait::async_trait;
use fortuna_core::book::{FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::Clock;
use fortuna_core::market::{
    notional, Action, ClientOrderId, Contracts, MarketId, Side, VenueId, VenueOrderId,
};
use fortuna_core::money::Cents;
use fortuna_gates::GatedOrder;
use fortuna_venues::fees::ScheduleFeeModel;
use fortuna_venues::{
    Cursor, Fill, FillPage, Market, MarketFilter, MarketStatus, OpenOrder, Venue, VenueError,
    VenuePosition,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Paper-engine configuration.
#[derive(Debug, Clone)]
pub struct PaperConfig {
    /// Percentage (1..=100) of a through-print's quantity our resting
    /// orders may absorb. Spec 11's "configurable quantity haircut".
    pub maker_haircut_pct: u8,
}

#[derive(Debug, Clone)]
struct OrderReq {
    market: MarketId,
    side: Side,
    action: Action,
    limit_price: Cents,
    qty: Contracts,
    client_order_id: ClientOrderId,
}

#[derive(Debug, Clone)]
struct RestingOrder {
    id: VenueOrderId,
    req: OrderReq,
    remaining: Contracts,
    reserved: Cents,
}

#[derive(Debug, Clone, Default)]
struct Pos {
    yes: i64,
    no: i64,
    cost: Cents,
}

struct State {
    markets: BTreeMap<MarketId, Market>,
    books: BTreeMap<MarketId, (Vec<PriceLevel>, Vec<PriceLevel>)>,
    resting: Vec<RestingOrder>,
    fills: Vec<Fill>,
    by_coid: BTreeMap<String, VenueOrderId>,
    positions: BTreeMap<MarketId, Pos>,
    cash: Cents,
    reserved: Cents,
    next_seq: u64,
}

/// The paper venue: real `Venue` interface, simulated fills, pushed data.
pub struct PaperVenue {
    venue_id: VenueId,
    clock: Arc<dyn Clock>,
    fees: ScheduleFeeModel,
    config: PaperConfig,
    state: Mutex<State>,
}

impl PaperVenue {
    pub fn new(
        venue_id: VenueId,
        clock: Arc<dyn Clock>,
        fees: ScheduleFeeModel,
        config: PaperConfig,
        starting_cash: Cents,
    ) -> Result<PaperVenue, VenueError> {
        if config.maker_haircut_pct == 0 || config.maker_haircut_pct > 100 {
            return Err(VenueError::Invalid {
                reason: format!(
                    "maker_haircut_pct must be in 1..=100, got {}",
                    config.maker_haircut_pct
                ),
            });
        }
        Ok(PaperVenue {
            venue_id,
            clock,
            fees,
            config,
            state: Mutex::new(State {
                markets: BTreeMap::new(),
                books: BTreeMap::new(),
                resting: Vec::new(),
                fills: Vec::new(),
                by_coid: BTreeMap::new(),
                positions: BTreeMap::new(),
                cash: starting_cash,
                reserved: Cents::ZERO,
                next_seq: 0,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn add_market(&self, market: Market) {
        let mut st = self.lock();
        st.books
            .entry(market.id.clone())
            .or_insert_with(|| (Vec::new(), Vec::new()));
        st.markets.insert(market.id.clone(), market);
    }

    /// Push a fresh canonical book (from the recorded/streamed feed).
    pub fn apply_book(
        &self,
        market: &MarketId,
        yes_bids: Vec<PriceLevel>,
        yes_asks: Vec<PriceLevel>,
    ) -> Result<(), VenueError> {
        let mut st = self.lock();
        if !st.markets.contains_key(market) {
            return Err(VenueError::NotFound {
                what: format!("market {market}"),
            });
        }
        let book = OrderBook {
            market: market.clone(),
            as_of: self.clock.now(),
            yes_bids,
            yes_asks,
        };
        book.validate().map_err(|e| VenueError::Invalid {
            reason: e.to_string(),
        })?;
        st.books
            .insert(market.clone(), (book.yes_bids, book.yes_asks));
        Ok(())
    }

    /// Push one public trade print (yes-space price). THE maker-fill rule
    /// lives here: only prints strictly THROUGH a resting limit fill it,
    /// at most haircut% of the print's quantity shared FIFO across our
    /// orders on the market.
    pub fn apply_public_trade(
        &self,
        market: &MarketId,
        yes_price: Cents,
        qty: i64,
    ) -> Result<Vec<Fill>, VenueError> {
        if qty <= 0 || !(1..=99).contains(&yes_price.raw()) {
            return Err(VenueError::Invalid {
                reason: format!("bad print: {yes_price} x {qty}"),
            });
        }
        let mut st = self.lock();
        // The shared budget this print can fill of OUR orders.
        let mut budget = qty
            .checked_mul(i64::from(self.config.maker_haircut_pct))
            .map(|x| x / 100)
            .unwrap_or(0);
        let mut fills = Vec::new();
        let mut idx = 0;
        while idx < st.resting.len() && budget > 0 {
            let (matches, take) = {
                let r = &st.resting[idx];
                if &r.req.market != market {
                    (false, 0)
                } else {
                    let (is_bid, yes_limit) =
                        yes_space(r.req.side, r.req.action, r.req.limit_price)?;
                    // STRICTLY through, never at touch (spec 11).
                    let through = if is_bid {
                        yes_price < yes_limit
                    } else {
                        yes_price > yes_limit
                    };
                    if through {
                        (true, r.remaining.raw().min(budget))
                    } else {
                        (false, 0)
                    }
                }
            };
            if matches && take > 0 {
                let fill = self.fill_resting(&mut st, idx, take, true)?;
                budget -= take;
                fills.push(fill);
                // fill_resting may remove the order; only advance when it
                // survived (partial).
                if idx < st.resting.len() && st.resting[idx].remaining.raw() > 0 {
                    idx += 1;
                }
            } else {
                idx += 1;
            }
        }
        Ok(fills)
    }

    /// Settle a market (paper GO metrics need realized outcomes).
    pub fn settle_market(&self, market: &MarketId, winner: Side) -> Result<Cents, VenueError> {
        let mut st = self.lock();
        let payout_per = {
            let m = st.markets.get(market).ok_or_else(|| VenueError::NotFound {
                what: format!("market {market}"),
            })?;
            if m.status == MarketStatus::Settled {
                return Err(VenueError::Rejected {
                    reason: format!("market {market} already settled"),
                });
            }
            m.payout_per_contract
        };
        let payout = match st.positions.get(market) {
            Some(pos) => {
                let winning = match winner {
                    Side::Yes => pos.yes.max(0),
                    Side::No => pos.no.max(0),
                };
                payout_per.checked_mul(winning).map_err(VenueError::Money)?
            }
            None => Cents::ZERO,
        };
        let new_cash = st.cash.checked_add(payout).map_err(VenueError::Money)?;
        let resting = std::mem::take(&mut st.resting);
        let mut kept = Vec::with_capacity(resting.len());
        for r in resting {
            if &r.req.market == market {
                st.reserved = st
                    .reserved
                    .checked_sub(r.reserved)
                    .map_err(VenueError::Money)?;
            } else {
                kept.push(r);
            }
        }
        st.resting = kept;
        st.positions.remove(market);
        st.cash = new_cash;
        if let Some(m) = st.markets.get_mut(market) {
            m.status = MarketStatus::Settled;
        }
        st.books.remove(market);
        Ok(payout)
    }

    fn worst_case_cost(&self, req: &OrderReq, category: &str) -> Result<Cents, VenueError> {
        let cost = notional(req.limit_price, req.qty).map_err(VenueError::Money)?;
        let at = self.clock.now();
        let taker = self.fees.fee(
            FillRole::Taker,
            req.limit_price,
            req.qty,
            Some(category),
            at,
        )?;
        let maker = self.fees.fee(
            FillRole::Maker,
            req.limit_price,
            req.qty,
            Some(category),
            at,
        )?;
        cost.checked_add(taker.max(maker).max(Cents::ZERO))
            .map_err(VenueError::Money)
    }

    /// Taker phase: cross visible displayed depth, never mid. Then rest.
    fn execute_order(
        &self,
        st: &mut State,
        id: VenueOrderId,
        req: OrderReq,
    ) -> Result<(), VenueError> {
        let mut remaining = req.qty;
        while remaining.raw() > 0 {
            let Some((fill_price, level_qty)) = best_counter_level(st, &req)? else {
                break;
            };
            let take = remaining.raw().min(level_qty);
            self.apply_fill(st, &id, &req, fill_price, take, false)?;
            consume_counter_level(st, &req, take);
            remaining = Contracts::new(remaining.raw() - take);
        }
        if remaining.raw() > 0 {
            let reserved = if req.action == Action::Buy {
                let mut rest = req.clone();
                rest.qty = remaining;
                let category = st
                    .markets
                    .get(&req.market)
                    .map(|m| m.category.clone())
                    .unwrap_or_default();
                self.worst_case_cost(&rest, &category)?
            } else {
                Cents::ZERO
            };
            st.reserved = st
                .reserved
                .checked_add(reserved)
                .map_err(VenueError::Money)?;
            st.resting.push(RestingOrder {
                id,
                req,
                remaining,
                reserved,
            });
        }
        Ok(())
    }

    fn fill_resting(
        &self,
        st: &mut State,
        idx: usize,
        take: i64,
        is_maker: bool,
    ) -> Result<Fill, VenueError> {
        let (id, req, old_reserved, remaining_before) = {
            let r = &st.resting[idx];
            (r.id.clone(), r.req.clone(), r.reserved, r.remaining)
        };
        st.reserved = st
            .reserved
            .checked_sub(old_reserved)
            .map_err(VenueError::Money)?;
        let fill = self.apply_fill(st, &id, &req, req.limit_price, take, is_maker)?;
        let remaining_after = Contracts::new(remaining_before.raw() - take);
        if remaining_after.raw() > 0 {
            let new_reserved = if req.action == Action::Buy {
                let mut rest = req.clone();
                rest.qty = remaining_after;
                let category = st
                    .markets
                    .get(&req.market)
                    .map(|m| m.category.clone())
                    .unwrap_or_default();
                self.worst_case_cost(&rest, &category)?
            } else {
                Cents::ZERO
            };
            st.reserved = st
                .reserved
                .checked_add(new_reserved)
                .map_err(VenueError::Money)?;
            let rec = &mut st.resting[idx];
            rec.remaining = remaining_after;
            rec.reserved = new_reserved;
        } else {
            st.resting.remove(idx);
        }
        Ok(fill)
    }

    fn apply_fill(
        &self,
        st: &mut State,
        id: &VenueOrderId,
        req: &OrderReq,
        price: Cents,
        take: i64,
        is_maker: bool,
    ) -> Result<Fill, VenueError> {
        let category = st
            .markets
            .get(&req.market)
            .map(|m| m.category.clone())
            .unwrap_or_default();
        let role = if is_maker {
            FillRole::Maker
        } else {
            FillRole::Taker
        };
        let at = self.clock.now();
        let fee = self
            .fees
            .fee(role, price, Contracts::new(take), Some(&category), at)?;
        let gross = notional(price, Contracts::new(take)).map_err(VenueError::Money)?;
        match req.action {
            Action::Buy => {
                st.cash = st
                    .cash
                    .checked_sub(gross)
                    .and_then(|c| c.checked_sub(fee))
                    .map_err(VenueError::Money)?;
                let pos = st.positions.entry(req.market.clone()).or_default();
                match req.side {
                    Side::Yes => pos.yes += take,
                    Side::No => pos.no += take,
                }
                pos.cost = pos.cost.checked_add(gross).map_err(VenueError::Money)?;
            }
            Action::Sell => {
                st.cash = st
                    .cash
                    .checked_add(gross)
                    .and_then(|c| c.checked_sub(fee))
                    .map_err(VenueError::Money)?;
                let pos = st.positions.entry(req.market.clone()).or_default();
                match req.side {
                    Side::Yes => pos.yes -= take,
                    Side::No => pos.no -= take,
                }
                pos.cost = pos.cost.checked_sub(gross).map_err(VenueError::Money)?;
            }
        }
        if st
            .positions
            .get(&req.market)
            .is_some_and(|p| p.yes == 0 && p.no == 0)
        {
            st.positions.remove(&req.market);
        }
        let seq = st.fills.len() as u64;
        let fill = Fill {
            fill_id: format!("p-{seq}"),
            venue_order_id: id.clone(),
            client_order_id: req.client_order_id.clone(),
            market: req.market.clone(),
            side: req.side,
            action: req.action,
            price,
            qty: Contracts::new(take),
            fee,
            is_maker,
            at,
        };
        st.fills.push(fill.clone());
        Ok(fill)
    }
}

fn yes_space(side: Side, action: Action, price: Cents) -> Result<(bool, Cents), VenueError> {
    let mirrored = Cents::new(100)
        .checked_sub(price)
        .map_err(VenueError::Money)?;
    Ok(match (side, action) {
        (Side::Yes, Action::Buy) => (true, price),
        (Side::Yes, Action::Sell) => (false, price),
        (Side::No, Action::Buy) => (false, mirrored),
        (Side::No, Action::Sell) => (true, mirrored),
    })
}

fn best_counter_level(st: &State, req: &OrderReq) -> Result<Option<(Cents, i64)>, VenueError> {
    let Some((bids, asks)) = st.books.get(&req.market) else {
        return Ok(None);
    };
    let mirrored_limit = Cents::new(100)
        .checked_sub(req.limit_price)
        .map_err(VenueError::Money)?;
    let mirror = |l: &PriceLevel| -> Result<(Cents, i64), VenueError> {
        Ok((
            Cents::new(100)
                .checked_sub(l.price)
                .map_err(VenueError::Money)?,
            l.qty.raw(),
        ))
    };
    Ok(match (req.side, req.action) {
        (Side::Yes, Action::Buy) => asks
            .first()
            .filter(|l| l.price <= req.limit_price)
            .map(|l| (l.price, l.qty.raw())),
        (Side::Yes, Action::Sell) => bids
            .first()
            .filter(|l| l.price >= req.limit_price)
            .map(|l| (l.price, l.qty.raw())),
        (Side::No, Action::Buy) => bids
            .first()
            .filter(|l| l.price >= mirrored_limit)
            .map(mirror)
            .transpose()?,
        (Side::No, Action::Sell) => asks
            .first()
            .filter(|l| l.price <= mirrored_limit)
            .map(mirror)
            .transpose()?,
    })
}

fn consume_counter_level(st: &mut State, req: &OrderReq, take: i64) {
    let Some((bids, asks)) = st.books.get_mut(&req.market) else {
        return;
    };
    let levels = match (req.side, req.action) {
        (Side::Yes, Action::Buy) | (Side::No, Action::Sell) => asks,
        (Side::Yes, Action::Sell) | (Side::No, Action::Buy) => bids,
    };
    if let Some(first) = levels.first_mut() {
        let left = first.qty.raw() - take;
        if left > 0 {
            first.qty = Contracts::new(left);
        } else {
            levels.remove(0);
        }
    }
}

#[async_trait]
impl Venue for PaperVenue {
    fn id(&self) -> VenueId {
        self.venue_id.clone()
    }

    async fn markets(&self, filter: MarketFilter) -> Result<Vec<Market>, VenueError> {
        let st = self.lock();
        Ok(st
            .markets
            .values()
            .filter(|m| {
                filter.category.as_ref().is_none_or(|c| &m.category == c)
                    && filter.status.is_none_or(|s| m.status == s)
            })
            .cloned()
            .collect())
    }

    async fn book(&self, market: &MarketId) -> Result<OrderBook, VenueError> {
        let st = self.lock();
        let (bids, asks) = st.books.get(market).ok_or_else(|| VenueError::NotFound {
            what: format!("market {market}"),
        })?;
        Ok(OrderBook {
            market: market.clone(),
            as_of: self.clock.now(),
            yes_bids: bids.clone(),
            yes_asks: asks.clone(),
        })
    }

    async fn place(&self, order: GatedOrder) -> Result<VenueOrderId, VenueError> {
        let req = OrderReq {
            market: order.market().clone(),
            side: order.side(),
            action: order.action(),
            limit_price: order.limit_price(),
            qty: order.qty(),
            client_order_id: order.client_order_id().clone(),
        };
        let mut st = self.lock();
        if let Some(existing) = st.by_coid.get(req.client_order_id.as_str()) {
            return Err(VenueError::AlreadyExists {
                existing: existing.clone(),
            });
        }
        let (status, _category) = match st.markets.get(&req.market) {
            Some(m) => (m.status, m.category.clone()),
            None => {
                return Err(VenueError::NotFound {
                    what: format!("market {}", req.market),
                })
            }
        };
        if status != MarketStatus::Trading {
            return Err(VenueError::Rejected {
                reason: format!("market {} not trading ({status:?})", req.market),
            });
        }
        if !(1..=99).contains(&req.limit_price.raw()) {
            return Err(VenueError::Invalid {
                reason: format!("limit price {} outside [1, 99]", req.limit_price),
            });
        }
        if req.qty.raw() <= 0 {
            return Err(VenueError::Invalid {
                reason: format!("quantity {} must be positive", req.qty),
            });
        }
        match req.action {
            Action::Buy => {
                let category = st
                    .markets
                    .get(&req.market)
                    .map(|m| m.category.clone())
                    .unwrap_or_default();
                let worst = self.worst_case_cost(&req, &category)?;
                let available = st
                    .cash
                    .checked_sub(st.reserved)
                    .map_err(VenueError::Money)?;
                if worst > available {
                    return Err(VenueError::Rejected {
                        reason: format!("insufficient funds: need {worst}, available {available}"),
                    });
                }
            }
            Action::Sell => {
                let held = st
                    .positions
                    .get(&req.market)
                    .map(|p| match req.side {
                        Side::Yes => p.yes,
                        Side::No => p.no,
                    })
                    .unwrap_or(0);
                let working: i64 = st
                    .resting
                    .iter()
                    .filter(|r| {
                        r.req.market == req.market
                            && r.req.side == req.side
                            && r.req.action == Action::Sell
                    })
                    .map(|r| r.remaining.raw())
                    .sum();
                if req.qty.raw() + working > held {
                    return Err(VenueError::Rejected {
                        reason: format!(
                            "sell {} exceeds held {held} ({working} already working)",
                            req.qty
                        ),
                    });
                }
            }
        }
        let seq = st.next_seq;
        st.next_seq += 1;
        let id = VenueOrderId::new(format!("paper-{seq}")).map_err(|e| VenueError::Invalid {
            reason: e.to_string(),
        })?;
        st.by_coid
            .insert(req.client_order_id.as_str().to_string(), id.clone());
        self.execute_order(&mut st, id.clone(), req)?;
        Ok(id)
    }

    async fn cancel(&self, id: &VenueOrderId) -> Result<(), VenueError> {
        let mut st = self.lock();
        if let Some(i) = st.resting.iter().position(|r| &r.id == id) {
            let r = st.resting.remove(i);
            st.reserved = st
                .reserved
                .checked_sub(r.reserved)
                .map_err(VenueError::Money)?;
            Ok(())
        } else {
            Err(VenueError::NotFound {
                what: format!("order {id}"),
            })
        }
    }

    async fn positions(&self) -> Result<Vec<VenuePosition>, VenueError> {
        let st = self.lock();
        Ok(st
            .positions
            .iter()
            .map(|(market, pos)| VenuePosition {
                market: market.clone(),
                yes: pos.yes,
                no: pos.no,
                cost: pos.cost,
            })
            .collect())
    }

    async fn open_orders(&self) -> Result<Vec<OpenOrder>, VenueError> {
        let st = self.lock();
        Ok(st
            .resting
            .iter()
            .map(|r| OpenOrder {
                venue_order_id: r.id.clone(),
                client_order_id: r.req.client_order_id.clone(),
                market: r.req.market.clone(),
                side: r.req.side,
                action: r.req.action,
                limit_price: r.req.limit_price,
                remaining_qty: r.remaining,
            })
            .collect())
    }

    async fn balance(&self) -> Result<Cents, VenueError> {
        let st = self.lock();
        st.cash.checked_sub(st.reserved).map_err(VenueError::Money)
    }

    async fn fills_since(&self, cursor: Cursor) -> Result<FillPage, VenueError> {
        let st = self.lock();
        let start: usize = if cursor.0.is_empty() {
            0
        } else {
            cursor.0.parse().map_err(|_| VenueError::Invalid {
                reason: format!("bad cursor {:?}", cursor.0),
            })?
        };
        let fills = st.fills.get(start..).unwrap_or(&[]).to_vec();
        Ok(FillPage {
            fills,
            next_cursor: Cursor(st.fills.len().to_string()),
        })
    }

    fn fee_model(&self) -> &dyn FeeModel {
        &self.fees
    }
}
