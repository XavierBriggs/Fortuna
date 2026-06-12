//! fortuna-state: positions, account views, marks, reservations. Spec 5.14, 5.13.
//!
//! Account views: settled, committed, floating, total; deployable = settled -
//! committed. Conservative-side marking (bid for long, ask for short;
//! wide/stale book => conservative bound + wide-mark flag). Reservation ledger
//! is DERIVED state: rebuilt at boot from open intents and positions.
//! Exposure accounting: resolution_pending and disputed positions remain in
//! exposure at worst case while excluded from bankroll. Drawdown halt flags
//! (I2): human re-arm only, via CLI - the flag itself lives in fortuna-gates;
//! this crate computes the breaches.
//!
//! Everything here is pure, single-threaded, deterministic state: no IO, no
//! wall time (all `now` values are injected `UtcTimestamp`s from the caller's
//! `Clock`), `BTreeMap` everywhere for deterministic iteration, checked
//! integer-cent arithmetic with errors propagated, never panics.

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

mod accounts;
mod drawdown;
mod margin;
mod marks;
mod positions;
mod reservations;
mod settlement;
mod sizing;

pub use accounts::{build_account_view, AccountView, PositionValuation};
pub use drawdown::{DrawdownMonitor, DrawdownVerdict};
pub use margin::{equity_with_margin, HaltEquity};
pub use marks::{mark_lots, Mark, MarkPolicy};
pub use positions::{Lot, Position, PositionBook, PositionLifecycle};
pub use reservations::ReservationLedger;
pub use settlement::{SettlementEntry, SettlementLedger, SettlementSnapshot, SettlementStatus};
pub use sizing::{affordable_sets, kelly_binary, kelly_contracts};

use fortuna_core::ids::IntentId;
use fortuna_core::market::MarketId;
use fortuna_core::money::{Cents, MoneyError};
use thiserror::Error;

/// Errors from state tracking. All arithmetic is checked; overflow is an
/// error value, never a panic and never a silent wrap.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StateError {
    /// Integer-cent arithmetic failed (overflow at conversion or combination).
    #[error(transparent)]
    Money(#[from] MoneyError),
    /// Non-money checked arithmetic failed (quantities, timestamps).
    #[error("checked arithmetic overflow during {op}")]
    Arithmetic { op: &'static str },
    /// A fill is structurally unusable for position accounting.
    #[error("invalid fill {fill_id}: {reason}")]
    InvalidFill {
        fill_id: String,
        reason: &'static str,
    },
    /// The market has no tracked position.
    #[error("no position tracked for market {market}")]
    UnknownMarket { market: MarketId },
    /// A sell reduced a lot below zero: venues are close-only, so this is a
    /// books-vs-venue discrepancy, never a silent flip (spec 5.13).
    #[error("over-close on {market} {side:?}: held {held}, closing {closing}")]
    OverClose {
        market: MarketId,
        side: fortuna_core::market::Side,
        held: i64,
        closing: i64,
    },
    /// Reservations are fail-closed: a strategy without a configured
    /// envelope cannot reserve.
    #[error("unknown strategy {strategy:?}: no envelope configured (fail-closed)")]
    UnknownStrategy { strategy: String },
    /// The settlement-entry chain refused a transition (illegal step,
    /// duplicate pending, or reversal of a non-posted head). Spec 5.13:
    /// illegal transitions error, never coerce.
    #[error("settlement chain on {market}: {reason}")]
    SettlementChain { market: MarketId, reason: String },
    /// A position-level settlement reversal was structurally impossible.
    #[error("illegal settlement reversal on {market}: {reason}")]
    IllegalReversal {
        market: MarketId,
        reason: &'static str,
    },
    /// The reservation would push the strategy past its envelope.
    #[error(
        "reservation of {requested} for strategy {strategy:?} exceeds envelope \
         headroom {headroom}"
    )]
    EnvelopeExceeded {
        strategy: String,
        requested: Cents,
        headroom: Cents,
    },
    /// The intent already holds an active reservation (one reservation per
    /// intent; a second reserve is a logic bug, never silently merged).
    #[error("intent {intent} already holds an active reservation")]
    DuplicateReservation { intent: IntentId },
    /// Reservation amounts are non-negative by construction.
    #[error("negative reservation amount {amount} for intent {intent}")]
    NegativeReservation { intent: IntentId, amount: Cents },
}
