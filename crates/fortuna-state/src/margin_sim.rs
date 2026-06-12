//! Paper-engine margin semantics (spec 5.15, plan §B5; T5.B5).
//!
//! A deterministic, single-account perp margin simulator: signed position
//! updates with VWAP entries rounded against us by side, realized PnL
//! floored toward -inf, funding accruals on the venue's 04:00/12:00/20:00
//! UTC schedule (research §4, empirically confirmed), and liquidation
//! simulation from the recorded risk curves under portfolio margin.
//!
//! Doctrine: "a liquidation under-modeled = test failure, not surprise" —
//! a held notional beyond the last risk-curve tier, or a held position
//! with no mark, is an ERROR, never a guess. Liquidation closes the WHOLE
//! account (portfolio margin: one position's excursion consumes the
//! account) at the worse-for-us mark plus a configured penalty against us;
//! post-liquidation balances may be negative (clawback risk is modeled,
//! not clamped).
//!
//! This engine lives in fortuna-state (track-C margin ownership); the
//! fortuna-paper wiring that drives it from recorded streams belongs to
//! the paper crate's owner and is ledgered in GAPS.

use crate::StateError;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, Contracts, MarketId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{FundingAccrual, MarginAccountView, PerpMarks, PerpPosition, PerpPrice};
use rust_decimal::Decimal;
use std::collections::BTreeMap;

const MILLIS_PER_HOUR: i64 = 3_600_000;
const MILLIS_PER_UTC_DAY: i64 = 86_400_000;
/// Funding times: 04:00, 12:00, 20:00 UTC (research §4, confirmed live).
const FUNDING_OFFSETS_MS: [i64; 3] = [
    4 * MILLIS_PER_HOUR,
    12 * MILLIS_PER_HOUR,
    20 * MILLIS_PER_HOUR,
];

/// Maintenance-margin risk curve (recorded venue leverage_estimates by
/// notional): ascending (max_notional, mm_bps) tiers. A notional beyond
/// the last tier cannot be bounded.
#[derive(Debug, Clone)]
pub struct RiskCurve {
    pub tiers: Vec<(Cents, i64)>,
}

impl RiskCurve {
    fn validate(&self, market: &MarketId) -> Result<(), StateError> {
        if self.tiers.is_empty() {
            return Err(StateError::MarginSim {
                scope: market.to_string(),
                reason: "empty risk curve: no curve, no bound, no positions".into(),
            });
        }
        let mut prev = 0i64;
        for (threshold, bps) in &self.tiers {
            if threshold.raw() <= prev {
                return Err(StateError::MarginSim {
                    scope: market.to_string(),
                    reason: "risk-curve thresholds must be positive and strictly ascending".into(),
                });
            }
            if !(1..=10_000).contains(bps) {
                return Err(StateError::MarginSim {
                    scope: market.to_string(),
                    reason: "risk-curve bps must be in [1, 10000]".into(),
                });
            }
            prev = threshold.raw();
        }
        Ok(())
    }

    fn mm_bps(&self, notional: Cents) -> Option<i64> {
        self.tiers
            .iter()
            .find(|(threshold, _)| notional <= *threshold)
            .map(|(_, bps)| *bps)
    }
}

/// Margin-sim parameters. The multiplier should match the gate config so
/// the paper world is at least as strict as the gates assume.
#[derive(Debug, Clone)]
pub struct MarginSimConfig {
    /// Percent multiplier on the approximated maintenance margin
    /// (>= 100; below would weaken the venue's own requirement).
    pub mm_multiplier_pct: i64,
    /// Liquidation close slippage in bps of the mark, applied AGAINST us
    /// (long closes below the mark, short closes above). >= 0.
    pub liquidation_penalty_bps: i64,
    /// Per-market risk curves; a market without a curve cannot be traded.
    pub curves: BTreeMap<MarketId, RiskCurve>,
}

/// One fill's effect on the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FillOutcome {
    /// Realized PnL on the reduced/flipped part, floored toward -inf.
    pub realized_pnl: Cents,
}

/// A simulated venue liquidation (spec 5.15: order_source=system fills).
/// The caller owes the mandatory alert + halt evaluation; the sim reports.
#[derive(Debug, Clone)]
pub struct Liquidation {
    /// Positions as they stood at the moment of liquidation.
    pub closed: Vec<(MarketId, PerpPosition)>,
    pub balance_after: Cents,
}

/// Deterministic single-account perp margin simulator.
#[derive(Debug, Clone)]
pub struct MarginSim {
    balance: Cents,
    positions: BTreeMap<MarketId, PerpPosition>,
    funding_log: Vec<FundingAccrual>,
    config: MarginSimConfig,
}

