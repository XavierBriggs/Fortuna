//! The sim venue: deterministic exchange with seeded fault injection.
//! Spec 5.1/5.2: "a sim venue adapter plus seeded fault injection... runs the
//! full core through thousands of randomized failure scenarios per CI run."
//!
//! Behavior model:
//! - One canonical YES book per market (exogenous liquidity, set via
//!   `set_book`); NO liquidity is the derived mirror (no_ask(p) =
//!   yes_bid(100-p), pair-mint matching like Kalshi).
//! - Our orders match against visible depth (best level first, level
//!   granularity); remainders rest and fill when injected public flow
//!   crosses them (maker).
//! - Buys reserve worst-case cost (limit x qty + worst-case fee) at accept
//!   time, like real venues; the exact reserved amount is stored and released
//!   on cancel/fill (never recomputed, so schedule changes cannot drift the
//!   ledger). Sells are close-only against held positions.
//! - Faults are seeded; rolls happen in a DOCUMENTED, FIXED order per call
//!   and consume randomness only when the corresponding rate is non-zero.
//!   Same seed + same fault config + same call sequence => identical behavior.

use crate::fees::ScheduleFeeModel;
use crate::{
    Cursor, Fill, FillPage, Market, MarketFilter, MarketStatus, SettlementNotice,
    SettlementOutcome, SettlementPage, VenueError, VenuePosition,
};
use async_trait::async_trait;
use fortuna_core::book::{FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_core::ids::SplitMix64;
use fortuna_core::market::{
    notional, Action, ClientOrderId, Contracts, MarketId, Side, VenueId, VenueOrderId,
};
use fortuna_core::money::Cents;
use fortuna_gates::GatedOrder;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Per-mille fault rates plus the seed. All-zero = faithful venue.
#[derive(Debug, Clone)]
pub struct FaultConfig {
    pub seed: u64,
    /// Whole call fails transiently with `Outage`; no side effect.
    pub api_error_pm: u32,
    /// `place` returns `Timeout` AFTER the order took effect.
    pub place_timeout_but_placed_pm: u32,
    /// `place` cleanly rejected; no side effect.
    pub place_reject_pm: u32,
    /// Order accepted but processes only at the next `tick`.
    pub ack_delay_pm: u32,
    /// A fill is withheld from one poll and delivered on a later one.
    pub drop_fill_pm: u32,
    /// A fill is delivered twice across polls.
    pub dup_fill_pm: u32,
    /// `cancel` returns `Timeout` but the cancel happened.
    pub cancel_timeout_cancelled_pm: u32,
    /// `cancel` returns `Timeout` and the order is still live.
    pub cancel_timeout_not_cancelled_pm: u32,
}

impl FaultConfig {
    /// No faults; the seed still drives any future randomness.
    pub fn none(seed: u64) -> Self {
        FaultConfig {
            seed,
            api_error_pm: 0,
            place_timeout_but_placed_pm: 0,
            place_reject_pm: 0,
            ack_delay_pm: 0,
            drop_fill_pm: 0,
            dup_fill_pm: 0,
            cancel_timeout_cancelled_pm: 0,
            cancel_timeout_not_cancelled_pm: 0,
        }
    }
}

/// Order parameters as the sim exchange consumes them. The production path
/// is `Venue::place(GatedOrder)`, which destructures into this; `place_raw`
/// is the intake for sim-crate tests and the DST harness until the gate
/// pipeline lands (T0.5), after which DST drives gated orders end to end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceOrder {
    pub market: MarketId,
    pub side: Side,
    pub action: Action,
    pub limit_price: Cents,
    pub qty: Contracts,
    pub client_order_id: ClientOrderId,
}

#[derive(Debug, Clone)]
struct RestingOrder {
    id: VenueOrderId,
    req: PlaceOrder,
    remaining: Contracts,
    /// Exact amount reserved for this order; released verbatim.
    reserved: Cents,
}

