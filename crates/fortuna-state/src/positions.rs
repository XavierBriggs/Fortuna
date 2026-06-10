//! Per-market positions folded from already-deduped fills. Spec 5.13, 5.14.
//!
//! Fill dedup is fortuna-exec's job (fills are deduped by `fill_id` before
//! they reach this book); this fold assumes each fill is applied exactly
//! once.
//!
//! # Per-side lots (the pair-value rule)
//!
//! YES and NO are tracked as SEPARATE lots and never net against each other:
//! a held YES+NO pair pays exactly $1 at settlement regardless of outcome,
//! so netting would silently destroy real value (the sum-arb strategies
//! exist precisely to collect that value). `net_yes()` exists as an
//! EXPOSURE view only (event-direction risk), never as a valuation.
//!
//! # Accounting model (per lot)
//!
//! - Buys increase the lot: `cost_basis += price x qty` (cash out).
//! - Sells reduce the lot (venues are close-only; spec T0.3): proceeds
//!   `price x qty` realize against the proportional share of the standing
//!   basis. Selling more than the lot holds is `OverClose` — a discrepancy
//!   (venue would have rejected it), never a silent flip.
//!
//! # Rounding direction
//!
//! The proportional basis closed is `floor(cost_basis * closed_qty /
//! held_qty)` using FLOOR division (`div_euclid`, toward negative infinity —
//! NOT Rust's truncating `/`). Sub-cent dust stays in the open basis and is
//! realized exactly on the final close (`closed == held` divides exactly),
//! so per-lot conservation holds at all times:
//! `cost_basis == sum(buy gross) - sum(sell gross) + realized_from_this_lot`.
//!
//! # Fees
//!
//! `fees_paid` accumulates `Fill::fee` separately (rebates may be negative);
//! fees never touch basis or realized PnL.
//!
//! # Lifecycle, settlement, void
//!
//! Lifecycle (spec 5.13 projection): `Open`, `ResolutionPending`,
//! `Disputed`; the account view excludes non-Open positions from bankroll
//! while reporting their worst-case exposure separately.
//!
//! `apply_settlement(winner, payout)` realizes BOTH lots: the winning lot at
//! `payout x qty`, the losing lot at zero (its basis is the lost premium);
//! both zero out, lifecycle resets to `Open`, and the venue-owed payout
//! (winning lot only) is returned. `apply_void_refund` returns
//! `yes.cost_basis + no.cost_basis` (the venue refunds what was paid) and
//! zeroes both lots; realized PnL is untouched by voids (spec 5.13: a voided
//! market is the world breaking the question).
//!
//! Entries are retained (zeroed) after closes/settlements as the per-market
//! realized-PnL and fee accumulators.

use crate::StateError;
use fortuna_core::market::{notional, Action, MarketId, Side};
use fortuna_core::money::Cents;
use fortuna_venues::Fill;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Position lifecycle relevant to capital accounting (spec 5.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionLifecycle {
    /// Tradeable; marks count toward floating bankroll.
    Open,
    /// Underlying occurred / market expired; awaiting venue determination.
    /// Out of bankroll, in exposure at worst case.
    ResolutionPending,
    /// Frozen pending dispute resolution. Out of bankroll, in exposure at
    /// worst case.
    Disputed,
}

/// One side's holding: non-negative quantity plus the basis pricing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Lot {
    pub qty: i64,
    pub cost_basis: Cents,
}

/// One market's folded position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub market: MarketId,
    pub yes: Lot,
    pub no: Lot,
    /// Cumulative realized PnL from reductions and settlements (not voids).
    pub realized_pnl: Cents,
    /// Cumulative fees (negative entries are rebates). Never in basis/PnL.
    pub fees_paid: Cents,
    pub lifecycle: PositionLifecycle,
}

impl Position {
    fn flat(market: MarketId) -> Position {
        Position {
            market,
            yes: Lot::default(),
            no: Lot::default(),
            realized_pnl: Cents::ZERO,
            fees_paid: Cents::ZERO,
            lifecycle: PositionLifecycle::Open,
        }
    }

    /// Event-direction EXPOSURE view (gate inputs), never a valuation:
    /// the pair component (min of the lots) carries no direction risk.
    pub fn net_yes(&self) -> i64 {
        self.yes.qty - self.no.qty
    }

    pub fn is_flat(&self) -> bool {
        self.yes.qty == 0 && self.no.qty == 0
    }
}

