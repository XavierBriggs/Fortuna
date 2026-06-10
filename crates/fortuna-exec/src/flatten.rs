//! Flatten planner. Spec 5.4: any flatten request through the main runtime
//! first computes an estimated book-walk cost; default action is
//! freeze-and-cancel; flatten executes only with operator confirmation
//! displaying the estimate, or automatically within a configured bound.
//! Panic-flattening a thin book is a self-inflicted loss the system refuses
//! to take silently. The standalone kill switch is EXEMPT by design (I4):
//! its freeze-and-cancel/best-effort path lives in fortuna-ops and never
//! calls this planner.

use fortuna_core::book::{FeeModel, FillRole, OrderBook};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Contracts, MarketId};
use fortuna_core::money::Cents;
use std::collections::BTreeMap;

/// One position to flatten: `net_yes` > 0 sells YES into yes bids;
/// `net_yes` < 0 (long NO) sells NO into mirrored yes asks.
#[derive(Debug, Clone)]
pub struct FlattenLeg {
    pub market: MarketId,
    pub net_yes: i64,
    /// Contracts the visible book can absorb.
    pub fillable: i64,
    /// Contracts with no visible exit liquidity.
    pub unfillable: i64,
    /// Estimated proceeds net of taker fees for the fillable part.
    pub est_proceeds: Cents,
    /// Conservative mark value (touch price x quantity; 0 if no touch).
    pub mark_value: Cents,
}

#[derive(Debug, Clone)]
pub struct FlattenPlan {
    pub legs: Vec<FlattenLeg>,
    pub total_est_proceeds: Cents,
    pub total_mark_value: Cents,
    /// What flattening burns vs the conservative mark (slippage + fees).
    pub est_cost_vs_mark: Cents,
    pub any_unfillable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlattenDecision {
    /// Default: cancel resting orders, freeze, wait for a human.
    FreezeAndCancel { reason: String },
    /// Estimated cost within the configured bound: flatten may proceed
    /// automatically (orders still pass the gates).
    AutoFlatten,
    /// Operator must confirm with the estimate in front of them.
    NeedsOperatorConfirm,
}

impl FlattenPlan {
    /// Spec 5.4 decision rule.
    pub fn decide(&self, auto_bound: Cents) -> FlattenDecision {
        if self.any_unfillable {
            return FlattenDecision::FreezeAndCancel {
                reason: "book cannot absorb the position: freezing instead of panic-selling".into(),
            };
        }
        if self.est_cost_vs_mark <= auto_bound {
            FlattenDecision::AutoFlatten
        } else {
            FlattenDecision::NeedsOperatorConfirm
        }
    }
}

/// Walk visible depth to price an orderly exit of every position.
/// Deterministic; fees modeled at taker (flattening crosses the spread).
pub fn plan_flatten(
    positions: &[(MarketId, i64)],
    books: &BTreeMap<MarketId, OrderBook>,
    fees: &dyn FeeModel,
    at: UtcTimestamp,
) -> FlattenPlan {
    let mut legs = Vec::new();
    let mut total_proceeds = Cents::ZERO;
    let mut total_mark = Cents::ZERO;
    let mut any_unfillable = false;

    for (market, net_yes) in positions {
        if *net_yes == 0 {
            continue;
        }
        let book = books.get(market);
        // Exit levels in OUR side's price space: selling YES hits yes bids;
        // selling NO hits no bids (= mirrored yes asks).
        let levels: Vec<(Cents, i64)> = match book {
            None => Vec::new(),
            Some(b) => {
                if *net_yes > 0 {
                    b.yes_bids.iter().map(|l| (l.price, l.qty.raw())).collect()
                } else {
                    b.yes_asks
                        .iter()
                        .filter_map(|l| {
                            Cents::new(100)
                                .checked_sub(l.price)
                                .ok()
                                .map(|p| (p, l.qty.raw()))
                        })
                        .collect()
                }
            }
        };
        let mark_touch = levels.first().map(|(p, _)| *p).unwrap_or(Cents::ZERO);
        let mut remaining = net_yes.abs();
        let mark_value = mark_touch.checked_mul(remaining).unwrap_or(Cents::ZERO);

        let mut proceeds = Cents::ZERO;
        let mut filled = 0i64;
        for (price, qty) in levels {
            if remaining == 0 {
                break;
            }
            let take = remaining.min(qty);
            let gross = match price.checked_mul(take) {
                Ok(g) => g,
                Err(_) => break,
            };
            let fee = fees
                .fee(FillRole::Taker, price, Contracts::new(take), None, at)
                .unwrap_or(Cents::ZERO)
                .max(Cents::ZERO);
            proceeds = proceeds
                .checked_add(gross)
                .and_then(|p| p.checked_sub(fee))
                .unwrap_or(proceeds);
            filled += take;
            remaining -= take;
        }
        if remaining > 0 {
            any_unfillable = true;
        }
        total_proceeds = total_proceeds
            .checked_add(proceeds)
            .unwrap_or(total_proceeds);
        total_mark = total_mark.checked_add(mark_value).unwrap_or(total_mark);
        legs.push(FlattenLeg {
            market: market.clone(),
            net_yes: *net_yes,
            fillable: filled,
            unfillable: remaining,
            est_proceeds: proceeds,
            mark_value,
        });
    }

    let est_cost_vs_mark = total_mark
        .checked_sub(total_proceeds)
        .unwrap_or(Cents::ZERO);
    FlattenPlan {
        legs,
        total_est_proceeds: total_proceeds,
        total_mark_value: total_mark,
        est_cost_vs_mark,
        any_unfillable,
    }
}