#[derive(Debug, Clone)]
struct PendingOrder {
    id: VenueOrderId,
    req: PlaceOrder,
    /// Exact amount reserved at accept time; released verbatim before
    /// processing or on cancel.
    reserved: Cents,
}

#[derive(Debug, Clone)]
struct FillRec {
    fill: Fill,
    withheld_done: bool,
    dup_done: bool,
}

#[derive(Debug, Clone, Default)]
struct Pos {
    yes: i64,
    no: i64,
    cost: Cents,
}

struct State {
    rng: SplitMix64,
    markets: BTreeMap<MarketId, Market>,
    /// (yes_bids descending, yes_asks ascending) exogenous liquidity.
    books: BTreeMap<MarketId, (Vec<PriceLevel>, Vec<PriceLevel>)>,
    resting: Vec<RestingOrder>,
    pending: Vec<PendingOrder>,
    fills: Vec<FillRec>,
    by_coid: BTreeMap<String, VenueOrderId>,
    positions: BTreeMap<MarketId, Pos>,
    cash: Cents,
    reserved: Cents,
    next_order_seq: u64,
    outage_until: Option<UtcTimestamp>,
    faults: FaultConfig,
    /// Authoritative settlement history (spec 5.13 notice stream).
    /// Corrections append NEW notices; nothing is edited.
    settlement_notices: Vec<SettlementNotice>,
    /// What each settlement paid (per market), so a venue-side reversal
    /// can claw back and re-pay exactly.
    settled_history: BTreeMap<MarketId, SettledRecord>,
}

#[derive(Debug, Clone)]
struct SettledRecord {
    yes: i64,
    no: i64,
    winner: Side,
    paid: Cents,
}

impl State {
    /// Consumes randomness ONLY when `pm > 0`, so disabled faults never
    /// perturb sequences. Roll-site order per call is part of the contract.
    fn roll(&mut self, pm: u32) -> bool {
        if pm == 0 {
            return false;
        }
        self.rng.next_u64() % 1000 < u64::from(pm)
    }

    fn reserve(&mut self, amount: Cents) -> Result<(), VenueError> {
        self.reserved = self
            .reserved
            .checked_add(amount)
            .map_err(VenueError::Money)?;
        Ok(())
    }

    fn release(&mut self, amount: Cents) -> Result<(), VenueError> {
        self.reserved = self
            .reserved
            .checked_sub(amount)
            .map_err(VenueError::Money)?;
        Ok(())
    }
}

/// The simulated venue. All methods take `&self`; state is internally locked.
pub struct SimVenue {
    venue_id: VenueId,
    clock: Arc<dyn Clock>,
    fees: ScheduleFeeModel,
    state: Mutex<State>,
}

