//! The shared sizing library (spec 5.14, 5.9, 13.1: one sizing library,
//! per-strategy comparators).
//!
//! Sizing is NEVER the model's job. Inputs are deterministic: envelope
//! headroom (via the reservation ledger), edge/probability, gate caps.
//! Phase 0 ships the two primitives the launch strategies need; the
//! calibration haircut (5.10) wraps `kelly_binary` in Phase 2.

use crate::StateError;
use fortuna_core::money::Cents;

/// How many whole "sets" (or contracts) fit in the envelope headroom at a
/// given all-in cost, bounded by an absolute cap. Floor division: never
/// oversize. Zero/negative inputs size zero (refusing beats erroring here:
/// a strategy with no headroom simply does not trade).
pub fn affordable_sets(headroom: Cents, cost_per_set: Cents, cap_sets: i64) -> i64 {
    if headroom.raw() <= 0 || cost_per_set.raw() <= 0 || cap_sets <= 0 {
        return 0;
    }
    (headroom.raw() / cost_per_set.raw()).min(cap_sets)
}

/// Fractional Kelly for a $1-payout binary contract bought at `price` with
/// calibrated probability `p`: full Kelly f* = (100p - price) / (100 -
/// price), clamped to [0, 1], scaled by `fraction` (default 0.25 per
/// config). Returns the fraction of the envelope to commit.
///
/// Probabilities are f64 (cognition-side); the OUTPUT is only ever used to
/// scale integer-cent envelopes via `kelly_contracts`, where flooring keeps
/// money integral and conservative.
pub fn kelly_binary(p: f64, price: Cents, fraction: f64) -> Result<f64, StateError> {
    if !(0.0..=1.0).contains(&p) || !p.is_finite() {
        return Err(StateError::Arithmetic {
            op: "kelly probability out of [0, 1]",
        });
    }
    if !(0.0..=1.0).contains(&fraction) || !fraction.is_finite() {
        return Err(StateError::Arithmetic {
            op: "kelly fraction out of [0, 1]",
        });
    }
    let c = price.raw();
    if !(1..=99).contains(&c) {
        return Err(StateError::Arithmetic {
            op: "kelly price outside [1, 99] cents",
        });
    }
    let edge = p * 100.0 - c as f64;
    let full = edge / (100.0 - c as f64);
    Ok(full.clamp(0.0, 1.0) * fraction)
}

/// Contracts to buy: kelly fraction of the envelope headroom at the given
/// all-in cost per contract, floored, capped. The integer-money boundary:
/// the f64 fraction converts ONCE to parts-per-million (floored), and the
/// budget is computed in widened integer arithmetic — money never rides
/// in a float. Double flooring (ppm, then division) is conservative.
pub fn kelly_contracts(
    p: f64,
    price: Cents,
    fraction: f64,
    headroom: Cents,
    all_in_cost_per_contract: Cents,
    cap_contracts: i64,
) -> Result<i64, StateError> {
    let f = kelly_binary(p, price, fraction)?;
    if headroom.raw() <= 0 || all_in_cost_per_contract.raw() <= 0 || cap_contracts <= 0 {
        return Ok(0);
    }
    // f in [0,1] by construction; ppm in [0, 1_000_000].
    let f_ppm = (f * 1_000_000.0).floor() as i128;
    let budget_wide = i128::from(headroom.raw()) * f_ppm / 1_000_000;
    let budget = i64::try_from(budget_wide).unwrap_or(i64::MAX);
    Ok((budget / all_in_cost_per_contract.raw())
        .min(cap_contracts)
        .max(0))
}
