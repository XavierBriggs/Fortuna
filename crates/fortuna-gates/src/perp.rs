//! The perp arm of the universal gate pipeline (spec 5.15 "New gates";
//! T5.B3).
//!
//! Perp orders pass through the SAME `GatePipeline` instance as event
//! contracts: the same halt flags (a venue halt blocks both domains), the
//! same I3 rate buckets (a breach on either arm halts both), the same audit
//! record stream, fail-closed everywhere. They seal into `GatedPerpOrder` —
//! the same private-constructor discipline as `GatedOrder`; a separate
//! sealed type because 5.15's type-level price separation forbids a
//! `Cents`-typed order from carrying a `PerpPrice`.
//!
//! Loss model (spec 5.15): the margin ACCOUNT — not the position — is the
//! exposure unit, and worst-case exposure for any perp order is the
//! LIQUIDATION loss, never the premium analogy. The venue's maintenance-
//! margin formula is unpublished: these checks approximate it from the
//! configured risk curve (recorded venue leverage_estimates by notional)
//! with a safety multiplier, and REFUSE any order whose worst case the
//! approximation cannot bound.
//!
//! Reduce-only doctrine: a VALID reduce-only order (opposite the position,
//! qty <= |position|) is exposure-REDUCING — the risk gates (margin
//! headroom, liquidation distance, leverage, notional caps) and the edge
//! floor pass it with an exposure-reducing note, because rejecting a
//! de-risking close would force staying at higher risk (and blocking a
//! stop-loss on a margined instrument is how margin models become losses).
//! It still faces halts, price/size sanity, rate limits, idempotency, and
//! internal netting in full.
//!
//! Check order (PERP_ALL): Halts(1), MarginHeadroom(11),
//! LiquidationDistance(12), LeverageCap(13), PerpNotionalCap(14),
//! PriceSanity(4), SizeSanity(5), EdgeFloor(6, with funding drag + fee-trap
//! assumed fees), RateLimits(7), Idempotency(8), InternalNetting(10).

use crate::config::{PerpAssetLimits, PerpVenueLimits, RateLimits};
use crate::pipeline::{GateCheck, GateCheckRecord, GatePipeline, GateRejection, Verdict};
use crate::rate::Bucket;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::IntentId;
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{MarginAccountView, PerpPosition, PerpPrice};
use serde::Serialize;
use std::collections::BTreeSet;

/// The perp-arm pipeline order. Mirrors spec 5.3's shape: halts, capital
/// (margin headroom is the capital analog), risk caps, sanity, edge, rate,
/// idempotency, netting.
pub const PERP_ALL: [GateCheck; 11] = [
    GateCheck::Halts,
    GateCheck::MarginHeadroom,
    GateCheck::LiquidationDistance,
    GateCheck::LeverageCap,
    GateCheck::PerpNotionalCap,
    GateCheck::PriceSanity,
    GateCheck::SizeSanity,
    GateCheck::EdgeFloor,
    GateCheck::RateLimits,
    GateCheck::Idempotency,
    GateCheck::InternalNetting,
];

/// A pre-gate perp order. `fair_value` is the deterministic calibrated
/// fair price; `holding_windows` is the strategy's declared intended
/// holding period in 8h funding windows (funding drag, spec 5.15 gate (c)).
#[derive(Debug, Clone)]
pub struct PerpCandidateOrder {
    pub intent_id: IntentId,
    pub strategy: StrategyId,
    pub venue: VenueId,
    pub market: MarketId,
    /// Buy = long-increasing, Sell = short-increasing (venue side).
    pub action: Action,
    /// Reduce-only close. The venue requires IOC/FOK on reduce-only
    /// orders; that is execution policy (T5.B4), not gate scope.
    pub reduce_only: bool,
    pub limit_price: PerpPrice,
    /// Positive contract count; direction rides in `action`.
    pub qty: Contracts,
    pub fair_value: PerpPrice,
    /// Intended holding period in 8h funding windows; must be >= 1 (every
    /// position can cross a funding tick).
    pub holding_windows: u32,
    pub client_order_id: ClientOrderId,
}

/// Our own resting perp order, as the netting check needs to see it.
#[derive(Debug, Clone)]
pub struct PerpRestingOrderView {
    pub market: MarketId,
    pub action: Action,
    pub price: PerpPrice,
}