impl SimVenue {
    pub fn new(
        venue_id: VenueId,
        clock: Arc<dyn Clock>,
        fees: ScheduleFeeModel,
        faults: FaultConfig,
        starting_cash: Cents,
    ) -> Self {
        SimVenue {
            venue_id,
            clock,
            fees,
            state: Mutex::new(State {
                rng: SplitMix64::new(faults.seed),
                markets: BTreeMap::new(),
                books: BTreeMap::new(),
                resting: Vec::new(),
                pending: Vec::new(),
                fills: Vec::new(),
                by_coid: BTreeMap::new(),
                positions: BTreeMap::new(),
                cash: starting_cash,
                reserved: Cents::ZERO,
                next_order_seq: 0,
                outage_until: None,
                settlement_notices: Vec::new(),
                settled_history: BTreeMap::new(),
                faults,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn check_outage(&self, st: &State) -> Result<(), VenueError> {
        if let Some(until) = st.outage_until {
            if self.clock.now() < until {
                return Err(VenueError::Outage {
                    venue: self.venue_id.to_string(),
                    reason: format!("outage window until {until}"),
                });
            }
        }
        Ok(())
    }

    fn transient(&self, st: &mut State) -> Result<(), VenueError> {
        let pm = st.faults.api_error_pm;
        if st.roll(pm) {
            return Err(VenueError::Outage {
                venue: self.venue_id.to_string(),
                reason: "transient API error (injected)".into(),
            });
        }
        Ok(())
    }

    // ---- test/DST control surface ----

    pub fn add_market(&self, market: Market) {
        let mut st = self.lock();
        st.books
            .entry(market.id.clone())
            .or_insert_with(|| (Vec::new(), Vec::new()));
        st.markets.insert(market.id.clone(), market);
    }

    pub fn set_book(
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

    pub fn set_outage_until(&self, until: UtcTimestamp) {
        self.lock().outage_until = Some(until);
    }

    /// Ground-truth totals for DST invariant checks:
    /// (total cash, reserved, recorded fill count, pending order count).
    pub fn inspect_totals(&self) -> (Cents, Cents, usize, usize) {
        let st = self.lock();
        (st.cash, st.reserved, st.fills.len(), st.pending.len())
    }

    /// Live resting orders (ours), in placement order, qty = remaining.
    pub fn resting_orders(&self) -> Vec<(VenueOrderId, PlaceOrder)> {
        self.lock()
            .resting
            .iter()
            .map(|r| {
                let mut req = r.req.clone();
                req.qty = r.remaining;
                (r.id.clone(), req)
            })
            .collect()
    }

    /// Process ack-delayed orders (FIFO). Deterministic: no fault rolls.
    pub fn tick(&self) -> Result<(), VenueError> {
        let mut st = self.lock();
        let pending = std::mem::take(&mut st.pending);
        for p in pending {
            st.release(p.reserved)?;
            self.execute_order(&mut st, p.id, p.req)?;
        }
        Ok(())
    }

    /// Public (non-FORTUNA) aggressor flow: fills OUR resting orders that it
    /// crosses, best price first, FIFO within a price. Its remainder
    /// evaporates (goes elsewhere). Pair-mint semantics: a public NO buy at p
    /// takes YES bids at >= 100-p, and vice versa.
    pub fn inject_public_order(
        &self,
        market: &MarketId,
        side: Side,
        action: Action,
        limit: Cents,
        qty: i64,
    ) -> Result<Vec<Fill>, VenueError> {
        let mut st = self.lock();
        let (yes_action, yes_limit) = match side {
            Side::Yes => (action, limit),
            Side::No => (
                match action {
                    Action::Buy => Action::Sell,
                    Action::Sell => Action::Buy,
                },
                Cents::new(100)
                    .checked_sub(limit)
                    .map_err(VenueError::Money)?,
            ),
        };
        let mut remaining = qty;
        let mut fills = Vec::new();
        while remaining > 0 {
            let mut best: Option<(usize, Cents)> = None;
            for (i, r) in st.resting.iter().enumerate() {
                if &r.req.market != market {
                    continue;
                }
                let (is_yes_bid, yes_price) = resting_yes_quote(&r.req)?;
                let crossed = match yes_action {
                    Action::Sell => is_yes_bid && yes_price >= yes_limit,
                    Action::Buy => !is_yes_bid && yes_price <= yes_limit,
                };
                if !crossed {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some((_, bp)) => match yes_action {
                        Action::Sell => yes_price > bp,
                        Action::Buy => yes_price < bp,
                    },
                };
                if better {
                    best = Some((i, yes_price));
                }
            }
            let Some((idx, _)) = best else { break };
            let take = remaining.min(st.resting[idx].remaining.raw());
            let fill = self.fill_resting(&mut st, idx, take)?;
            fills.push(fill);
            remaining -= take;
        }
        Ok(fills)
    }

    /// Settle a market: winners pay out at full contract value, positions
    /// clear, resting orders on it cancel (reservations released), and the
    /// market leaves the tradable set.
    pub fn settle_market(&self, market: &MarketId, winner: Side) -> Result<Cents, VenueError> {
        let mut st = self.lock();
        let payout_per = {
            let m = st.markets.get(market).ok_or_else(|| VenueError::NotFound {
                what: format!("market {market}"),
            })?;
            if matches!(m.status, MarketStatus::Settled | MarketStatus::Voided) {
                return Err(VenueError::Rejected {
                    reason: format!("market {market} already terminal ({:?})", m.status),
                });
            }
            m.payout_per_contract
        };

        // Compute payout before mutating anything.
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

        // Cancel resting orders on this market, releasing exact reservations.
        let resting = std::mem::take(&mut st.resting);
        let mut kept = Vec::with_capacity(resting.len());
        for r in resting {
            if &r.req.market == market {
                st.release(r.reserved)?;
            } else {
                kept.push(r);
            }
        }
        st.resting = kept;
        // And any pending (ack-delayed) orders on it.
        let pending = std::mem::take(&mut st.pending);
        let mut kept_pending = Vec::with_capacity(pending.len());
        for p in pending {
            if &p.req.market == market {
                st.release(p.reserved)?;
            } else {
                kept_pending.push(p);
            }
        }
        st.pending = kept_pending;

        let held = st.positions.remove(market).unwrap_or(Pos {
            yes: 0,
            no: 0,
            cost: Cents::ZERO,
        });
        st.cash = new_cash;
        if let Some(m) = st.markets.get_mut(market) {
            m.status = MarketStatus::Settled;
        }
        st.books.remove(market);
        st.settled_history.insert(
            market.clone(),
            SettledRecord {
                yes: held.yes,
                no: held.no,
                winner,
                paid: payout,
            },
        );
        let seq = st.settlement_notices.len();
        let notice = SettlementNotice {
            notice_id: format!("stl-{market}-{seq}"),
            market: market.clone(),
            outcome: SettlementOutcome::Winner(winner),
            at: self.clock.now(),
            detail: serde_json::json!({ "paid_cents": payout.raw() }),
        };
        st.settlement_notices.push(notice);
        Ok(payout)
    }

    /// Void path (spec 5.13 terminal alternative): refund the position's
    /// exact cost basis, cancel the market's orders, emit a Voided notice.
    pub fn void_market(&self, market: &MarketId) -> Result<Cents, VenueError> {
        let mut st = self.lock();
        {
            let m = st.markets.get(market).ok_or_else(|| VenueError::NotFound {
                what: format!("market {market}"),
            })?;
            if matches!(m.status, MarketStatus::Settled | MarketStatus::Voided) {
                return Err(VenueError::Rejected {
                    reason: format!("market {market} already terminal ({:?})", m.status),
                });
            }
        }
        let refund = st
            .positions
            .get(market)
            .map(|p| p.cost)
            .unwrap_or(Cents::ZERO);
        let new_cash = st.cash.checked_add(refund).map_err(VenueError::Money)?;
        let resting = std::mem::take(&mut st.resting);
        let mut kept = Vec::with_capacity(resting.len());
        for r in resting {
            if &r.req.market == market {
                st.release(r.reserved)?;
            } else {
                kept.push(r);
            }
        }
        st.resting = kept;
        let pending = std::mem::take(&mut st.pending);
        let mut kept_pending = Vec::with_capacity(pending.len());
        for p in pending {
            if &p.req.market == market {
                st.release(p.reserved)?;
            } else {
                kept_pending.push(p);
            }
        }
        st.pending = kept_pending;
        st.positions.remove(market);
        st.cash = new_cash;
        if let Some(m) = st.markets.get_mut(market) {
            m.status = MarketStatus::Voided;
        }
        st.books.remove(market);
        let seq = st.settlement_notices.len();
        let notice = SettlementNotice {
            notice_id: format!("stl-{market}-{seq}"),
            market: market.clone(),
            outcome: SettlementOutcome::Voided,
            at: self.clock.now(),
            detail: serde_json::json!({ "refund_cents": refund.raw() }),
        };
        st.settlement_notices.push(notice);
        Ok(refund)
    }

    /// Venue correction (spec 5.13: determined may be reversed and
    /// re-determined; reversals are new entries). Claws back what the
    /// original settlement paid, pays the corrected winner from the
    /// recorded held lots, and emits a NEW notice for the correction.
    pub fn reverse_settlement(
        &self,
        market: &MarketId,
        corrected_winner: Side,
    ) -> Result<(), VenueError> {
        let mut st = self.lock();
        let record =
            st.settled_history
                .get(market)
                .cloned()
                .ok_or_else(|| VenueError::Rejected {
                    reason: format!("market {market} has no settlement to reverse"),
                })?;
        if record.winner == corrected_winner {
            return Err(VenueError::Rejected {
                reason: format!("correction matches the original winner {corrected_winner:?}"),
            });
        }
        let payout_per = st
            .markets
            .get(market)
            .map(|m| m.payout_per_contract)
            .ok_or_else(|| VenueError::NotFound {
                what: format!("market {market}"),
            })?;
        let winning = match corrected_winner {
            Side::Yes => record.yes.max(0),
            Side::No => record.no.max(0),
        };
        let repay = payout_per.checked_mul(winning).map_err(VenueError::Money)?;
        let new_cash = st
            .cash
            .checked_sub(record.paid)
            .and_then(|c| c.checked_add(repay))
            .map_err(VenueError::Money)?;
        st.cash = new_cash;
        st.settled_history.insert(
            market.clone(),
            SettledRecord {
                winner: corrected_winner,
                paid: repay,
                ..record
            },
        );
        let seq = st.settlement_notices.len();
        let notice = SettlementNotice {
            notice_id: format!("stl-{market}-{seq}"),
            market: market.clone(),
            outcome: SettlementOutcome::Winner(corrected_winner),
            at: self.clock.now(),
            detail: serde_json::json!({
                "correction": true,
                "clawed_cents": record.paid.raw(),
                "repaid_cents": repay.raw(),
            }),
        };
        st.settlement_notices.push(notice);
        Ok(())
    }

    /// What the venue's settlement record says it paid for a settled
    /// market (post-reversal: the corrected repay). DST I-money input.
    pub fn settled_paid(&self, market: &MarketId) -> Option<Cents> {
        self.lock().settled_history.get(market).map(|r| r.paid)
    }

    /// Test/DST hook: force a market's lifecycle status (e.g. Disputed
    /// for the 5.13 dispute-watchdog scenarios).
    pub fn set_market_status(&self, market: &MarketId, status: MarketStatus) {
        let mut st = self.lock();
        if let Some(m) = st.markets.get_mut(market) {
            m.status = status;
        }
    }

    /// Test/DST hook: seed a held position directly (qty + cost basis)
    /// without driving the order path.
    pub fn seed_position(&self, market: &MarketId, yes: i64, no: i64, cost: Cents) {
        let mut st = self.lock();
        st.positions.insert(market.clone(), Pos { yes, no, cost });
    }

    // ---- order intake ----

    /// Sim/DST intake; `Venue::place(GatedOrder)` destructures into this.
    ///
    /// Roll order (the determinism contract): idempotency short-circuits
    /// before any roll; then api_error, reject, ack_delay, timeout-but-placed.
    pub fn place_raw(&self, req: PlaceOrder) -> Result<VenueOrderId, VenueError> {
        let mut st = self.lock();
        self.check_outage(&st)?;

        // Idempotency FIRST (no rolls): a known client order id is refused
        // with the original order's id, mirroring Kalshi's
        // ORDER_ALREADY_EXISTS (docs/research/venue/kalshi-fees-2026-06-09).
        // Crash resubmission treats this as success (spec 5.4).
        if let Some(existing) = st.by_coid.get(req.client_order_id.as_str()) {
            return Err(VenueError::AlreadyExists {
                existing: existing.clone(),
            });
        }

        self.transient(&mut st)?;

        let (status, category) = match st.markets.get(&req.market) {
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
                reason: format!("limit price {} outside [1, 99] cents", req.limit_price),
            });
        }
        if req.qty.raw() <= 0 {
            return Err(VenueError::Invalid {
                reason: format!("quantity {} must be positive", req.qty),
            });
        }

        let reject_pm = st.faults.place_reject_pm;
        if st.roll(reject_pm) {
            return Err(VenueError::Rejected {
                reason: "rejected (injected fault)".into(),
            });
        }

        let worst = self.worst_case_cost(&req, &category)?;
        match req.action {
            Action::Buy => {
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
                let held_side = st
                    .positions
                    .get(&req.market)
                    .map(|p| match req.side {
                        Side::Yes => p.yes,
                        Side::No => p.no,
                    })
                    .unwrap_or(0);
                let already_selling: i64 = st
                    .resting
                    .iter()
                    .map(|r| (&r.req, r.remaining.raw()))
                    .chain(st.pending.iter().map(|p| (&p.req, p.req.qty.raw())))
                    .filter(|(r, _)| {
                        r.market == req.market && r.side == req.side && r.action == Action::Sell
                    })
                    .map(|(_, q)| q)
                    .sum();
                if req.qty.raw() + already_selling > held_side {
                    return Err(VenueError::Rejected {
                        reason: format!(
                            "sell {} exceeds held {held_side} ({already_selling} already working)",
                            req.qty
                        ),
                    });
                }
            }
        }

        let seq = st.next_order_seq;
        st.next_order_seq += 1;
        let id = VenueOrderId::new(format!("sim-{seq}")).map_err(|e| VenueError::Invalid {
            reason: e.to_string(),
        })?;
        st.by_coid
            .insert(req.client_order_id.as_str().to_string(), id.clone());

        let delay_pm = st.faults.ack_delay_pm;
        if st.roll(delay_pm) {
            let reserved = if req.action == Action::Buy {
                worst
            } else {
                Cents::ZERO
            };
            st.reserve(reserved)?;
            st.pending.push(PendingOrder {
                id: id.clone(),
                req,
                reserved,
            });
        } else {
            self.execute_order(&mut st, id.clone(), req)?;
        }

        let timeout_pm = st.faults.place_timeout_but_placed_pm;
        if st.roll(timeout_pm) {
            // The order took effect above; the caller just never hears it.
            return Err(VenueError::Timeout {
                operation: "place".into(),
            });
        }
        Ok(id)
    }

    /// Worst-case cash for a buy: limit x qty plus the larger of the
    /// maker/taker fee at the limit price (with the market's category).
    fn worst_case_cost(&self, req: &PlaceOrder, category: &str) -> Result<Cents, VenueError> {
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

    /// Match an accepted order against visible depth; rest the remainder
    /// (with an exact stored reservation for buys). Deterministic: no rolls.
    fn execute_order(
        &self,
        st: &mut State,
        id: VenueOrderId,
        req: PlaceOrder,
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
            st.reserve(reserved)?;
            st.resting.push(RestingOrder {
                id,
                req,
                remaining,
                reserved,
            });
        }
        Ok(())
    }

    /// Fill `take` contracts of the resting order at `idx` at its own price
    /// (maker). Releases/re-reserves the exact stored amounts.
    fn fill_resting(&self, st: &mut State, idx: usize, take: i64) -> Result<Fill, VenueError> {
        let (id, req, old_reserved, remaining_before) = {
            let r = &st.resting[idx];
            (r.id.clone(), r.req.clone(), r.reserved, r.remaining)
        };
        st.release(old_reserved)?;
        let fill = self.apply_fill(st, &id, &req, req.limit_price, take, true)?;
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
            st.reserve(new_reserved)?;
            let rec = &mut st.resting[idx];
            rec.remaining = remaining_after;
            rec.reserved = new_reserved;
        } else {
            st.resting.remove(idx);
        }
        Ok(fill)
    }

    /// Book a fill: cash, positions, fee, fill record.
    fn apply_fill(
        &self,
        st: &mut State,
        id: &VenueOrderId,
        req: &PlaceOrder,
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
            fill_id: format!("f-{seq}"),
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
        st.fills.push(FillRec {
            fill: fill.clone(),
            withheld_done: false,
            dup_done: false,
        });
        Ok(fill)
    }
}

/// What YES-space quote a resting order provides: (is_yes_bid, yes_price).
fn resting_yes_quote(req: &PlaceOrder) -> Result<(bool, Cents), VenueError> {
    let mirrored = Cents::new(100)
        .checked_sub(req.limit_price)
        .map_err(VenueError::Money)?;
    Ok(match (req.side, req.action) {
        (Side::Yes, Action::Buy) => (true, req.limit_price),
        (Side::Yes, Action::Sell) => (false, req.limit_price),
        (Side::No, Action::Buy) => (false, mirrored),
        (Side::No, Action::Sell) => (true, mirrored),
    })
}

/// Best exogenous-book level our aggressing order can take. Returns
/// (fill_price in OUR side's space, available level qty).
fn best_counter_level(st: &State, req: &PlaceOrder) -> Result<Option<(Cents, i64)>, VenueError> {
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
        // Buy YES: lift yes asks <= limit.
        (Side::Yes, Action::Buy) => asks
            .first()
            .filter(|l| l.price <= req.limit_price)
            .map(|l| (l.price, l.qty.raw())),
        // Sell YES: hit yes bids >= limit.
        (Side::Yes, Action::Sell) => bids
            .first()
            .filter(|l| l.price >= req.limit_price)
            .map(|l| (l.price, l.qty.raw())),
        // Buy NO: pair-mints against yes bids >= 100-limit; fill at 100-bid.
        (Side::No, Action::Buy) => bids
            .first()
            .filter(|l| l.price >= mirrored_limit)
            .map(mirror)
            .transpose()?,
        // Sell NO: unwinds against yes asks <= 100-limit; fill at 100-ask.
        (Side::No, Action::Sell) => asks
            .first()
            .filter(|l| l.price <= mirrored_limit)
            .map(mirror)
            .transpose()?,
    })
}

