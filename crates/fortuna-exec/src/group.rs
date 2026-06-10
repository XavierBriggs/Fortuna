//! Multi-leg execution: IntentGroup completion policy. Spec 5.4.
//!
//! Legs submit as one group with a declared completion policy (max unhedged
//! notional, max leg-open duration). Breaching either bound triggers a
//! DETERMINISTIC complete-or-unwind decision: taker-complete if the group's
//! remaining value net of taker fees still clears the floor, otherwise
//! unwind. The tracker only decides; completion/unwind orders are new
//! candidates that pass the full gate pipeline like everything else (I1).
//! Unwind decisions are recorded so execution-loss attribution lands on the
//! strategy (the documented 62% combinatorial-arb failure mode is survived
//! or measured, never hidden).

use crate::manager::{IntentStatus, IntentView};
use fortuna_core::book::{FeeModel, FillRole, OrderBook};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IntentGroupId, IntentId};
use fortuna_core::market::{notional, Action, Contracts, MarketId, Side};
use fortuna_core::money::Cents;
use std::collections::BTreeMap;

/// Completion policy declared at group open (spec 5.4).
#[derive(Debug, Clone)]
pub struct GroupPolicy {
    pub max_unhedged_notional: Cents,
    pub max_leg_open_ms: i64,
    /// What one completed "set" of all legs is worth (e.g. 100c for a
    /// YES+NO pair). Drives the complete-or-unwind economics.
    pub value_per_set: Cents,
    /// Completion must clear this net-edge floor (bps of completion cost).
    pub min_completion_edge_bps: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupStatus {
    Forming,
    Complete,
    Breached,
    Unwinding,
    Closed,
}

#[derive(Debug, Clone)]
pub struct GroupState {
    pub id: IntentGroupId,
    pub legs: Vec<IntentId>,
    pub policy: GroupPolicy,
    pub opened_at: UtcTimestamp,
    pub status: GroupStatus,
}

/// What the evaluator tells the runner to do.
#[derive(Debug)]
pub enum GroupDecision {
    /// Every leg fully filled: hedged and done.
    Complete { group: IntentGroupId },
    /// A bound was breached: run complete-or-unwind on the remaining legs.
    Breached {
        group: IntentGroupId,
        reason: String,
        unfilled_legs: Vec<IntentId>,
    },
}

/// Tracks group lifecycles over the order manager's intent records.
#[derive(Debug, Default)]
pub struct GroupTracker {
    groups: BTreeMap<IntentGroupId, GroupState>,
}

impl GroupTracker {
    pub fn open(
        &mut self,
        id: IntentGroupId,
        policy: GroupPolicy,
        legs: Vec<IntentId>,
        at: UtcTimestamp,
    ) {
        self.groups.insert(
            id,
            GroupState {
                id,
                legs,
                policy,
                opened_at: at,
                status: GroupStatus::Forming,
            },
        );
    }

    pub fn group(&self, id: IntentGroupId) -> Option<&GroupState> {
        self.groups.get(&id)
    }

    pub fn mark_unwinding(&mut self, id: IntentGroupId) {
        if let Some(g) = self.groups.get_mut(&id) {
            g.status = GroupStatus::Unwinding;
        }
    }

    pub fn mark_closed(&mut self, id: IntentGroupId) {
        if let Some(g) = self.groups.get_mut(&id) {
            g.status = GroupStatus::Closed;
        }
    }