/// Deterministic state views the perp checks consume. The account view and
/// the mark are CONSERVATIVE (worse-for-us) per spec 5.15; the caller
/// (state layer) builds them that way.
pub struct PerpGateInputs<'a> {
    pub now: UtcTimestamp,
    /// Margin account view at conservative marks (the I2 equity input).
    pub account: &'a MarginAccountView,
    /// Current signed position in the candidate's market, if any.
    pub position: Option<&'a PerpPosition>,
    /// The conservative mark for this market (worse-for-us of venue
    /// settlement mark and our independent mark).
    pub conservative_mark: PerpPrice,
    /// Worst-case open perps notional already on this venue (positions +
    /// resting orders), for the per-venue cap.
    pub venue_open_notional_cents: Cents,
    pub own_resting: &'a [PerpRestingOrderView],
    pub recent_client_order_ids: &'a BTreeSet<String>,
}

/// A perp order that has passed the full perp gate arm. Constructible only
/// by fortuna-gates (same discipline as `GatedOrder`: private fields, no
/// public constructor, Serialize ONLY — a Deserialize impl would be a
/// constructor bypass and is forbidden).
#[derive(Debug, Clone, Serialize)]
pub struct GatedPerpOrder {
    intent_id: IntentId,
    strategy: StrategyId,
    venue: VenueId,
    market: MarketId,
    action: Action,
    reduce_only: bool,
    limit_price: PerpPrice,
    qty: Contracts,
    client_order_id: ClientOrderId,
}

