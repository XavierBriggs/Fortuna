//! The universal gate pipeline: ordered, fail-closed checks 1-11 (spec 5.3).
//!
//! Every order, regardless of origin, passes here (I1). Each evaluated check
//! emits an audit record with verdict and reason; the first rejection stops
//! evaluation. Any internal error in any check rejects the order
//! (fail-closed). The only constructor of `GatedOrder` lives at the end of
//! this pipeline.
//!
//! Side spaces: candidates carry prices and fair values in their OWN side's
//! space (a NO order quotes NO cents). Price sanity (check 4) and internal
//! netting (check 10) convert to YES space; notional, fees, and edge math
//! (checks 2, 3, 5, 6, 9) stay in candidate space, which is what the venue
//! charges.
//!
//! Check 11 (book-age freshness): opt-in via `GateConfig.max_book_age_ms`.
//! `None` (default) means the check is a no-op (always passes); `Some(ms)`
//! rejects orders backed by a book older than `ms` milliseconds. Absent a
//! book (`GateInputs.book = None`) the check passes — book absence is already
//! handled by check 4 (PriceSanity, fail-closed on no reference).

use crate::config::{GateConfig, GateError, RateLimits, StrategyLimits};
use crate::halt::{HaltFlags, HaltScope};
use crate::order::GatedOrder;
use crate::rate::Bucket;
use fortuna_core::book::{FeeModel, FillRole, OrderBook};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{EventId, IntentId};
use fortuna_core::market::{
    notional, Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId,
};
use fortuna_core::money::Cents;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// A pre-gate order: what strategies/sizing produce. `fair_value` is the
/// deterministic calibrated value of one contract in the order's own side
/// space; the gate recomputes net edge from it (check 6).
#[derive(Debug, Clone)]
pub struct CandidateOrder {
    pub intent_id: IntentId,
    pub strategy: StrategyId,
    pub venue: VenueId,
    pub market: MarketId,
    pub side: Side,
    pub action: Action,
    pub limit_price: Cents,
    pub qty: Contracts,
    pub fair_value: Cents,
    pub client_order_id: ClientOrderId,
}

/// The pipeline checks: spec 5.3 checks 1-11, plus the spec 5.15 perps
/// additions (12-15). `ALL` is the event-contract order; `perp::PERP_ALL`
/// is the perp-arm order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum GateCheck {
    Halts,
    Capital,
    PositionCaps,
    PriceSanity,
    SizeSanity,
    EdgeFloor,
    RateLimits,
    Idempotency,
    EventExposure,
    InternalNetting,
    /// Check 11 (P2 safety): rejects orders priced on a stale order book.
    /// Opt-in via `GateConfig.max_book_age_ms`; `None` = disabled (no-op).
    BookAge,
    /// Spec 5.15: worst-case (liquidation-loss) margin requirement vs the
    /// margin account's conservative equity.
    MarginHeadroom,
    /// Spec 5.15 gate (a): minimum distance between the conservative mark
    /// and the estimated liquidation point.
    LiquidationDistance,
    /// Spec 5.15 gate (b): per-asset leverage cap.
    LeverageCap,
    /// Spec 5.15 gate (d): per-venue and per-asset perps notional caps.
    PerpNotionalCap,
}

impl GateCheck {
    pub const ALL: [GateCheck; 11] = [
        GateCheck::Halts,
        GateCheck::Capital,
        GateCheck::PositionCaps,
        GateCheck::PriceSanity,
        GateCheck::SizeSanity,
        GateCheck::EdgeFloor,
        GateCheck::RateLimits,
        GateCheck::Idempotency,
        GateCheck::EventExposure,
        GateCheck::InternalNetting,
        GateCheck::BookAge,
    ];