impl MarginSim {
    pub fn new(initial_balance: Cents, config: MarginSimConfig) -> Result<MarginSim, StateError> {
        if config.mm_multiplier_pct < 100 {
            return Err(StateError::MarginSim {
                scope: "account".into(),
                reason: "mm_multiplier_pct must be >= 100".into(),
            });
        }
        if config.liquidation_penalty_bps < 0 {
            return Err(StateError::MarginSim {
                scope: "account".into(),
                reason: "liquidation penalty must be non-negative".into(),
            });
        }
        for (market, curve) in &config.curves {
            curve.validate(market)?;
        }
        Ok(MarginSim {
            balance: initial_balance,
            positions: BTreeMap::new(),
            funding_log: Vec::new(),
            config,
        })
    }

    pub fn balance(&self) -> Cents {
        self.balance
    }

    pub fn position(&self, market: &MarketId) -> Option<&PerpPosition> {
        self.positions.get(market)
    }

    /// Append-only funding accrual log (mirrors the venue funding_history:
    /// entries exist only for held positions).
    pub fn funding_log(&self) -> &[FundingAccrual] {
        &self.funding_log
    }

    /// Apply one fill. VWAP entries round against us by side (long entries
    /// ceil, short entries floor); realized PnL on reduces/flips floors
    /// toward -inf; the fee is debited from margin cash.
    pub fn apply_fill(
        &mut self,
        market: &MarketId,
        action: Action,
        qty: Contracts,
        price: PerpPrice,
        fee: Cents,
    ) -> Result<FillOutcome, StateError> {
        let q = qty.raw();
        if q <= 0 {
            return Err(StateError::MarginSim {
                scope: market.to_string(),
                reason: format!("fill quantity {q} must be positive"),
            });
        }
        if fee < Cents::ZERO {
            return Err(StateError::MarginSim {
                scope: market.to_string(),
                reason: "negative fee".into(),
            });
        }
        // Fail-closed: a market without a risk curve cannot be margin-
        // checked later, so it cannot be traded now.
        if !self.config.curves.contains_key(market) {
            return Err(StateError::MarginSim {
                scope: market.to_string(),
                reason: "no risk curve configured for this market: fail-closed".into(),
            });
        }
        let delta: i64 = match action {
            Action::Buy => q,
            Action::Sell => -q,
        };
        let (pos_qty, entry) = match self.positions.get(market) {
            Some(p) => (p.qty.raw(), p.avg_entry.raw()),
            None => (0, 0),
        };

        let adding = pos_qty == 0 || (pos_qty > 0) == (delta > 0);
        let realized = if adding {
            let new_qty = pos_qty
                .checked_add(delta)
                .ok_or(StateError::Arithmetic { op: "fill add" })?;
            // VWAP in i128; round against us by the NEW position's side.
            let total_tt = i128::from(entry) * i128::from(pos_qty.unsigned_abs())
                + i128::from(price.raw()) * i128::from(q);
            let denom = i128::from(pos_qty.unsigned_abs()) + i128::from(q);
            let vwap = if new_qty > 0 {
                ceil_div(total_tt, denom)
            } else {
                total_tt.div_euclid(denom)
            };
            let avg_entry = i64::try_from(vwap).map_err(|_| StateError::Arithmetic {
                op: "vwap entry out of range",
            })?;
            self.positions.insert(
                market.clone(),
                PerpPosition {
                    market: market.clone(),
                    qty: Contracts::new(new_qty),
                    avg_entry: PerpPrice::new(avg_entry),
                },
            );
            Cents::ZERO
        } else {
            let abs_pos = i128::from(pos_qty.unsigned_abs());
            let closing = i128::from(q).min(abs_pos);
            // Long realizes (price - entry); short realizes (entry - price).
            let per_contract = if pos_qty > 0 {
                i128::from(price.raw()) - i128::from(entry)
            } else {
                i128::from(entry) - i128::from(price.raw())
            };
            let realized_tt = per_contract * closing;
            let realized = cents_floor_from_tt(realized_tt)?;
            let new_qty = pos_qty
                .checked_add(delta)
                .ok_or(StateError::Arithmetic { op: "fill reduce" })?;
            if new_qty == 0 {
                self.positions.remove(market);
            } else if (new_qty > 0) == (pos_qty > 0) {
                // Plain reduce: entry unchanged.
                self.positions.insert(
                    market.clone(),
                    PerpPosition {
                        market: market.clone(),
                        qty: Contracts::new(new_qty),
                        avg_entry: PerpPrice::new(entry),
                    },
                );
            } else {
                // Flip: the remainder opens at the fill price.
                self.positions.insert(
                    market.clone(),
                    PerpPosition {
                        market: market.clone(),
                        qty: Contracts::new(new_qty),
                        avg_entry: price,
                    },
                );
            }
            realized
        };

        self.balance = self
            .balance
            .checked_add(realized)
            .and_then(|b| b.checked_sub(fee))
            .map_err(StateError::Money)?;
        Ok(FillOutcome {
            realized_pnl: realized,
        })
    }