impl GatedPerpOrder {
    /// THE ONLY CONSTRUCTOR. pub(crate): callable solely from the perp gate
    /// arm, after every check has passed. Widening this visibility or
    /// adding any other construction path (including Deserialize) is an I1
    /// violation.
    pub(crate) fn assemble(candidate: &PerpCandidateOrder) -> GatedPerpOrder {
        GatedPerpOrder {
            intent_id: candidate.intent_id,
            strategy: candidate.strategy.clone(),
            venue: candidate.venue.clone(),
            market: candidate.market.clone(),
            action: candidate.action,
            reduce_only: candidate.reduce_only,
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

    pub fn action(&self) -> Action {
        self.action
    }

    pub fn reduce_only(&self) -> bool {
        self.reduce_only
    }

    pub fn limit_price(&self) -> PerpPrice {
        self.limit_price
    }

    pub fn qty(&self) -> Contracts {
        self.qty
    }

    pub fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }
}

/// Evaluation result for the perp arm: the sealed order (or rejection)
/// plus the audit trail (same record type as the event-contract arm).
#[derive(Debug)]
pub struct PerpGateOutcome {
    pub gated: Result<GatedPerpOrder, GateRejection>,
    pub records: Vec<GateCheckRecord>,
}

/// Per-candidate exposure derivation, computed once after the halt check.
/// Worst-case quantity is the largest |position| along the fill path:
/// max(|pos|, |pos + delta|) for risk-adding orders.
struct PerpProfile {
    /// True for a VALID reduce-only order (opposite direction, qty bounded
    /// by the position): exposure-reducing by construction.
    reducing: bool,
    /// Worst-case absolute position while the order is live (contracts).
    worst_qty: i64,
    /// Worst-case position notional: worst_qty x max(limit, mark), ceiled.
    worst_notional: Cents,
    /// The order's own notional at limit: qty x limit, ceiled.
    order_notional: Cents,
    /// Funding drag over the declared holding windows, ceiled.
    drag: Cents,
}

impl GatePipeline {
    /// Run the perp gate arm (PERP_ALL order), fail-closed, emitting one
    /// audit record per evaluated check. Shares halt flags and I3 buckets
    /// with the event-contract arm.
    pub fn evaluate_perp(
        &mut self,
        candidate: &PerpCandidateOrder,
        inputs: &PerpGateInputs,
    ) -> PerpGateOutcome {
        let mut records = Vec::with_capacity(PERP_ALL.len());

        // Check 1: halts (shared flags).
        let halts = match self
            .halts
            .blocking(candidate.strategy.as_str(), candidate.venue.as_str())
        {
            Some((scope, reason)) => Err(format!("halted ({scope}): {reason}")),
            None => Ok("no halt flags set".to_string()),
        };
        if let Some(out) = push(&mut records, GateCheck::Halts, halts, candidate, inputs.now) {
            return out;
        }

        // Everything after the halt check needs the exposure profile and
        // the perp config; failure to derive either is a rejection at the
        // first perp check (fail-closed).
        let derived: Result<(PerpProfile, &PerpVenueLimits, &PerpAssetLimits), String> = (|| {
            let venue_cfg = self
                .config
                .perp
                .venues
                .get(candidate.venue.as_str())
                .ok_or_else(|| {
                    format!("no perp config for venue {}: fail-closed", candidate.venue)
                })?;
            let asset_cfg = self
                .config
                .perp
                .assets
                .get(candidate.market.as_str())
                .ok_or_else(|| {
                    format!("no perp config for asset {}: fail-closed", candidate.market)
                })?;
            let profile = build_profile(candidate, inputs, venue_cfg)?;
            Ok((profile, venue_cfg, asset_cfg))
        })(
        );
        let (profile, venue_cfg, asset_cfg) = match derived {
            Ok(parts) => parts,
            Err(reason) => {
                if let Some(out) = push(
                    &mut records,
                    GateCheck::MarginHeadroom,
                    Err(reason),
                    candidate,
                    inputs.now,
                ) {
                    return out;
                }
                // push always returns Some on Err; unreachable, fail closed.
                return PerpGateOutcome {
                    gated: Err(GateRejection {
                        check: GateCheck::MarginHeadroom,
                        reason: "profile derivation failed".into(),
                    }),
                    records,
                };
            }
        };

        // Checks 11/12/13/14, 4/5/6 (perp arms), 7, 8, 10.
        let checks: [(GateCheck, Result<String, String>); 7] = [
            (
                GateCheck::MarginHeadroom,
                check_margin_headroom(&profile, venue_cfg, asset_cfg, inputs),
            ),
            (
                GateCheck::LiquidationDistance,
                check_liquidation_distance(&profile, venue_cfg, asset_cfg, inputs),
            ),
            (
                GateCheck::LeverageCap,
                check_leverage_cap(&profile, venue_cfg, asset_cfg, inputs),
            ),
            (
                GateCheck::PerpNotionalCap,
                check_notional_caps(&profile, venue_cfg, asset_cfg, inputs),
            ),
            (
                GateCheck::PriceSanity,
                check_price_sanity(candidate, venue_cfg, inputs),
            ),
            (
                GateCheck::SizeSanity,
                self.check_perp_size_sanity(candidate, &profile, venue_cfg),
            ),
            (
                GateCheck::EdgeFloor,
                self.check_perp_edge_floor(candidate, &profile, venue_cfg),
            ),
        ];
        for (check, result) in checks {
            if let Some(out) = push(&mut records, check, result, candidate, inputs.now) {
                return out;
            }
        }

        let rate = self.consume_perp_rate(candidate, inputs.now);
        if let Some(out) = push(
            &mut records,
            GateCheck::RateLimits,
            rate,
            candidate,
            inputs.now,
        ) {
            return out;
        }

        let idem = if inputs
            .recent_client_order_ids
            .contains(candidate.client_order_id.as_str())
        {
            Err(format!(
                "duplicate client order id {}",
                candidate.client_order_id
            ))
        } else {
            Ok("client order id is fresh".to_string())
        };
        if let Some(out) = push(
            &mut records,
            GateCheck::Idempotency,
            idem,
            candidate,
            inputs.now,
        ) {
            return out;
        }

        let netting = check_internal_netting(candidate, inputs);
        if let Some(out) = push(
            &mut records,
            GateCheck::InternalNetting,
            netting,
            candidate,
            inputs.now,
        ) {
            return out;
        }

        PerpGateOutcome {
            gated: Ok(GatedPerpOrder::assemble(candidate)),
            records,
        }
    }

