//! Settlement entry lifecycle (spec 5.13): pending -> posted ->
//! confirmed | reversed. Every transition is a NEW entry superseding the
//! previous one — the same shape as the append-only `settlement_entries`
//! Postgres table (whose triggers refuse UPDATE/DELETE), so the in-memory
//! ledger and the durable record cannot drift structurally. Reversals are
//! venue corrections: the reversed entry supersedes the confirmed one and
//! a fresh pending chain carries the corrected re-settlement.
//!
//! Entry ids are INJECTED (the runner's seeded IdGen), never generated
//! here: the ledger stays deterministic and replayable.

use crate::{Lot, StateError};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, VenueId};
use fortuna_core::money::Cents;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Point-in-time lots + realized PnL, captured BEFORE a settlement is
/// applied to the position book, sufficient to reverse it exactly (spec
/// 5.13: settled -> reversed -> re-settled on venue correction).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementSnapshot {
    pub yes: Lot,
    pub no: Lot,
    pub realized_pnl_before: Cents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettlementStatus {
    Pending,
    Posted,
    Confirmed,
    Reversed,
}

/// One immutable settlement entry (mirrors the Pg row).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementEntry {
    pub entry_id: String,
    pub market: MarketId,
    pub venue: VenueId,
    pub amount_cents: Cents,
    pub status: SettlementStatus,
    pub supersedes: Option<String>,
    pub detail: serde_json::Value,
    pub at: UtcTimestamp,
}

/// In-memory settlement-entry chains, one per market. The HEAD of a chain
/// is the current state; history is never edited.
#[derive(Debug, Default)]
pub struct SettlementLedger {
    chains: BTreeMap<MarketId, Vec<SettlementEntry>>,
}

impl SettlementLedger {
    pub fn new() -> SettlementLedger {
        SettlementLedger::default()
    }

    pub fn head(&self, market: &MarketId) -> Option<&SettlementEntry> {
        self.chains.get(market).and_then(|c| c.last())
    }

    /// Full history for a market, oldest first.
    pub fn chain(&self, market: &MarketId) -> &[SettlementEntry] {
        self.chains.get(market).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn markets(&self) -> impl Iterator<Item = &MarketId> {
        self.chains.keys()
    }

    /// Open a new pending settlement for a market. Refused while an
    /// unfinished chain head (pending/posted/confirmed) exists — a second
    /// settlement only follows a REVERSAL of the first.
    pub fn record_pending(
        &mut self,
        entry_id: String,
        market: MarketId,
        venue: VenueId,
        amount_cents: Cents,
        detail: serde_json::Value,
        at: UtcTimestamp,
    ) -> Result<String, StateError> {
        if let Some(head) = self.head(&market) {
            if head.status != SettlementStatus::Reversed {
                return Err(StateError::SettlementChain {
                    market,
                    reason: format!(
                        "cannot open a new pending settlement over a {:?} head",
                        head.status
                    ),
                });
            }
        }
        let entry = SettlementEntry {
            entry_id: entry_id.clone(),
            market: market.clone(),
            venue,
            amount_cents,
            status: SettlementStatus::Pending,
            supersedes: self.head(&market).map(|h| h.entry_id.clone()),
            detail,
            at,
        };
        self.chains.entry(market).or_default().push(entry);
        Ok(entry_id)
    }

    /// Advance the head by one legal step (pending -> posted ->
    /// confirmed), inserting a superseding entry. Anything else errors.
    pub fn advance(
        &mut self,
        entry_id: String,
        market: &MarketId,
        to: SettlementStatus,
        at: UtcTimestamp,
    ) -> Result<String, StateError> {
        let head = self
            .head(market)
            .ok_or_else(|| StateError::SettlementChain {
                market: market.clone(),
                reason: "no settlement chain to advance".to_string(),
            })?
            .clone();
        let legal = matches!(
            (head.status, to),
            (SettlementStatus::Pending, SettlementStatus::Posted)
                | (SettlementStatus::Posted, SettlementStatus::Confirmed)
        );
        if !legal {
            return Err(StateError::SettlementChain {
                market: market.clone(),
                reason: format!("illegal transition {:?} -> {to:?}", head.status),
            });
        }
        let entry = SettlementEntry {
            entry_id: entry_id.clone(),
            market: head.market,
            venue: head.venue,
            amount_cents: head.amount_cents,
            status: to,
            supersedes: Some(head.entry_id),
            detail: head.detail,
            at,
        };
        if let Some(chain) = self.chains.get_mut(market) {
            chain.push(entry);
        }
        Ok(entry_id)
    }

    /// Venue correction: supersede a posted/confirmed head with a
    /// Reversed entry. The corrected re-settlement follows as a fresh
    /// `record_pending`.
    pub fn reverse(
        &mut self,
        entry_id: String,
        market: &MarketId,
        detail: serde_json::Value,
        at: UtcTimestamp,
    ) -> Result<String, StateError> {
        let head = self
            .head(market)
            .ok_or_else(|| StateError::SettlementChain {
                market: market.clone(),
                reason: "no settlement chain to reverse".to_string(),
            })?
            .clone();
        if !matches!(
            head.status,
            SettlementStatus::Posted | SettlementStatus::Confirmed
        ) {
            return Err(StateError::SettlementChain {
                market: market.clone(),
                reason: format!(
                    "only posted/confirmed settlements reverse (head is {:?})",
                    head.status
                ),
            });
        }
        let entry = SettlementEntry {
            entry_id: entry_id.clone(),
            market: head.market,
            venue: head.venue,
            amount_cents: head.amount_cents,
            status: SettlementStatus::Reversed,
            supersedes: Some(head.entry_id),
            detail,
            at,
        };
        if let Some(chain) = self.chains.get_mut(market) {
            chain.push(entry);
        }
        Ok(entry_id)
    }

    /// Capital-in-limbo (spec 5.13 lifecycle metric): settlement value
    /// announced by the venue but not yet venue-confirmed on our side —
    /// the sum over heads in Pending or Posted.
    pub fn capital_in_limbo(&self) -> Result<Cents, StateError> {
        let mut sum = Cents::ZERO;
        for chain in self.chains.values() {
            if let Some(head) = chain.last() {
                if matches!(
                    head.status,
                    SettlementStatus::Pending | SettlementStatus::Posted
                ) {
                    sum = sum.checked_add(head.amount_cents)?;
                }
            }
        }
        Ok(sum)
    }
}
