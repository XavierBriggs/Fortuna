//! Perp margin equity composition for I2 halt math (spec 5.15; T5.B3).
//!
//! Spec 5.15: "I2 drawdown math includes funding paid/received and margin
//! unrealized PnL, marked at the venue's settlement mark per the
//! conservative-marking policy (if the venue mark and our conservative mark
//! disagree, the worse-for-us number governs halt math)."
//!
//! The conservative per-account number is `MarginAccountView.equity`
//! (fortuna-core perp domain: balance + worse-for-us unrealized PnL +
//! pending funding). This module composes it with event-contract equity
//! into the single number the `DrawdownMonitor` consumes. The unmarked
//! flag (some position valued on the venue's number alone) propagates so
//! the caller can ALERT on degraded marking; it never blocks halt math,
//! which is already conservative.

use crate::StateError;
use fortuna_core::money::Cents;
use fortuna_core::perp::MarginAccountView;

/// The composed halt-math equity (the `DrawdownMonitor` input).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HaltEquity {
    /// Event-contract equity + every margin account's conservative equity.
    pub total: Cents,
    /// True when any margin account valued a position without an
    /// independent conservative mark (alert-worthy, not halt-blocking).
    pub unmarked_flag: bool,
}

/// Compose event-contract equity with the perp margin accounts' conservative
/// equity. Checked arithmetic; overflow is an error, never a panic.
pub fn equity_with_margin(
    event_equity: Cents,
    margin_accounts: &[MarginAccountView],
) -> Result<HaltEquity, StateError> {
    let mut total = event_equity;
    let mut unmarked_flag = false;
    for account in margin_accounts {
        total = total
            .checked_add(account.equity)
            .map_err(StateError::Money)?;
        unmarked_flag = unmarked_flag || account.unmarked_flag;
    }
    Ok(HaltEquity {
        total,
        unmarked_flag,
    })
}