    // ---- check 5 (perp arm) ----
    fn check_perp_size_sanity(
        &self,
        c: &PerpCandidateOrder,
        profile: &PerpProfile,
        venue_cfg: &PerpVenueLimits,
    ) -> Result<String, String> {
        let qty = c.qty.raw();
        if qty < venue_cfg.min_order_contracts || qty > venue_cfg.max_order_contracts {
            return Err(format!(
                "quantity {qty} outside [{}, {}]",
                venue_cfg.min_order_contracts, venue_cfg.max_order_contracts
            ));
        }
        let limits = self.strategy_limits_by_id(&c.strategy)?;
        let cap = Cents::new(limits.max_order_notional_cents);
        if profile.order_notional > cap {
            return Err(format!(
                "notional {} exceeds per-order cap {cap}",
                profile.order_notional
            ));
        }
        Ok(format!(
            "qty {qty}, notional {} <= {cap}",
            profile.order_notional
        ))
    }

    // ---- check 6 (perp arm): edge floor with funding drag + assumed fees
    // (fee-trap rule, spec 5.15) ----
    fn check_perp_edge_floor(
        &self,
        c: &PerpCandidateOrder,
        profile: &PerpProfile,
        venue_cfg: &PerpVenueLimits,
    ) -> Result<String, String> {
        if profile.reducing {
            return Ok(
                "exposure-reducing close: edge floor not applicable (blocking a de-risking \
                 close would force staying at higher risk)"
                    .to_string(),
            );
        }
        if c.holding_windows == 0 {
            return Err(
                "holding_windows must be >= 1: every position can cross a funding tick".into(),
            );
        }
        let limits = self.strategy_limits_by_id(&c.strategy)?;
        if profile.order_notional.raw() <= 0 {
            return Err("notional must be positive for edge math".into());
        }
        let diff = match c.action {
            Action::Buy => c.fair_value.checked_sub(c.limit_price),
            Action::Sell => c.limit_price.checked_sub(c.fair_value),
        }
        .map_err(|e| format!("edge arithmetic failed: {e}"))?;
        // Gains floor toward -inf (fortuna-core conversion doctrine).
        let gross = diff
            .checked_mul(c.qty.raw())
            .map_err(|e| format!("edge arithmetic failed: {e}"))?
            .to_cents_floor();
        let fee = ceil_bps(profile.order_notional.raw(), venue_cfg.assumed_fee_bps, 1)?;
        let net = gross
            .checked_sub(fee)
            .and_then(|n| n.checked_sub(profile.drag))
            .map_err(|e| format!("edge arithmetic failed: {e}"))?;
        // Floor division (i128, against us) before comparing to the floor.
        let net_bps =
            (i128::from(net.raw()) * 10_000).div_euclid(i128::from(profile.order_notional.raw()));
        if net.raw() < 0 || net_bps < i128::from(limits.min_net_edge_bps) {
            return Err(format!(
                "net edge {net} ({net_bps} bps) below floor {} bps (gross {gross}, assumed fee \
                 {fee}, funding drag {} over {} window(s))",
                limits.min_net_edge_bps, profile.drag, c.holding_windows
            ));
        }
        Ok(format!(
            "net edge {net} = {net_bps} bps >= {} bps (assumed fee {fee}, funding drag {})",
            limits.min_net_edge_bps, profile.drag
        ))
    }

    // ---- check 7 (I3, perp arm): same dual token buckets as the
    // event-contract arm (crate::pipeline::check_rate_limits); kept in
    // lockstep deliberately — a breach on either arm halts the venue for
    // BOTH domains. ----
    fn consume_perp_rate(
        &mut self,
        c: &PerpCandidateOrder,
        now: UtcTimestamp,
    ) -> Result<String, String> {
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
            self.halts
                .set(crate::halt::HaltScope::Venue(venue), &reason);
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
            self.halts
                .set(crate::halt::HaltScope::Venue(venue), &reason);
            return Err(reason);
        }
        Ok("rate buckets consumed".into())
    }
}

/// Push one audit record; on rejection return the short-circuit outcome.
fn push(
    records: &mut Vec<GateCheckRecord>,
    check: GateCheck,
    result: Result<String, String>,
    candidate: &PerpCandidateOrder,
    now: UtcTimestamp,
) -> Option<PerpGateOutcome> {
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
        at: now,
        intent_id: candidate.intent_id,
        client_order_id: candidate.client_order_id.as_str().to_string(),
    });
    rejection.map(|rejection| PerpGateOutcome {
        gated: Err(rejection),
        records: std::mem::take(records),
    })
}

