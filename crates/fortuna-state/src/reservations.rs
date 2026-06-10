//! Per-strategy capital reservation ledger (spec 5.14).
//!
//! Capital allocation has exactly two tiers: total bankroll -> per-strategy
//! envelopes (config, monthly review). Sizing draws only from the strategy's
//! envelope through this ledger: each candidate order reserves at gate time
//! and releases on cancel or position close, so concurrent sizing cannot
//! jointly over-commit an envelope.
//!
//! Fail-closed rules: unknown strategy, negative amount, duplicate intent,
//! or envelope overflow all refuse the reservation. Exactly-at-envelope
//! passes. `release` is idempotent: it returns `Ok(true)` exactly once per
//! live reservation and `Ok(false)` for already-released or never-existing
//! intents - totals can never double-decrement.
//!
//! # Derived state and rebuild (spec 5.14)
//!
//! Reservations are DERIVED state: `rebuild` wholesale-reconstructs the
//! ledger from open intents during boot reconciliation, so a crash can never
//! leak a reservation and permanently lock envelope capital.
//!
//! Conservative choice, documented: `rebuild` ACCEPTS totals that exceed the
//! configured envelope (including strategies absent from config entirely).
//! At boot with a reduced envelope this is a legitimate state - the open
//! intents are real and need managing; refusing to load would brick
//! recovery. The condition is exposed via `over_envelope` (and a negative
//! `headroom`), and every NEW reservation for that strategy fails the
//! envelope check (or the unknown-strategy check) until the old ones unwind.
//! What rebuild does NOT accept silently is corrupt state: duplicate intent
//! ids or negative amounts are errors.

use crate::StateError;
use fortuna_core::ids::IntentId;
use fortuna_core::money::Cents;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
struct ReservationEntry {
    strategy: String,
    amount: Cents,
}

/// Envelope-capped reservation ledger. Deterministic iteration; pure state.
#[derive(Debug, Clone)]
pub struct ReservationLedger {
    /// Per-strategy envelopes from config.
    envelopes: BTreeMap<String, Cents>,
    /// Live reservations by intent.
    active: BTreeMap<IntentId, ReservationEntry>,
    /// Cached per-strategy totals (invariant: equals the sum over `active`).
    totals: BTreeMap<String, Cents>,
}

impl ReservationLedger {
    /// Fresh ledger with no active reservations.
    pub fn new(envelopes: BTreeMap<String, Cents>) -> ReservationLedger {
        ReservationLedger {
            envelopes,
            active: BTreeMap::new(),
            totals: BTreeMap::new(),
        }
    }

    /// Wholesale reconstruction from open intents at boot (derived state).
    /// Replaces ALL prior state by construction. Accepts over-envelope
    /// totals (see module doc); rejects corrupt state (duplicate intent ids,
    /// negative amounts).
    pub fn rebuild(
        envelopes: BTreeMap<String, Cents>,
        entries: impl IntoIterator<Item = (IntentId, String, Cents)>,
    ) -> Result<ReservationLedger, StateError> {
        let mut ledger = ReservationLedger::new(envelopes);
        for (intent, strategy, amount) in entries {
            if amount.raw() < 0 {
                return Err(StateError::NegativeReservation { intent, amount });
            }
            if ledger.active.contains_key(&intent) {
                return Err(StateError::DuplicateReservation { intent });
            }
            let new_total = ledger
                .active_total(&strategy)
                .checked_add(amount)
                .map_err(StateError::Money)?;
            ledger.totals.insert(strategy.clone(), new_total);
            ledger
                .active
                .insert(intent, ReservationEntry { strategy, amount });
        }
        Ok(ledger)
    }

    /// Reserve envelope capital for an intent. Fail-closed (see module doc);
    /// exactly-at-envelope passes.
    pub fn reserve(
        &mut self,
        strategy: &str,
        intent: IntentId,
        amount: Cents,
    ) -> Result<(), StateError> {
        let envelope =
            *self
                .envelopes
                .get(strategy)
                .ok_or_else(|| StateError::UnknownStrategy {
                    strategy: strategy.to_string(),
                })?;
        if amount.raw() < 0 {
            return Err(StateError::NegativeReservation { intent, amount });
        }
        if self.active.contains_key(&intent) {
            return Err(StateError::DuplicateReservation { intent });
        }
        let current = self.active_total(strategy);
        let new_total = current.checked_add(amount).map_err(StateError::Money)?;
        if new_total > envelope {
            return Err(StateError::EnvelopeExceeded {
                strategy: strategy.to_string(),
                requested: amount,
                headroom: envelope.checked_sub(current).map_err(StateError::Money)?,
            });
        }
        self.totals.insert(strategy.to_string(), new_total);
        self.active.insert(
            intent,
            ReservationEntry {
                strategy: strategy.to_string(),
                amount,
            },
        );
        Ok(())
    }

    /// Release an intent's reservation. Idempotent: `Ok(true)` if released
    /// NOW, `Ok(false)` if already released or never existed. The total is
    /// decremented exactly once per reservation, under any call pattern.
    pub fn release(&mut self, intent: IntentId) -> Result<bool, StateError> {
        let Some(entry) = self.active.get(&intent) else {
            return Ok(false);
        };
        // Compute the new total BEFORE mutating anything so an (impossible
        // by invariant) arithmetic failure leaves the ledger consistent.
        let new_total = self
            .active_total(&entry.strategy)
            .checked_sub(entry.amount)
            .map_err(StateError::Money)?;
        let strategy = entry.strategy.clone();
        self.totals.insert(strategy, new_total);
        self.active.remove(&intent);
        Ok(true)
    }

    /// Sum of live reservations for a strategy (zero if none/unknown).
    pub fn active_total(&self, strategy: &str) -> Cents {
        self.totals.get(strategy).copied().unwrap_or(Cents::ZERO)
    }

    /// Envelope minus live reservations. Negative after an over-envelope
    /// rebuild (documented above). Unknown strategy is an error (fail-closed).
    pub fn headroom(&self, strategy: &str) -> Result<Cents, StateError> {
        let envelope =
            *self
                .envelopes
                .get(strategy)
                .ok_or_else(|| StateError::UnknownStrategy {
                    strategy: strategy.to_string(),
                })?;
        envelope
            .checked_sub(self.active_total(strategy))
            .map_err(StateError::Money)
    }

    /// True when live reservations exceed the configured envelope (possible
    /// only via `rebuild`: reduced envelope or strategy gone from config).
    /// The runner must refuse new work for the strategy while this is set;
    /// `reserve` independently fails closed.
    pub fn over_envelope(&self, strategy: &str) -> bool {
        let envelope = self.envelopes.get(strategy).copied().unwrap_or(Cents::ZERO);
        self.active_total(strategy) > envelope
    }
}