/// Remove `take` contracts from the level our aggressing order just took.
fn consume_counter_level(st: &mut State, req: &PlaceOrder, take: i64) {
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
impl crate::Venue for SimVenue {
    fn id(&self) -> VenueId {
        self.venue_id.clone()
    }

    async fn markets(&self, filter: MarketFilter) -> Result<Vec<Market>, VenueError> {
        let mut st = self.lock();
        self.check_outage(&st)?;
        self.transient(&mut st)?;
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
        let mut st = self.lock();
        self.check_outage(&st)?;
        self.transient(&mut st)?;
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
        self.place_raw(PlaceOrder {
            market: order.market().clone(),
            side: order.side(),
            action: order.action(),
            limit_price: order.limit_price(),
            qty: order.qty(),
            client_order_id: order.client_order_id().clone(),
        })
    }

    /// Roll order: api_error, timeout-not-cancelled, timeout-cancelled.
    async fn cancel(&self, id: &VenueOrderId) -> Result<(), VenueError> {
        let mut st = self.lock();
        self.check_outage(&st)?;
        self.transient(&mut st)?;

        let timeout_not_pm = st.faults.cancel_timeout_not_cancelled_pm;
        if st.roll(timeout_not_pm) {
            return Err(VenueError::Timeout {
                operation: "cancel".into(),
            });
        }

        let cancelled = {
            if let Some(i) = st.pending.iter().position(|p| &p.id == id) {
                let p = st.pending.remove(i);
                st.release(p.reserved)?;
                true
            } else if let Some(i) = st.resting.iter().position(|r| &r.id == id) {
                let r = st.resting.remove(i);
                st.release(r.reserved)?;
                true
            } else {
                false
            }
        };

        let timeout_cancelled_pm = st.faults.cancel_timeout_cancelled_pm;
        if st.roll(timeout_cancelled_pm) {
            return Err(VenueError::Timeout {
                operation: "cancel".into(),
            });
        }
        if cancelled {
            Ok(())
        } else {
            Err(VenueError::NotFound {
                what: format!("order {id}"),
            })
        }
    }

    async fn open_orders(&self) -> Result<Vec<crate::OpenOrder>, VenueError> {
        let mut st = self.lock();
        self.check_outage(&st)?;
        self.transient(&mut st)?;
        let mut out: Vec<crate::OpenOrder> = st
            .resting
            .iter()
            .map(|r| crate::OpenOrder {
                venue_order_id: r.id.clone(),
                client_order_id: r.req.client_order_id.clone(),
                market: r.req.market.clone(),
                side: r.req.side,
                action: r.req.action,
                limit_price: r.req.limit_price,
                remaining_qty: r.remaining,
            })
            .collect();
        out.extend(st.pending.iter().map(|p| crate::OpenOrder {
            venue_order_id: p.id.clone(),
            client_order_id: p.req.client_order_id.clone(),
            market: p.req.market.clone(),
            side: p.req.side,
            action: p.req.action,
            limit_price: p.req.limit_price,
            remaining_qty: p.req.qty,
        }));
        Ok(out)
    }

    async fn positions(&self) -> Result<Vec<VenuePosition>, VenueError> {
        let mut st = self.lock();
        self.check_outage(&st)?;
        self.transient(&mut st)?;
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

    async fn balance(&self) -> Result<Cents, VenueError> {
        let mut st = self.lock();
        self.check_outage(&st)?;
        self.transient(&mut st)?;
        st.cash.checked_sub(st.reserved).map_err(VenueError::Money)
    }

    async fn account(&self) -> Result<(Cents, Cents), VenueError> {
        // Surface the EXACT (cash, reserved) the sim tracks by delegating to the
        // prior `inspect_totals` read — a pure lock-and-read, NO outage/transient
        // fault roll — so the runner's drawdown + dashboard numbers are
        // byte-identical to the pre-generalization `inspect_totals` calls (A3).
        let (cash, reserved, _, _) = self.inspect_totals();
        Ok((cash, reserved))
    }

    async fn fills_since(&self, cursor: Cursor) -> Result<FillPage, VenueError> {
        let mut st = self.lock();
        self.check_outage(&st)?;
        self.transient(&mut st)?;

        let start: usize = if cursor.0.is_empty() {
            0
        } else {
            cursor.0.parse().map_err(|_| VenueError::Invalid {
                reason: format!("bad cursor {:?}", cursor.0),
            })?
        };

        let mut fills = Vec::new();
        let drop_pm = st.faults.drop_fill_pm;
        let dup_pm = st.faults.dup_fill_pm;
        let total = st.fills.len();
        let mut i = start;
        let mut next = start;
        while i < total {
            let withheld_done = st.fills[i].withheld_done;
            if !withheld_done && st.roll(drop_pm) {
                // Withhold this fill for one poll; the cursor stays behind it
                // so it is delivered (late) on the next poll. Order preserved.
                st.fills[i].withheld_done = true;
                break;
            }
            fills.push(st.fills[i].fill.clone());
            let dup_done = st.fills[i].dup_done;
            if !dup_done && st.roll(dup_pm) {
                // Leave the cursor ON this fill: the next poll re-delivers it
                // (at-least-once duplication), then advances.
                st.fills[i].dup_done = true;
                next = i;
                break;
            }
            i += 1;
            next = i;
        }
        Ok(FillPage {
            fills,
            next_cursor: Cursor(next.to_string()),
        })
    }

    async fn settlements_since(&self, cursor: Cursor) -> Result<SettlementPage, VenueError> {
        let mut st = self.lock();
        self.check_outage(&st)?;
        self.transient(&mut st)?;
        let start: usize = if cursor.0.is_empty() {
            0
        } else {
            cursor.0.parse().map_err(|_| VenueError::Invalid {
                reason: format!("bad settlement cursor {:?}", cursor.0),
            })?
        };
        let notices: Vec<SettlementNotice> =
            st.settlement_notices.iter().skip(start).cloned().collect();
        let next = start + notices.len();
        Ok(SettlementPage {
            notices,
            next_cursor: Cursor(next.to_string()),
        })
    }

    fn fee_model(&self) -> &dyn FeeModel {
        &self.fees
    }
}