/// Derive the exposure profile. Errors here reject at MarginHeadroom
/// (fail-closed).
fn build_profile(
    c: &PerpCandidateOrder,
    i: &PerpGateInputs,
    venue_cfg: &PerpVenueLimits,
) -> Result<PerpProfile, String> {
    let qty = c.qty.raw();
    if qty < 0 {
        return Err("negative order quantity".into());
    }
    if let Some(p) = i.position {
        if p.market != c.market {
            return Err(format!(
                "position view is for {} but the order is for {}: refusing to certify",
                p.market, c.market
            ));
        }
    }
    let pos_qty = i.position.map(|p| p.qty.raw()).unwrap_or(0);
    let delta = match c.action {
        Action::Buy => qty,
        Action::Sell => qty.checked_neg().ok_or("quantity negation overflow")?,
    };

    let (reducing, worst_qty) = if c.reduce_only {
        if pos_qty == 0 {
            return Err("reduce_only with no position in this market".into());
        }
        let same_direction = (pos_qty > 0) == (delta > 0);
        if same_direction {
            return Err("reduce_only order in the position's own direction".into());
        }
        let abs_pos = pos_qty.checked_abs().ok_or("position abs overflow")?;
        if qty > abs_pos {
            return Err(format!(
                "reduce_only qty {qty} exceeds position {abs_pos}: would flip, not reduce"
            ));
        }
        (true, abs_pos - qty)
    } else {
        // Worst case along the fill path: the position passes through every
        // value between pos and pos+delta while the order fills.
        let abs_pos = pos_qty.checked_abs().ok_or("position abs overflow")?;
        let post = pos_qty
            .checked_add(delta)
            .ok_or("position arithmetic overflow")?;
        let abs_post = post.checked_abs().ok_or("position abs overflow")?;
        (false, abs_pos.max(abs_post))
    };

    // Conservative per-contract valuation: the worse of limit and mark.
    let per_contract_tt = c.limit_price.raw().max(i.conservative_mark.raw()).max(0);
    let worst_notional = cents_ceil_from_tt_product(worst_qty, per_contract_tt)?;
    let order_notional = cents_ceil_from_tt_product(qty, c.limit_price.raw().max(0))?;
    let drag = ceil_bps(
        order_notional.raw(),
        venue_cfg.funding_drag_bps_per_window,
        i64::from(c.holding_windows),
    )?;

    Ok(PerpProfile {
        reducing,
        worst_qty,
        worst_notional,
        order_notional,
        drag,
    })
}

/// The maintenance-margin approximation (spec 5.15): risk-curve lookup at
/// the worst-case notional, with the safety multiplier. A notional beyond
/// the last tier cannot be bounded: REFUSE.
fn required_margin(
    worst_notional: Cents,
    venue_cfg: &PerpVenueLimits,
    asset_cfg: &PerpAssetLimits,
) -> Result<Cents, String> {
    let n = worst_notional.raw();
    let mm_bps = asset_cfg
        .mm_curve
        .iter()
        .find(|(threshold, _)| n <= *threshold)
        .map(|(_, bps)| *bps)
        .ok_or_else(|| {
            format!(
                "worst-case notional {worst_notional} exceeds the last risk-curve tier: the \
                 margin approximation cannot bound this order (spec 5.15: refuse)"
            )
        })?;
    let mm = ceil_bps(n, mm_bps, 1)?;
    // Safety multiplier, ceiled (percent, >= 100 by validation).
    let required = ceil_div_i128(
        i128::from(mm.raw()) * i128::from(venue_cfg.mm_safety_multiplier_pct),
        100,
    );
    cents_from_i128(required, "required margin")
}

// ---- check 11 ----
fn check_margin_headroom(
    profile: &PerpProfile,
    venue_cfg: &PerpVenueLimits,
    asset_cfg: &PerpAssetLimits,
    i: &PerpGateInputs,
) -> Result<String, String> {
    if profile.reducing {
        return Ok("exposure-reducing close: margin headroom can only improve".to_string());
    }
    let required = required_margin(profile.worst_notional, venue_cfg, asset_cfg)?;
    let available = i
        .account
        .equity
        .checked_sub(profile.drag)
        .map_err(|e| format!("headroom arithmetic failed: {e}"))?;
    if available < required {
        return Err(format!(
            "required margin {required} (worst-case notional {}, safety x{}%) exceeds available \
             equity {available} (equity {} - funding drag {})",
            profile.worst_notional,
            venue_cfg.mm_safety_multiplier_pct,
            i.account.equity,
            profile.drag
        ));
    }
    Ok(format!(
        "required margin {required} <= available {available} (worst-case notional {})",
        profile.worst_notional
    ))
}