    /// Apply one funding tick for one market. Flat positions accrue
    /// nothing and log nothing (the venue's funding_history has entries
    /// only for held positions). The accrual amount is computed by
    /// `FundingAccrual::accrue` (floored against us) and applied to the
    /// balance; the record appends to the log.
    pub fn apply_funding(
        &mut self,
        market: &MarketId,
        rate: Decimal,
        settlement_mark: PerpPrice,
        funding_time: UtcTimestamp,
    ) -> Result<Option<FundingAccrual>, StateError> {
        let Some(position) = self.positions.get(market) else {
            return Ok(None);
        };
        if position.qty.raw() == 0 {
            return Ok(None);
        }
        let accrual = FundingAccrual::accrue(
            market.clone(),
            funding_time,
            rate,
            settlement_mark,
            position.qty,
        )
        .map_err(|e| StateError::MarginSim {
            scope: market.to_string(),
            reason: format!("funding accrual failed: {e}"),
        })?;
        self.balance = self
            .balance
            .checked_add(accrual.amount)
            .map_err(StateError::Money)?;
        self.funding_log.push(accrual.clone());
        Ok(Some(accrual))
    }

    /// The conservative account view at the given marks. Every held
    /// position must have a mark; a missing mark is an error, not a guess.
    pub fn account_view(
        &self,
        marks: &BTreeMap<MarketId, PerpMarks>,
        pending_funding: Cents,
    ) -> Result<MarginAccountView, StateError> {
        let pairs = self.marked_positions(marks)?;
        MarginAccountView::compute(self.balance, &pairs, pending_funding).map_err(|e| {
            StateError::MarginSim {
                scope: "account".into(),
                reason: format!("account view failed: {e}"),
            }
        })
    }

    /// Evaluate the liquidation condition: account equity (conservative)
    /// below the summed per-market maintenance requirement (risk curve x
    /// multiplier) liquidates the WHOLE account — every position closes at
    /// its worse-for-us mark plus the penalty, balance absorbs the losses
    /// (possibly negative), and the event reports what was closed. The
    /// caller owes the spec 5.15 mandatory alert + halt evaluation.
    pub fn check_liquidation(
        &mut self,
        marks: &BTreeMap<MarketId, PerpMarks>,
        pending_funding: Cents,
    ) -> Result<Option<Liquidation>, StateError> {
        let view = self.account_view(marks, pending_funding)?;

        // Summed maintenance requirement at worse-for-us notional marks.
        let mut required = Cents::ZERO;
        for (market, position) in &self.positions {
            let pair = marks.get(market).ok_or_else(|| StateError::MarginSim {
                scope: market.to_string(),
                reason: "no mark for held position".into(),
            })?;
            let notional_mark = worse_notional_mark(pair);
            let notional =
                position
                    .notional_at(notional_mark)
                    .map_err(|e| StateError::MarginSim {
                        scope: market.to_string(),
                        reason: format!("notional failed: {e}"),
                    })?;
            let curve = self
                .config
                .curves
                .get(market)
                .ok_or_else(|| StateError::MarginSim {
                    scope: market.to_string(),
                    reason: "no risk curve configured for held position".into(),
                })?;
            let mm_bps = curve
                .mm_bps(notional)
                .ok_or_else(|| StateError::MarginSim {
                    scope: market.to_string(),
                    reason: format!(
                        "notional {notional} exceeds the last risk-curve tier: liquidation \
                     cannot be modeled (under-modeled = failure, not surprise)"
                    ),
                })?;
            let mm = ceil_div(i128::from(notional.raw()) * i128::from(mm_bps), 10_000);
            let mm_required = ceil_div(mm * i128::from(self.config.mm_multiplier_pct), 100);
            let mm_required = i64::try_from(mm_required).map_err(|_| StateError::Arithmetic {
                op: "maintenance requirement out of range",
            })?;
            required = required
                .checked_add(Cents::new(mm_required))
                .map_err(StateError::Money)?;
        }

        if view.equity >= required {
            return Ok(None);
        }

        // Liquidation: close everything at worse-for-us mark +- penalty.
        let closed: Vec<(MarketId, PerpPosition)> = self
            .positions
            .iter()
            .map(|(m, p)| (m.clone(), p.clone()))
            .collect();
        for (market, position) in &closed {
            let pair = marks.get(market).ok_or_else(|| StateError::MarginSim {
                scope: market.to_string(),
                reason: "no mark for held position".into(),
            })?;
            let close_price =
                liquidation_close_price(position, pair, self.config.liquidation_penalty_bps)?;
            let abs_qty = i128::from(position.qty.raw().unsigned_abs());
            let per_contract = if position.qty.raw() > 0 {
                i128::from(close_price) - i128::from(position.avg_entry.raw())
            } else {
                i128::from(position.avg_entry.raw()) - i128::from(close_price)
            };
            let realized = cents_floor_from_tt(per_contract * abs_qty)?;
            self.balance = self
                .balance
                .checked_add(realized)
                .map_err(StateError::Money)?;
        }
        self.positions.clear();
        Ok(Some(Liquidation {
            closed,
            balance_after: self.balance,
        }))
    }