    /// Evaluate every forming group against its policy. Deterministic over
    /// the manager's folded state and the injected now.
    pub fn evaluate<M: IntentView>(
        &mut self,
        manager: &M,
        now: UtcTimestamp,
    ) -> Vec<GroupDecision> {
        let mut decisions = Vec::new();
        for g in self.groups.values_mut() {
            if g.status != GroupStatus::Forming {
                continue;
            }
            let mut all_filled = true;
            let mut filled_notionals: Vec<Cents> = Vec::new();
            let mut unfilled = Vec::new();
            for leg in &g.legs {
                let Some(rec) = manager.intent_record(*leg) else {
                    continue;
                };
                let filled = notional(rec.order.limit_price, rec.cum_filled).unwrap_or(Cents::ZERO);
                filled_notionals.push(filled);
                if rec.status != IntentStatus::Filled {
                    all_filled = false;
                    unfilled.push(*leg);
                }
            }
            if all_filled {
                g.status = GroupStatus::Complete;
                decisions.push(GroupDecision::Complete { group: g.id });
                continue;
            }
            // Unhedged notional: spread between the most- and least-filled
            // legs' filled notional (v1 measure of imbalance).
            let max_fill = filled_notionals
                .iter()
                .max()
                .copied()
                .unwrap_or(Cents::ZERO);
            let min_fill = filled_notionals
                .iter()
                .min()
                .copied()
                .unwrap_or(Cents::ZERO);
            let unhedged = max_fill.checked_sub(min_fill).unwrap_or(Cents::ZERO);
            let age_ms = now
                .epoch_millis()
                .saturating_sub(g.opened_at.epoch_millis());

            let breach = if unhedged > g.policy.max_unhedged_notional {
                Some(format!(
                    "unhedged notional {unhedged} exceeds {}",
                    g.policy.max_unhedged_notional
                ))
            } else if age_ms > g.policy.max_leg_open_ms {
                Some(format!(
                    "leg open {age_ms}ms exceeds {}ms",
                    g.policy.max_leg_open_ms
                ))
            } else {
                None
            };
            if let Some(reason) = breach {
                g.status = GroupStatus::Breached;
                decisions.push(GroupDecision::Breached {
                    group: g.id,
                    reason,
                    unfilled_legs: unfilled.clone(),
                });
            }
        }
        decisions
    }
}

/// A remaining leg as the complete-or-unwind math needs it.
#[derive(Debug, Clone)]
pub struct RemainingLeg {
    pub market: MarketId,
    pub side: Side,
    pub action: Action,
    pub remaining: Contracts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompleteOrUnwind {
    /// Taker-completing still clears the floor: estimated all-in cost given.
    TakerComplete { est_cost: Cents, net_edge_bps: i64 },
    /// Completion economics are gone: unwind the filled legs.
    Unwind { reason: String },
}

/// The deterministic complete-or-unwind decision (spec 5.4): walk current
/// books for the remaining legs at taker, all-in with taker fees; complete
/// iff sets_value - cost clears the policy floor. Insufficient depth or a
/// missing book means completion cannot even be priced: unwind.
pub fn decide_complete_or_unwind(
    remaining: &[RemainingLeg],
    books: &BTreeMap<MarketId, OrderBook>,
    fees: &dyn FeeModel,
    policy: &GroupPolicy,
    at: UtcTimestamp,
) -> CompleteOrUnwind {
    let mut total_cost = Cents::ZERO;
    let mut sets = i64::MAX;
    for leg in remaining {
        if leg.remaining.raw() == 0 {
            continue;
        }
        sets = sets.min(leg.remaining.raw());
        let Some(book) = books.get(&leg.market) else {
            return CompleteOrUnwind::Unwind {
                reason: format!("no book for {}: cannot price completion", leg.market),
            };
        };
        match walk_taker_cost(book, leg, fees, at) {
            Ok(cost) => {
                total_cost = match total_cost.checked_add(cost) {
                    Ok(c) => c,
                    Err(e) => {
                        return CompleteOrUnwind::Unwind {
                            reason: format!("cost arithmetic failed: {e}"),
                        }
                    }
                };
            }
            Err(reason) => return CompleteOrUnwind::Unwind { reason },
        }
    }
    if sets == i64::MAX {
        sets = 0;
    }
    let value = match policy.value_per_set.checked_mul(sets) {
        Ok(v) => v,
        Err(e) => {
            return CompleteOrUnwind::Unwind {
                reason: format!("value arithmetic failed: {e}"),
            }
        }
    };
    let net = value.raw() - total_cost.raw();
    if total_cost.raw() <= 0 {
        return CompleteOrUnwind::Unwind {
            reason: "zero completion cost: nothing to complete".into(),
        };
    }
    let net_bps = (i128::from(net) * 10_000).div_euclid(i128::from(total_cost.raw()));
    if net >= 0 && net_bps >= i128::from(policy.min_completion_edge_bps) {
        CompleteOrUnwind::TakerComplete {
            est_cost: total_cost,
            net_edge_bps: net_bps as i64,
        }
    } else {
        CompleteOrUnwind::Unwind {
            reason: format!(
                "completion edge {net_bps} bps below floor {} bps (cost {total_cost}, value {value})",
                policy.min_completion_edge_bps
            ),
        }
    }
}

/// All-in taker cost (gross + taker fee) to fill a leg by walking visible
/// depth. Errors when depth is insufficient (cannot complete honestly).
fn walk_taker_cost(
    book: &OrderBook,
    leg: &RemainingLeg,
    fees: &dyn FeeModel,
    at: UtcTimestamp,
) -> Result<Cents, String> {
    let mut remaining = leg.remaining.raw();
    let mut cost = Cents::ZERO;
    // Buying YES walks yes asks; buying NO walks mirrored yes bids.
    let levels: Vec<(Cents, i64)> = match (leg.side, leg.action) {
        (Side::Yes, Action::Buy) => book
            .yes_asks
            .iter()
            .map(|l| (l.price, l.qty.raw()))
            .collect(),
        (Side::No, Action::Buy) => book
            .yes_bids
            .iter()
            .filter_map(|l| {
                Cents::new(100)
                    .checked_sub(l.price)
                    .ok()
                    .map(|p| (p, l.qty.raw()))
            })
            .collect(),
        // Completion legs are buys in v1 (selling legs unwind instead).
        _ => return Err("completion pricing supports buy legs only (v1)".into()),
    };
    for (price, qty) in levels {
        if remaining == 0 {
            break;
        }
        let take = remaining.min(qty);
        let gross = price
            .checked_mul(take)
            .map_err(|e| format!("walk arithmetic failed: {e}"))?;
        let fee = fees
            .fee(FillRole::Taker, price, Contracts::new(take), None, at)
            .map_err(|e| format!("fee model failed: {e}"))?;
        cost = cost
            .checked_add(gross)
            .and_then(|c| c.checked_add(fee.max(Cents::ZERO)))
            .map_err(|e| format!("walk arithmetic failed: {e}"))?;
        remaining -= take;
    }
    if remaining > 0 {
        return Err(format!(
            "insufficient depth on {}: {} contracts unfillable",
            leg.market, remaining
        ));
    }
    Ok(cost)
}