// ---- check 12 (spec 5.15 gate (a)) ----
fn check_liquidation_distance(
    profile: &PerpProfile,
    venue_cfg: &PerpVenueLimits,
    asset_cfg: &PerpAssetLimits,
    i: &PerpGateInputs,
) -> Result<String, String> {
    if profile.reducing {
        return Ok("exposure-reducing close: liquidation distance can only grow".to_string());
    }
    if profile.worst_qty == 0 {
        return Ok("no worst-case exposure: liquidation distance not in play".to_string());
    }
    let mark = i.conservative_mark.raw();
    if mark <= 0 {
        return Err("no usable conservative mark: cannot certify liquidation distance".into());
    }
    let required = required_margin(profile.worst_notional, venue_cfg, asset_cfg)?;
    let available = i
        .account
        .equity
        .checked_sub(profile.drag)
        .map_err(|e| format!("distance arithmetic failed: {e}"))?;
    // Buffer above the (safety-multiplied) maintenance level; headroom has
    // already passed, so this is non-negative — clamp defensively anyway.
    let buffer = available
        .checked_sub(required)
        .map_err(|e| format!("distance arithmetic failed: {e}"))?
        .max(Cents::ZERO);
    // Price move (in ten-thousandths) the buffer absorbs, per contract,
    // floored (underestimating the distance is the conservative side).
    let move_tt = (i128::from(buffer.raw()) * 100).div_euclid(i128::from(profile.worst_qty));
    let distance_bps = (move_tt * 10_000).div_euclid(i128::from(mark));
    if distance_bps < i128::from(venue_cfg.min_liquidation_distance_bps) {
        return Err(format!(
            "estimated liquidation distance {distance_bps} bps of mark is below the floor {} \
             bps (buffer {buffer} over {} contracts)",
            venue_cfg.min_liquidation_distance_bps, profile.worst_qty
        ));
    }
    Ok(format!(
        "liquidation distance {distance_bps} bps >= floor {} bps",
        venue_cfg.min_liquidation_distance_bps
    ))
}

// ---- check 13 (spec 5.15 gate (b); operator 2x ceiling 2026-06-12) ----
fn check_leverage_cap(
    profile: &PerpProfile,
    venue_cfg: &PerpVenueLimits,
    asset_cfg: &PerpAssetLimits,
    i: &PerpGateInputs,
) -> Result<String, String> {
    if profile.reducing {
        return Ok("exposure-reducing close: leverage can only fall".to_string());
    }
    // Effective cap = min(operator venue-wide ceiling, per-asset cap);
    // an absent venue ceiling leaves the per-asset (venue-curve-derived)
    // cap as the ceiling — the operator's documented interim.
    let cap_x10 = match venue_cfg.max_leverage_x10 {
        Some(venue_cap) => venue_cap.min(asset_cfg.max_leverage_x10),
        None => asset_cfg.max_leverage_x10,
    };
    // worst_notional / equity <= cap/10, cross-multiplied in i128:
    // worst_notional x 10 <= equity x cap_x10.
    let lhs = i128::from(profile.worst_notional.raw()) * 10;
    let rhs = i128::from(i.account.equity.raw()) * i128::from(cap_x10);
    if lhs > rhs {
        return Err(format!(
            "worst-case notional {} exceeds the {}.{}x leverage cap on equity {}",
            profile.worst_notional,
            cap_x10 / 10,
            cap_x10 % 10,
            i.account.equity
        ));
    }
    Ok(format!(
        "worst-case notional {} within leverage cap ({} x10) on equity {}",
        profile.worst_notional, cap_x10, i.account.equity
    ))
}