/// `floor(basis * closed / held)`: proportional share of the standing basis
/// attributed to the closed quantity. Floor = toward negative infinity
/// (`div_euclid`); exact when `closed == held`. i128 intermediate so the
/// multiply cannot overflow.
fn floor_pro_rata(basis: Cents, closed: u64, held: u64) -> Result<Cents, StateError> {
    if held == 0 || closed > held {
        return Err(StateError::Arithmetic {
            op: "pro-rata basis (bad quantities)",
        });
    }
    let num = i128::from(basis.raw())
        .checked_mul(i128::from(closed))
        .ok_or(StateError::Arithmetic {
            op: "pro-rata basis multiply",
        })?;
    let quotient = num.div_euclid(i128::from(held));
    i64::try_from(quotient)
        .map(Cents::new)
        .map_err(|_| StateError::Arithmetic {
            op: "pro-rata basis quotient",
        })
}

/// All positions, keyed by market. Deterministic iteration (BTreeMap).
#[derive(Debug, Clone, Default)]
pub struct PositionBook {
    positions: BTreeMap<MarketId, Position>,
}

impl PositionBook {
    pub fn new() -> PositionBook {
        PositionBook::default()
    }

    pub fn position(&self, market: &MarketId) -> Option<&Position> {
        self.positions.get(market)
    }

    /// All tracked positions in market order (including zeroed entries,
    /// retained as per-market PnL/fee accumulators).
    pub fn positions(&self) -> impl Iterator<Item = &Position> {
        self.positions.values()
    }

    /// Fold one (already-deduped) fill into the book. Atomic: on error the
    /// book is unchanged.
    pub fn apply_fill(&mut self, fill: &Fill) -> Result<(), StateError> {
        let qty = fill.qty.raw();
        if qty <= 0 {
            return Err(StateError::InvalidFill {
                fill_id: fill.fill_id.clone(),
                reason: "non-positive quantity",
            });
        }
        if fill.price.raw() < 0 {
            return Err(StateError::InvalidFill {
                fill_id: fill.fill_id.clone(),
                reason: "negative price",
            });
        }

        let mut next = self
            .positions
            .get(&fill.market)
            .cloned()
            .unwrap_or_else(|| Position::flat(fill.market.clone()));

        next.fees_paid = next
            .fees_paid
            .checked_add(fill.fee)
            .map_err(StateError::Money)?;

        let gross = notional(fill.price, fill.qty).map_err(StateError::Money)?;
        let lot = match fill.side {
            Side::Yes => &mut next.yes,
            Side::No => &mut next.no,
        };

        match fill.action {
            Action::Buy => {
                lot.cost_basis = lot
                    .cost_basis
                    .checked_add(gross)
                    .map_err(StateError::Money)?;
                lot.qty = lot.qty.checked_add(qty).ok_or(StateError::Arithmetic {
                    op: "lot quantity add",
                })?;
            }
            Action::Sell => {
                if qty > lot.qty {
                    return Err(StateError::OverClose {
                        market: fill.market.clone(),
                        side: fill.side,
                        held: lot.qty,
                        closing: qty,
                    });
                }
                let basis_closed =
                    floor_pro_rata(lot.cost_basis, qty.unsigned_abs(), lot.qty.unsigned_abs())?;
                let pnl = gross.checked_sub(basis_closed).map_err(StateError::Money)?;
                lot.cost_basis = lot
                    .cost_basis
                    .checked_sub(basis_closed)
                    .map_err(StateError::Money)?;
                lot.qty -= qty;
                next.realized_pnl = next
                    .realized_pnl
                    .checked_add(pnl)
                    .map_err(StateError::Money)?;
            }
        }

        self.positions.insert(fill.market.clone(), next);
        Ok(())
    }

    /// Set the capital-accounting lifecycle for a tracked position.
    pub fn set_lifecycle(
        &mut self,
        market: &MarketId,
        lifecycle: PositionLifecycle,
    ) -> Result<(), StateError> {
        let pos = self
            .positions
            .get_mut(market)
            .ok_or_else(|| StateError::UnknownMarket {
                market: market.clone(),
            })?;
        pos.lifecycle = lifecycle;
        Ok(())
    }

