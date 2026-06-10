//! Halt flags (spec 5.3 check 1; I2/I3).
//!
//! Halts are set by the system (drawdown monitor, rate-limit breach) and
//! cleared ONLY by the operator re-arm path (CLI, T0.9). Nothing in the
//! trading loop clears a halt: no automatic resumption, ever (I2). A
//! rate-limit breach is a halt, not a throttle (I3): token refill never
//! un-halts.

use crate::config::GateError;
use std::collections::BTreeMap;
use std::fmt;

/// What a halt applies to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum HaltScope {
    Global,
    Strategy(String),
    Venue(String),
}

impl fmt::Display for HaltScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HaltScope::Global => write!(f, "global"),
            HaltScope::Strategy(s) => write!(f, "strategy {s}"),
            HaltScope::Venue(v) => write!(f, "venue {v}"),
        }
    }
}

/// The halt state. Owned by the gate pipeline; check 1 consults it on every
/// order regardless of origin (I1).
#[derive(Debug, Clone, Default)]
pub struct HaltFlags {
    global: Option<String>,
    strategies: BTreeMap<String, String>,
    venues: BTreeMap<String, String>,
}

impl HaltFlags {
    /// Set a halt. Idempotent; the first reason is preserved (the original
    /// cause matters more than the latest).
    pub fn set(&mut self, scope: HaltScope, reason: impl Into<String>) {
        let reason = reason.into();
        match scope {
            HaltScope::Global => {
                self.global.get_or_insert(reason);
            }
            HaltScope::Strategy(s) => {
                self.strategies.entry(s).or_insert(reason);
            }
            HaltScope::Venue(v) => {
                self.venues.entry(v).or_insert(reason);
            }
        }
    }

    /// First halt blocking this (strategy, venue) pair, if any.
    pub fn blocking(&self, strategy: &str, venue: &str) -> Option<(HaltScope, &str)> {
        if let Some(r) = &self.global {
            return Some((HaltScope::Global, r));
        }
        if let Some(r) = self.strategies.get(strategy) {
            return Some((HaltScope::Strategy(strategy.to_string()), r));
        }
        if let Some(r) = self.venues.get(venue) {
            return Some((HaltScope::Venue(venue.to_string()), r));
        }
        None
    }

    pub fn global_halted(&self) -> Option<&str> {
        self.global.as_deref()
    }

    pub fn strategy_halted(&self, strategy: &str) -> Option<&str> {
        self.strategies.get(strategy).map(String::as_str)
    }

    pub fn venue_halted(&self, venue: &str) -> Option<&str> {
        self.venues.get(venue).map(String::as_str)
    }

    /// Operator re-arm (I2): the ONLY clear path. Wired to the CLI in T0.9;
    /// nothing in the trading loop may call this. Re-arming a clear scope is
    /// an error: operator actions never silently no-op.
    pub fn rearm(&mut self, scope: HaltScope) -> Result<(), GateError> {
        let cleared = match &scope {
            HaltScope::Global => self.global.take().is_some(),
            HaltScope::Strategy(s) => self.strategies.remove(s).is_some(),
            HaltScope::Venue(v) => self.venues.remove(v).is_some(),
        };
        if cleared {
            Ok(())
        } else {
            Err(GateError::RearmNotHalted {
                scope: scope.to_string(),
            })
        }
    }
}