    /// 1-based pipeline position (spec numbering; 12-15 are the spec 5.15
    /// perps additions).
    pub fn index(self) -> usize {
        match self {
            GateCheck::Halts => 1,
            GateCheck::Capital => 2,
            GateCheck::PositionCaps => 3,
            GateCheck::PriceSanity => 4,
            GateCheck::SizeSanity => 5,
            GateCheck::EdgeFloor => 6,
            GateCheck::RateLimits => 7,
            GateCheck::Idempotency => 8,
            GateCheck::EventExposure => 9,
            GateCheck::InternalNetting => 10,
            GateCheck::BookAge => 11,
            GateCheck::MarginHeadroom => 12,
            GateCheck::LiquidationDistance => 13,
            GateCheck::LeverageCap => 14,
            GateCheck::PerpNotionalCap => 15,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Verdict {
    Pass,
    Reject,
}

/// One audit record per evaluated check (I5 feeds from these).
#[derive(Debug, Clone, Serialize)]
pub struct GateCheckRecord {
    pub check: GateCheck,
    pub verdict: Verdict,
    pub reason: String,
    pub at: UtcTimestamp,
    pub intent_id: IntentId,
    pub client_order_id: String,
}

#[derive(Debug, Clone)]
pub struct GateRejection {
    pub check: GateCheck,
    pub reason: String,
}

/// Evaluation result: the sealed order (or rejection) plus the audit trail.
#[derive(Debug)]
pub struct GateOutcome {
    pub gated: Result<GatedOrder, GateRejection>,
    pub records: Vec<GateCheckRecord>,
}

/// Our own resting order, as the netting check needs to see it.
#[derive(Debug, Clone)]
pub struct RestingOrderView {
    pub market: MarketId,
    pub side: Side,
    pub action: Action,
    pub price: Cents,
}

/// Deterministic state views the checks consume. Exposures are worst-case
/// (spec 5.13/5.14): pending/disputed positions remain included by the
/// caller building these numbers.
pub struct GateInputs<'a> {
    pub now: UtcTimestamp,
    pub open_exposure_cents: Cents,
    pub market_exposure_cents: Cents,
    pub strategy_exposure_cents: Cents,
    pub event_exposure_cents: Cents,
    pub event_id: Option<EventId>,
    pub book: Option<&'a OrderBook>,
    pub last_trade_price: Option<Cents>,
    pub fee_model: &'a dyn FeeModel,
    pub category: Option<&'a str>,
    pub own_resting: &'a [RestingOrderView],
    pub recent_client_order_ids: &'a BTreeSet<String>,
}

/// The pipeline. Owns halt flags and rate-bucket state; everything else is
/// pure functions over (candidate, inputs, config).
pub struct GatePipeline {
    // pub(crate): the perp arm (crate::perp) shares this exact state — same
    // config, same halt flags, same I3 buckets — so a breach on either arm
    // halts both.
    pub(crate) config: GateConfig,
    pub(crate) halts: HaltFlags,
    pub(crate) venue_buckets: BTreeMap<String, Bucket>,
    pub(crate) market_buckets: BTreeMap<(String, String), Bucket>,
}

impl GatePipeline {
    pub fn new(config: GateConfig) -> Result<Self, GateError> {
        config.validate()?;
        Ok(GatePipeline {
            config,
            halts: HaltFlags::default(),
            venue_buckets: BTreeMap::new(),
            market_buckets: BTreeMap::new(),
        })
    }

    pub fn halts(&self) -> &HaltFlags {
        &self.halts
    }

    /// System halt entry point (drawdown monitor, runaway detection, ops).
    pub fn set_halt(&mut self, scope: HaltScope, reason: impl Into<String>) {
        self.halts.set(scope, reason);
    }

    /// Operator re-arm (I2): CLI-only path; see HaltFlags::rearm.
    pub fn rearm(&mut self, scope: HaltScope) -> Result<(), GateError> {
        self.halts.rearm(scope)
    }

    /// Operator hot-reload (I1). Halts are preserved unconditionally (a
    /// config push must never un-halt anything); rate buckets re-initialize
    /// under the new limits.
    pub fn reload_config(&mut self, config: GateConfig) -> Result<(), GateError> {
        config.validate()?;
        self.config = config;
        self.venue_buckets.clear();
        self.market_buckets.clear();
        Ok(())
    }

    /// Run checks 1-11 in order, fail-closed, emitting one record per
    /// evaluated check.
    pub fn evaluate(&mut self, candidate: &CandidateOrder, inputs: &GateInputs) -> GateOutcome {
        let mut records = Vec::with_capacity(GateCheck::ALL.len());
        for check in GateCheck::ALL {
            let result = self.run_check(check, candidate, inputs);
            let (verdict, reason, rejection) = match result {
                Ok(note) => (Verdict::Pass, note, None),
                Err(reason) => (
                    Verdict::Reject,
                    reason.clone(),
                    Some(GateRejection { check, reason }),
                ),
            };
            records.push(GateCheckRecord {
                check,
                verdict,
                reason,
                at: inputs.now,
                intent_id: candidate.intent_id,
                client_order_id: candidate.client_order_id.as_str().to_string(),
            });
            if let Some(rejection) = rejection {
                return GateOutcome {
                    gated: Err(rejection),
                    records,
                };
            }
        }
        GateOutcome {
            gated: Ok(GatedOrder::assemble(candidate)),
            records,
        }
    }

    fn run_check(
        &mut self,
        check: GateCheck,
        c: &CandidateOrder,
        i: &GateInputs,
    ) -> Result<String, String> {
        match check {
            GateCheck::Halts => self.check_halts(c),
            GateCheck::Capital => self.check_capital(c, i),
            GateCheck::PositionCaps => self.check_position_caps(c, i),
            GateCheck::PriceSanity => self.check_price_sanity(c, i),
            GateCheck::SizeSanity => self.check_size_sanity(c),
            GateCheck::EdgeFloor => self.check_edge_floor(c, i),
            GateCheck::RateLimits => self.check_rate_limits(c, i),
            GateCheck::Idempotency => self.check_idempotency(c, i),
            GateCheck::EventExposure => self.check_event_exposure(c, i),
            GateCheck::InternalNetting => self.check_internal_netting(c, i),
            GateCheck::BookAge => self.check_book_age(c, i),
            // Perp-domain checks (spec 5.15) never run on the event-contract
            // arm; `ALL` does not contain them. Defensive fail-closed arm.
            GateCheck::MarginHeadroom
            | GateCheck::LiquidationDistance
            | GateCheck::LeverageCap
            | GateCheck::PerpNotionalCap => {
                Err("perp-domain check invoked on the event-contract pipeline: fail-closed".into())
            }
        }
    }

    // ---- check 1 ----
    fn check_halts(&self, c: &CandidateOrder) -> Result<String, String> {
        match self.halts.blocking(c.strategy.as_str(), c.venue.as_str()) {
            Some((scope, reason)) => Err(format!("halted ({scope}): {reason}")),
            None => Ok("no halt flags set".into()),
        }
    }

    // ---- check 2 ----
    fn check_capital(&self, c: &CandidateOrder, i: &GateInputs) -> Result<String, String> {
        let cost = worst_case_cost(c, i)?;
        let cap = Cents::new(self.config.global.max_total_exposure_cents);
        let total = i
            .open_exposure_cents
            .checked_add(cost)
            .map_err(|e| format!("exposure arithmetic failed: {e}"))?;
        if total > cap {
            return Err(format!(
                "order cost {cost} + open exposure {} = {total} exceeds account cap {cap}",
                i.open_exposure_cents
            ));
        }
        Ok(format!(
            "cost {cost} + exposure {} within {cap}",
            i.open_exposure_cents
        ))
    }

    // ---- check 3 ----
    fn check_position_caps(&self, c: &CandidateOrder, i: &GateInputs) -> Result<String, String> {
        let limits = self.strategy_limits(c)?;
        let cost = worst_case_cost(c, i)?;

        let market_cap = Cents::new(self.config.global.per_market_exposure_cents);
        let market_total = i
            .market_exposure_cents
            .checked_add(cost)
            .map_err(|e| format!("market exposure arithmetic failed: {e}"))?;
        if market_total > market_cap {
            return Err(format!(
                "market exposure {market_total} would exceed per-market cap {market_cap}"
            ));
        }

        let strategy_cap = Cents::new(limits.max_exposure_cents);
        let strategy_total = i
            .strategy_exposure_cents
            .checked_add(cost)
            .map_err(|e| format!("strategy exposure arithmetic failed: {e}"))?;
        if strategy_total > strategy_cap {
            return Err(format!(
                "strategy exposure {strategy_total} would exceed cap {strategy_cap}"
            ));
        }
        Ok(format!(
            "market {market_total} <= {market_cap}, strategy {strategy_total} <= {strategy_cap}"
        ))
    }

    // ---- check 4 ----
    fn check_price_sanity(&self, c: &CandidateOrder, i: &GateInputs) -> Result<String, String> {
        let (is_bid, yes_limit) = yes_space(c)?;
        if !(1..=99).contains(&yes_limit.raw()) {
            return Err(format!(
                "limit {} (yes-space {yes_limit}) outside [1, 99] cents",
                c.limit_price
            ));
        }
        // Fail-closed: certifying sanity against the wrong market's book
        // would be worse than having no book at all.
        if let Some(b) = i.book {
            if b.market != c.market {
                return Err(format!(
                    "book is for {} but the order is for {}: refusing to certify",
                    b.market, c.market
                ));
            }
        }
        // Reference price: book mid (or single-sided touch), else last trade.
        let reference = match i.book {
            Some(b) => match (b.best_bid(), b.best_ask()) {
                (Some(bid), Some(ask)) => Some(Cents::new((bid.price.raw() + ask.price.raw()) / 2)),
                (Some(bid), None) => Some(bid.price),
                (None, Some(ask)) => Some(ask.price),
                (None, None) => i.last_trade_price,
            },
            None => i.last_trade_price,
        };
        let Some(reference) = reference else {
            return Err(
                "no price reference (no book, no last trade): cannot certify sanity".into(),
            );
        };
        let band = self.config.global.price_band_cents;
        let distance = (yes_limit.raw() - reference.raw()).abs();
        if distance > band {
            return Err(format!(
                "limit (yes-space {yes_limit}) is {distance}c from reference {reference}, band {band}c"
            ));
        }
        // Crossing depth: a marketable order may not cross beyond the touch
        // by more than max_cross.
        let max_cross = self.config.global.max_cross_cents;
        if let Some(b) = i.book {
            if is_bid {
                if let Some(ask) = b.best_ask() {
                    if yes_limit.raw() > ask.price.raw() + max_cross {
                        return Err(format!(
                            "buy (yes-space {yes_limit}) crosses best ask {} by more than {max_cross}c",
                            ask.price
                        ));
                    }
                }
            } else if let Some(bid) = b.best_bid() {
                if yes_limit.raw() < bid.price.raw() - max_cross {
                    return Err(format!(
                        "sell (yes-space {yes_limit}) crosses best bid {} by more than {max_cross}c",
                        bid.price
                    ));
                }
            }
        }
        Ok(format!(
            "yes-space {yes_limit} within {band}c of reference {reference}"
        ))
    }

    // ---- check 5 ----
    fn check_size_sanity(&self, c: &CandidateOrder) -> Result<String, String> {
        let g = &self.config.global;
        let qty = c.qty.raw();
        if qty < g.min_order_contracts || qty > g.max_order_contracts {
            return Err(format!(
                "quantity {qty} outside [{}, {}]",
                g.min_order_contracts, g.max_order_contracts
            ));
        }
        let limits = self.strategy_limits(c)?;
        let order_notional = notional(c.limit_price, c.qty)
            .map_err(|e| format!("notional arithmetic failed: {e}"))?;
        let cap = Cents::new(limits.max_order_notional_cents);
        if order_notional > cap {
            return Err(format!(
                "notional {order_notional} exceeds per-order cap {cap}"
            ));
        }
        Ok(format!("qty {qty}, notional {order_notional} <= {cap}"))
    }

    // ---- check 6 ----
    fn check_edge_floor(&self, c: &CandidateOrder, i: &GateInputs) -> Result<String, String> {
        if !(0..=100).contains(&c.fair_value.raw()) {
            return Err(format!(
                "fair value {} outside [0, 100] cents",
                c.fair_value
            ));
        }
        let limits = self.strategy_limits(c)?;
        let gross_per_contract = match c.action {
            Action::Buy => c.fair_value.raw() - c.limit_price.raw(),
            Action::Sell => c.limit_price.raw() - c.fair_value.raw(),
        };
        let qty = c.qty.raw();
        let gross = gross_per_contract
            .checked_mul(qty)
            .ok_or("edge arithmetic overflow")?;
        let fee = worst_case_fee(c, i)?;
        let net = gross
            .checked_sub(fee.raw())
            .ok_or("edge arithmetic overflow")?;
        let order_notional = notional(c.limit_price, c.qty)
            .map_err(|e| format!("notional arithmetic failed: {e}"))?;
        if order_notional.raw() <= 0 {
            return Err("notional must be positive for edge math".into());
        }
        // Floor division (i128, against us) before comparing to the floor.
        let net_bps = (i128::from(net) * 10_000).div_euclid(i128::from(order_notional.raw()));
        if net < 0 || net_bps < i128::from(limits.min_net_edge_bps) {
            return Err(format!(
                "net edge {net}c ({net_bps} bps) below floor {} bps (gross {gross}c, worst fee {fee})",
                limits.min_net_edge_bps
            ));
        }
        Ok(format!(
            "net edge {net}c = {net_bps} bps >= {} bps",
            limits.min_net_edge_bps
        ))
    }

    // ---- check 7 (I3) ----
    fn check_rate_limits(&mut self, c: &CandidateOrder, i: &GateInputs) -> Result<String, String> {
        let venue = c.venue.as_str().to_string();
        let Some(rate_cfg) = self.config.rate.get(&venue).cloned() else {
            return Err(format!(
                "no rate-limit config for venue {venue}: fail-closed"
            ));
        };
        let RateLimits {
            burst,
            sustained_per_min,
            market_burst,
            market_sustained_per_min,
        } = rate_cfg;
        let now = i.now;

        let venue_ok = self
            .venue_buckets
            .entry(venue.clone())
            .or_insert_with(|| Bucket::new(burst, sustained_per_min, now))
            .try_consume(now);
        if !venue_ok {
            let reason = format!(
                "venue {venue} rate limit breached (burst {burst}, sustained {sustained_per_min}/min)"
            );
            // I3: breach is a HALT, not a throttle.
            self.halts.set(HaltScope::Venue(venue), &reason);
            return Err(reason);
        }

        let market = c.market.as_str().to_string();
        let market_ok = self
            .market_buckets
            .entry((venue.clone(), market.clone()))
            .or_insert_with(|| Bucket::new(market_burst, market_sustained_per_min, now))
            .try_consume(now);
        if !market_ok {
            let reason = format!(
                "market {market} rate limit breached on {venue} (burst {market_burst}, sustained {market_sustained_per_min}/min)"
            );
            self.halts.set(HaltScope::Venue(venue), &reason);
            return Err(reason);
        }
        Ok("rate buckets consumed".into())
    }

    // ---- check 8 ----
    fn check_idempotency(&self, c: &CandidateOrder, i: &GateInputs) -> Result<String, String> {
        if i.recent_client_order_ids
            .contains(c.client_order_id.as_str())
        {
            return Err(format!("duplicate client order id {}", c.client_order_id));
        }
        Ok("client order id is fresh".into())
    }

    // ---- check 9 ----
    fn check_event_exposure(&self, c: &CandidateOrder, i: &GateInputs) -> Result<String, String> {
        match i.event_id {
            None => {
                if self.config.global.require_event_mapping {
                    Err("no event mapping for market and require_event_mapping is set".into())
                } else {
                    Ok("no event mapping: per-event cap not bindable for this order".into())
                }
            }
            Some(event) => {
                let cost = worst_case_cost(c, i)?;
                let cap = Cents::new(self.config.global.per_event_exposure_cents);
                let total = i
                    .event_exposure_cents
                    .checked_add(cost)
                    .map_err(|e| format!("event exposure arithmetic failed: {e}"))?;
                if total > cap {
                    return Err(format!(
                        "event {event} exposure {total} would exceed cap {cap}"
                    ));
                }
                Ok(format!("event {event} exposure {total} <= {cap}"))
            }
        }
    }

    // ---- check 10 ----
    fn check_internal_netting(&self, c: &CandidateOrder, i: &GateInputs) -> Result<String, String> {
        let (c_is_bid, c_yes) = yes_space(c)?;
        for own in i.own_resting {
            if own.market != c.market {
                continue;
            }
            let (own_is_bid, own_yes) = yes_space_view(own)?;
            let crosses = match (c_is_bid, own_is_bid) {
                (true, false) => c_yes >= own_yes, // our new bid hits our own ask
                (false, true) => c_yes <= own_yes, // our new ask hits our own bid
                _ => false,
            };
            if crosses {
                return Err(format!(
                    "order (yes-space {c_yes}) would cross own resting order (yes-space {own_yes}) on {}",
                    c.market
                ));
            }
        }
        Ok("no self-crossing".into())
    }

    // ---- check 11 ----
    fn check_book_age(&self, _c: &CandidateOrder, i: &GateInputs) -> Result<String, String> {
        let Some(max) = self.config.max_book_age_ms else {
            return Ok("book-age check disabled".into());
        };
        let Some(book) = i.book else {
            return Ok("no book to age-check".into());
        };
        let age_ms = i
            .now
            .epoch_millis()
            .saturating_sub(book.as_of.epoch_millis());
        if age_ms > max {
            return Err(format!("book stale: {age_ms}ms > {max}ms"));
        }
        Ok(format!("book age {age_ms}ms <= {max}ms"))
    }

    fn strategy_limits(&self, c: &CandidateOrder) -> Result<&StrategyLimits, String> {
        self.strategy_limits_by_id(&c.strategy)
    }

    /// Shared with the perp arm (crate::perp): fail-closed strategy lookup.
    pub(crate) fn strategy_limits_by_id(
        &self,
        strategy: &StrategyId,
    ) -> Result<&StrategyLimits, String> {
        self.config
            .per_strategy
            .get(strategy.as_str())
            .ok_or_else(|| format!("no limits configured for strategy {strategy}: fail-closed"))
    }
}

/// (is_bid_in_yes_space, yes_price) for a candidate.
fn yes_space(c: &CandidateOrder) -> Result<(bool, Cents), String> {
    yes_space_parts(c.side, c.action, c.limit_price)
}

fn yes_space_view(v: &RestingOrderView) -> Result<(bool, Cents), String> {
    yes_space_parts(v.side, v.action, v.price)
}

fn yes_space_parts(side: Side, action: Action, price: Cents) -> Result<(bool, Cents), String> {
    let mirrored = Cents::new(100)
        .checked_sub(price)
        .map_err(|e| format!("price mirror arithmetic failed: {e}"))?;
    Ok(match (side, action) {
        (Side::Yes, Action::Buy) => (true, price),
        (Side::Yes, Action::Sell) => (false, price),
        (Side::No, Action::Buy) => (false, mirrored),
        (Side::No, Action::Sell) => (true, mirrored),
    })
}

/// Worst-case modeled fee: max(maker, taker, 0) at the limit price.
fn worst_case_fee(c: &CandidateOrder, i: &GateInputs) -> Result<Cents, String> {
    let taker = i
        .fee_model
        .fee(FillRole::Taker, c.limit_price, c.qty, i.category, i.now)
        .map_err(|e| format!("fee model failed: {e}"))?;
    let maker = i
        .fee_model
        .fee(FillRole::Maker, c.limit_price, c.qty, i.category, i.now)
        .map_err(|e| format!("fee model failed: {e}"))?;
    Ok(taker.max(maker).max(Cents::ZERO))
}

/// Worst-case capital cost. Buys: notional + worst fee. Sells: zero
/// additional exposure (close-only venue semantics; see ASSUMPTIONS.md).
fn worst_case_cost(c: &CandidateOrder, i: &GateInputs) -> Result<Cents, String> {
    match c.action {
        Action::Sell => Ok(Cents::ZERO),
        Action::Buy => {
            let cost = notional(c.limit_price, c.qty)
                .map_err(|e| format!("notional arithmetic failed: {e}"))?;
            let fee = worst_case_fee(c, i)?;
            cost.checked_add(fee)
                .map_err(|e| format!("cost arithmetic failed: {e}"))
        }
    }
}