    fn marked_positions(
        &self,
        marks: &BTreeMap<MarketId, PerpMarks>,
    ) -> Result<Vec<(PerpPosition, PerpMarks)>, StateError> {
        self.positions
            .iter()
            .map(|(market, position)| {
                marks
                    .get(market)
                    .map(|pair| (position.clone(), *pair))
                    .ok_or_else(|| StateError::MarginSim {
                        scope: market.to_string(),
                        reason: "no mark for held position".into(),
                    })
            })
            .collect()
    }
}

/// Funding times t on the venue schedule with `after < t <= until`
/// (research §4: 04:00 / 12:00 / 20:00 UTC, 8h apart). An exact tick at
/// `after` is already processed; an exact tick at `until` is due.
pub fn funding_times_between(
    after: UtcTimestamp,
    until: UtcTimestamp,
) -> Result<Vec<UtcTimestamp>, StateError> {
    let mut times = Vec::new();
    if until <= after {
        return Ok(times);
    }
    let first_day = after.epoch_millis().div_euclid(MILLIS_PER_UTC_DAY);
    let last_day = until.epoch_millis().div_euclid(MILLIS_PER_UTC_DAY);
    for day in first_day..=last_day {
        for offset in FUNDING_OFFSETS_MS {
            let t_ms = day
                .checked_mul(MILLIS_PER_UTC_DAY)
                .and_then(|d| d.checked_add(offset))
                .ok_or(StateError::Arithmetic {
                    op: "funding time overflow",
                })?;
            if t_ms > after.epoch_millis() && t_ms <= until.epoch_millis() {
                let t =
                    UtcTimestamp::from_epoch_millis(t_ms).map_err(|_| StateError::Arithmetic {
                        op: "funding time out of range",
                    })?;
                times.push(t);
            }
        }
    }
    Ok(times)
}

/// The notional-valuation mark: the HIGHER of the two (a bigger notional
/// means a bigger maintenance requirement — conservative).
fn worse_notional_mark(pair: &PerpMarks) -> PerpPrice {
    match pair.conservative {
        Some(ours) => ours.max(pair.venue_settlement),
        None => pair.venue_settlement,
    }
}

/// Liquidation close price: the worse-for-us mark (long: lower of the two;
/// short: higher), pushed FURTHER against us by the penalty (ceiled).
fn liquidation_close_price(
    position: &PerpPosition,
    pair: &PerpMarks,
    penalty_bps: i64,
) -> Result<i64, StateError> {
    let venue = pair.venue_settlement.raw();
    let mark = match pair.conservative {
        Some(ours) => {
            if position.qty.raw() > 0 {
                ours.raw().min(venue)
            } else {
                ours.raw().max(venue)
            }
        }
        None => venue,
    };
    let penalty = ceil_div(i128::from(mark) * i128::from(penalty_bps), 10_000);
    let close = if position.qty.raw() > 0 {
        i128::from(mark) - penalty
    } else {
        i128::from(mark) + penalty
    };
    i64::try_from(close).map_err(|_| StateError::Arithmetic {
        op: "liquidation close price out of range",
    })
}

/// ceil(n / d) toward +inf for d > 0.
fn ceil_div(n: i128, d: i128) -> i128 {
    let q = n.div_euclid(d);
    if n.rem_euclid(d) != 0 {
        q + 1
    } else {
        q
    }
}

/// Ten-thousandths to cents, floored toward -inf (gains never overstated).
fn cents_floor_from_tt(tt: i128) -> Result<Cents, StateError> {
    i64::try_from(tt.div_euclid(100))
        .map(Cents::new)
        .map_err(|_| StateError::Arithmetic {
            op: "realized pnl out of range",
        })
}
