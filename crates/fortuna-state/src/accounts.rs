//! Account views (spec 5.14) with spec 5.13 exposure accounting.
//!
//! Four continuously reconciled numbers plus the limbo report:
//!
//! - `settled`: venue-confirmed cash (input; venue truth governs money).
//! - `committed` = resting order cost + active reservations.
//! - `floating` = pending settlements at guaranteed minimum + OPEN positions
//!   at conservative marks. Positions whose lifecycle is
//!   `ResolutionPending` or `Disputed` are EXCLUDED from floating - they are
//!   out of the bankroll until final - while their worst-case exposure is
//!   reported separately as `exposure_in_limbo` for gate inputs: reversal
//!   risk is real risk, and dropping it from exposure would free gate
//!   headroom to re-risk the same event (spec 5.13).
//! - `total` = settled + floating (checked).
//! - `deployable` = settled - committed (checked). MAY be negative when
//!   commitments exceed settled cash (e.g. resting orders against
//!   yet-unconfirmed cash); the view reports it as-is and the gates treat it
//!   as no headroom - masking it here would hide an over-commitment.
//!
//! This is a pure function of explicit inputs: no hidden state, no IO, no
//! clock. The caller assembles the inputs (marks via `marks::mark_position`,
//! reservations via `ReservationLedger`, etc.). All arithmetic is checked;
//! overflow propagates as an error.

use crate::{PositionLifecycle, StateError};
use fortuna_core::money::Cents;
use serde::{Deserialize, Serialize};

/// One position's contribution to the view, pre-valued by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionValuation {
    pub lifecycle: PositionLifecycle,
    /// Conservative mark of the position (see `marks`). Counted into
    /// `floating` only while lifecycle is `Open`.
    pub mark_value: Cents,
    /// Worst-case exposure (gate input). Counted into `exposure_in_limbo`
    /// while lifecycle is `ResolutionPending` or `Disputed`.
    pub worst_case_exposure: Cents,
}

/// The running tally (spec 5.14): dashboard headline and sizing input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountView {
    pub settled: Cents,
    pub committed: Cents,
    pub floating: Cents,
    /// settled + floating.
    pub total: Cents,
    /// settled - committed. May be negative (documented above).
    pub deployable: Cents,
    /// Worst-case exposure of resolution-pending/disputed positions:
    /// excluded from the bankroll, still charged against gate limits.
    pub exposure_in_limbo: Cents,
}

/// Build the account view from explicit inputs. Pure; checked; errors
/// propagate.
pub fn build_account_view(
    settled: Cents,
    resting_order_cost: Cents,
    active_reservations: Cents,
    pending_settlements_min: Cents,
    positions: impl IntoIterator<Item = PositionValuation>,
) -> Result<AccountView, StateError> {
    let committed = resting_order_cost
        .checked_add(active_reservations)
        .map_err(StateError::Money)?;

    let mut floating = pending_settlements_min;
    let mut exposure_in_limbo = Cents::ZERO;
    for position in positions {
        match position.lifecycle {
            PositionLifecycle::Open => {
                floating = floating
                    .checked_add(position.mark_value)
                    .map_err(StateError::Money)?;
            }
            PositionLifecycle::ResolutionPending | PositionLifecycle::Disputed => {
                exposure_in_limbo = exposure_in_limbo
                    .checked_add(position.worst_case_exposure)
                    .map_err(StateError::Money)?;
            }
        }
    }

    let total = settled.checked_add(floating).map_err(StateError::Money)?;
    let deployable = settled.checked_sub(committed).map_err(StateError::Money)?;

    Ok(AccountView {
        settled,
        committed,
        floating,
        total,
        deployable,
        exposure_in_limbo,
    })
}