// ---- check 14 (spec 5.15 gate (d)) ----
fn check_notional_caps(
    profile: &PerpProfile,
    venue_cfg: &PerpVenueLimits,
    asset_cfg: &PerpAssetLimits,
    i: &PerpGateInputs,
) -> Result<String, String> {
    if profile.reducing {
        return Ok("exposure-reducing close: notional can only fall".to_string());
    }
    let venue_cap = Cents::new(venue_cfg.max_total_notional_cents);
    let venue_total = i
        .venue_open_notional_cents
        .checked_add(profile.order_notional)
        .map_err(|e| format!("venue notional arithmetic failed: {e}"))?;
    if venue_total > venue_cap {
        return Err(format!(
            "venue perps notional {venue_total} would exceed cap {venue_cap}"
        ));
    }
    let asset_cap = Cents::new(asset_cfg.max_notional_cents);
    if profile.worst_notional > asset_cap {
        return Err(format!(
            "worst-case position notional {} would exceed per-asset cap {asset_cap}",
            profile.worst_notional
        ));
    }
    Ok(format!(
        "venue {venue_total} <= {venue_cap}, asset worst-case {} <= {asset_cap}",
        profile.worst_notional
    ))
}

// ---- check 4 (perp arm) ----
fn check_price_sanity(
    c: &PerpCandidateOrder,
    venue_cfg: &PerpVenueLimits,
    i: &PerpGateInputs,
) -> Result<String, String> {
    let limit = c.limit_price.raw();
    if limit <= 0 {
        return Err(format!("limit {} is not a positive price", c.limit_price));
    }
    let mark = i.conservative_mark.raw();
    if mark <= 0 {
        return Err("no usable conservative mark: cannot certify price sanity".into());
    }
    // |limit - mark| as bps of mark, cross-multiplied (no division).
    let distance = i128::from((limit - mark).abs()) * 10_000;
    let band = i128::from(venue_cfg.price_band_bps) * i128::from(mark);
    if distance > band {
        return Err(format!(
            "limit {} is more than {} bps from conservative mark {}",
            c.limit_price, venue_cfg.price_band_bps, i.conservative_mark
        ));
    }
    Ok(format!(
        "limit {} within {} bps of mark {}",
        c.limit_price, venue_cfg.price_band_bps, i.conservative_mark
    ))
}

// ---- check 10 (perp arm) ----
fn check_internal_netting(c: &PerpCandidateOrder, i: &PerpGateInputs) -> Result<String, String> {
    for own in i.own_resting {
        if own.market != c.market {
            continue;
        }
        let crosses = match (c.action, own.action) {
            // Our new buy at or above our own resting sell.
            (Action::Buy, Action::Sell) => c.limit_price >= own.price,
            // Our new sell at or below our own resting buy.
            (Action::Sell, Action::Buy) => c.limit_price <= own.price,
            _ => false,
        };
        if crosses {
            return Err(format!(
                "order at {} would cross own resting order at {} on {}",
                c.limit_price, own.price, c.market
            ));
        }
    }
    Ok("no self-crossing".into())
}

// ---- integer helpers (i128 intermediates, explicit rounding) ----

/// ceil(n / d) toward +inf for d > 0.
fn ceil_div_i128(n: i128, d: i128) -> i128 {
    let q = n.div_euclid(d);
    if n.rem_euclid(d) != 0 {
        q + 1
    } else {
        q
    }
}

fn cents_from_i128(v: i128, what: &str) -> Result<Cents, String> {
    i64::try_from(v)
        .map(Cents::new)
        .map_err(|_| format!("{what} exceeds the representable cent range"))
}

/// qty x per-contract ten-thousandths, converted to cents rounded UP
/// (exposure is never understated).
fn cents_ceil_from_tt_product(qty: i64, per_contract_tt: i64) -> Result<Cents, String> {
    let tt = i128::from(qty) * i128::from(per_contract_tt);
    cents_from_i128(ceil_div_i128(tt, 100), "notional")
}

/// notional x bps x scale / 10_000, ceiled (costs round up).
fn ceil_bps(notional_cents: i64, bps: i64, scale: i64) -> Result<Cents, String> {
    let raw = i128::from(notional_cents) * i128::from(bps) * i128::from(scale);
    cents_from_i128(ceil_div_i128(raw, 10_000), "bps cost")
}