    /// Settle a determined market: the winning lot realizes
    /// `payout_per_contract x qty` (returned: the venue owes it), the losing
    /// lot realizes zero against its basis; both lots clear; lifecycle
    /// resets. Settling an untracked market is an error (discrepancy
    /// territory, spec 5.13 — never a silent no-op).
    pub fn apply_settlement(
        &mut self,
        market: &MarketId,
        winner: Side,
        payout_per_contract: Cents,
    ) -> Result<Cents, StateError> {
        let pos = self
            .positions
            .get_mut(market)
            .ok_or_else(|| StateError::UnknownMarket {
                market: market.clone(),
            })?;
        let (winning, losing) = match winner {
            Side::Yes => (&pos.yes, &pos.no),
            Side::No => (&pos.no, &pos.yes),
        };
        let payout = payout_per_contract
            .checked_mul(winning.qty)
            .map_err(StateError::Money)?;
        // Winner: payout - basis. Loser: 0 - basis.
        let pnl = payout
            .checked_sub(winning.cost_basis)
            .and_then(|p| p.checked_sub(losing.cost_basis))
            .map_err(StateError::Money)?;
        pos.realized_pnl = pos
            .realized_pnl
            .checked_add(pnl)
            .map_err(StateError::Money)?;
        pos.yes = Lot::default();
        pos.no = Lot::default();
        pos.lifecycle = PositionLifecycle::Open;
        Ok(payout)
    }

    /// Point-in-time lot snapshot for settlement reversal (spec 5.13:
    /// settled -> reversed -> re-settled). Captured BEFORE
    /// `apply_settlement`; `reverse_settlement` restores it exactly.
    pub fn settlement_snapshot(
        &self,
        market: &MarketId,
    ) -> Result<crate::SettlementSnapshot, StateError> {
        let pos = self
            .positions
            .get(market)
            .ok_or_else(|| StateError::UnknownMarket {
                market: market.clone(),
            })?;
        Ok(crate::SettlementSnapshot {
            yes: pos.yes,
            no: pos.no,
            realized_pnl_before: pos.realized_pnl,
        })
    }

    /// Venue settlement correction (spec 5.13: reversals are new entries,
    /// never edits — at the POSITION level the reversal restores the exact
    /// pre-settlement lots and realized PnL so the corrected re-settlement
    /// replays cleanly). Returns the clawback owed back to the venue
    /// (= the payout the reversed settlement had credited). Refuses if the
    /// market is unknown or its lots are not the post-settlement zeroes
    /// (a repopulated market cannot be reversed into).
    pub fn reverse_settlement(
        &mut self,
        market: &MarketId,
        snapshot: &crate::SettlementSnapshot,
    ) -> Result<Cents, StateError> {
        let pos = self
            .positions
            .get_mut(market)
            .ok_or_else(|| StateError::UnknownMarket {
                market: market.clone(),
            })?;
        if pos.yes.qty != 0 || pos.no.qty != 0 {
            return Err(StateError::IllegalReversal {
                market: market.clone(),
                reason: "live lots present; only a settled (zeroed) market reverses",
            });
        }
        // realized delta the settlement applied = current - before.
        let delta = pos
            .realized_pnl
            .checked_sub(snapshot.realized_pnl_before)
            .map_err(StateError::Money)?;
        // The venue claws back what it paid: payout = delta + total basis
        // (settlement pnl = payout - basis ==> payout = pnl + basis).
        let basis_total = snapshot
            .yes
            .cost_basis
            .checked_add(snapshot.no.cost_basis)
            .map_err(StateError::Money)?;
        let clawback = delta.checked_add(basis_total).map_err(StateError::Money)?;
        pos.yes = snapshot.yes;
        pos.no = snapshot.no;
        pos.realized_pnl = snapshot.realized_pnl_before;
        pos.lifecycle = PositionLifecycle::ResolutionPending;
        Ok(clawback)
    }

    /// Void/refund path: both lots clear and the refund (= total cost basis
    /// across both lots) is returned. Realized PnL is untouched: a voided
    /// market is the world breaking the question, not a trading outcome
    /// (spec 5.13).
    pub fn apply_void_refund(&mut self, market: &MarketId) -> Result<Cents, StateError> {
        let pos = self
            .positions
            .get_mut(market)
            .ok_or_else(|| StateError::UnknownMarket {
                market: market.clone(),
            })?;
        let refund = pos
            .yes
            .cost_basis
            .checked_add(pos.no.cost_basis)
            .map_err(StateError::Money)?;
        pos.yes = Lot::default();
        pos.no = Lot::default();
        pos.lifecycle = PositionLifecycle::Open;
        Ok(refund)
    }
}
