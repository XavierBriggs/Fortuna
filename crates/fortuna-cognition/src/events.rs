//! Canonical events, market-event edges, benchmark snapshots, and CLV
//! (spec 5.12 + 5.5 + the 5.13 event lifecycle reference model).
//!
//! Beliefs attach to EVENTS; markets are venue projections joined by
//! edges. Everything here is pure deterministic logic: lifecycle
//! transitions are legal-or-error, edge tiers gate usage structurally,
//! the deterministic edge checks score what the model proposes, and CLV
//! refuses to manufacture a number when no liquid pre-benchmark snapshot
//! exists. Persistence is the ledger repos' job.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, Side};
use fortuna_core::money::Cents;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EventError {
    #[error("illegal event transition {from:?} -> {to:?}")]
    IllegalTransition { from: EventStatus, to: EventStatus },
    #[error("dead_reason {got:?} not in (voided|source_lost|mutated)")]
    BadDeadReason { got: String },
    #[error("event {event_id} is terminal ({status:?})")]
    Terminal {
        event_id: String,
        status: EventStatus,
    },
}

/// Spec 5.13 event lifecycle vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Created,
    Active,
    ResolutionPending,
    ResolvedProvisional,
    Disputed,
    ResolvedFinal,
    Dead,
}

/// A canonical event (spec 5.12 `events` row).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalEvent {
    pub event_id: String,
    /// UNTRUSTED text (spec 5.11): data, never instructions.
    pub statement: String,
    pub resolution_criteria: String,
    pub resolution_source: String,
    pub horizon: Option<UtcTimestamp>,
    /// Anchors the CLV snapshot schedule (event start when known, else
    /// expected resolution).
    pub benchmark_at: UtcTimestamp,
    pub category: String,
    pub status: EventStatus,
    /// No checkable resolution source => excluded from calibration and
    /// watchlist counts (no beliefs nobody can grade, spec 5.12).
    pub unscoreable: bool,
}

impl CanonicalEvent {
    /// Legal-or-error lifecycle step (5.13): created -> active ->
    /// resolution_pending -> resolved_provisional -> resolved_final, with
    /// the provisional <-> disputed excursion. Terminal states refuse.
    pub fn transition(&mut self, to: EventStatus) -> Result<(), EventError> {
        use EventStatus::*;
        let legal = matches!(
            (self.status, to),
            (Created, Active)
                | (Active, ResolutionPending)
                | (ResolutionPending, ResolvedProvisional)
                | (ResolvedProvisional, Disputed)
                | (Disputed, ResolvedProvisional)
                | (ResolvedProvisional, ResolvedFinal)
                | (Disputed, ResolvedFinal)
        );
        if !legal {
            return Err(EventError::IllegalTransition {
                from: self.status,
                to,
            });
        }
        self.status = to;
        Ok(())
    }

    /// Terminal alternative, reachable from any PRE-final state (5.13).
    pub fn mark_dead(&mut self, reason: &str) -> Result<(), EventError> {
        if !matches!(reason, "voided" | "source_lost" | "mutated") {
            return Err(EventError::BadDeadReason {
                got: reason.to_string(),
            });
        }
        if matches!(self.status, EventStatus::ResolvedFinal | EventStatus::Dead) {
            return Err(EventError::Terminal {
                event_id: self.event_id.clone(),
                status: self.status,
            });
        }
        self.status = EventStatus::Dead;
        Ok(())
    }
}

/// Edge mapping vocabulary (spec 5.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingType {
    Direct,
    Negation,
    BracketComponent,
    ConditionalOn,
}

/// Confidence tiers gate edge usage: cross-venue and multi-leg strategies
/// require HUMAN-confirmed edges (a wrong equivalence edge converts an
/// arbitrage into an unhedged position — the UMA-style failure mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeTier {
    Proposed,
    Confirmed,
}

impl EdgeTier {
    pub fn satisfies(self, required: EdgeTier) -> bool {
        self >= required
    }
}

/// A proposed (or confirmed) market-event edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeProposal {
    pub market: MarketId,
    pub venue: String,
    pub event_id: String,
    pub mapping: MappingType,
    /// Proposer's confidence in [0,1]; the deterministic checks and the
    /// review flow are the gates, not this number alone.
    pub confidence: f64,
    /// model_id or operator id.
    pub proposed_by: String,
    pub confirmed_by: Option<String>,
}

impl EdgeProposal {
    pub fn tier(&self) -> EdgeTier {
        if self.confirmed_by.is_some() {
            EdgeTier::Confirmed
        } else {
            EdgeTier::Proposed
        }
    }
}

/// Inputs for the deterministic edge checks (spec 5.12: "deterministic
/// checks (resolution source match, horizon match) score them").
#[derive(Debug)]
pub struct EdgeCheckInputs<'a> {
    pub event_resolution_source: &'a str,
    pub market_resolution_source: &'a str,
    pub event_horizon: Option<UtcTimestamp>,
    pub market_close_at: Option<UtcTimestamp>,
    pub horizon_tolerance_ms: i64,
}

/// Resolution-source mismatch is the hard failure (score 0.0 — different
/// oracles can disagree forever); a horizon mismatch beyond tolerance
/// halves the score (suspicious but reviewable); both matching scores 1.0.
/// Missing data counts as a mismatch for that check, never a pass.
pub fn deterministic_edge_score(inputs: &EdgeCheckInputs<'_>) -> f64 {
    let source_match = !inputs.event_resolution_source.trim().is_empty()
        && inputs
            .event_resolution_source
            .eq_ignore_ascii_case(inputs.market_resolution_source.trim());
    if !source_match {
        return 0.0;
    }
    let horizon_match = match (inputs.event_horizon, inputs.market_close_at) {
        (Some(h), Some(c)) => {
            (h.epoch_millis() - c.epoch_millis()).abs() <= inputs.horizon_tolerance_ms
        }
        _ => false,
    };
    if horizon_match {
        1.0
    } else {
        0.5
    }
}

/// Snapshot schedule vocabulary (spec 5.5 + the price_snapshots table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    T24h,
    T1h,
    T5m,
    OnTrade,
}

impl SnapshotKind {
    /// Offset before benchmark_at at which the scheduled kind opens.
    fn lead_ms(self) -> Option<i64> {
        match self {
            SnapshotKind::T24h => Some(24 * 3_600_000),
            SnapshotKind::T1h => Some(3_600_000),
            SnapshotKind::T5m => Some(5 * 60_000),
            SnapshotKind::OnTrade => None,
        }
    }
}

/// One due snapshot request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueSnapshot {
    pub event_id: String,
    pub market: MarketId,
    pub kind: SnapshotKind,
}

/// Dedup key for taken scheduled snapshots.
pub type TakenKey = (String, MarketId, SnapshotKind);

impl DueSnapshot {
    pub fn key(&self) -> TakenKey {
        (self.event_id.clone(), self.market.clone(), self.kind)
    }
}

/// Which scheduled snapshots are due NOW for one event's linked markets:
/// a kind is due once its window opens (benchmark_at - lead <= now) and
/// it has not been taken, and NEVER at/after benchmark_at (post-event
/// windows are oracle-drift noise, spec 5.5). Deterministic order:
/// markets as given, kinds T24h < T1h < T5m.
pub fn due_snapshots(
    event_id: &str,
    benchmark_at: UtcTimestamp,
    markets: &[MarketId],
    now: UtcTimestamp,
    taken: &BTreeSet<TakenKey>,
) -> Vec<DueSnapshot> {
    let mut due = Vec::new();
    if now.epoch_millis() >= benchmark_at.epoch_millis() {
        return due;
    }
    for market in markets {
        for kind in [SnapshotKind::T24h, SnapshotKind::T1h, SnapshotKind::T5m] {
            let Some(lead) = kind.lead_ms() else { continue };
            let opens_ms = benchmark_at.epoch_millis() - lead;
            if now.epoch_millis() < opens_ms {
                continue;
            }
            let key = (event_id.to_string(), market.clone(), kind);
            if taken.contains(&key) {
                continue;
            }
            due.push(DueSnapshot {
                event_id: event_id.to_string(),
                market: market.clone(),
                kind,
            });
        }
    }
    due
}

/// One captured book touch (what the price_snapshots row stores).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotPoint {
    pub at: UtcTimestamp,
    pub best_bid: Option<Cents>,
    pub best_ask: Option<Cents>,
    pub bid_qty: i64,
    pub ask_qty: i64,
}

/// Minimum-liquidity filter (spec 5.5: "stale or one-sided books produce
/// no CLV rather than fake CLV").
#[derive(Debug, Clone, Copy)]
pub struct LiquidityPolicy {
    pub min_touch_qty: i64,
    pub max_spread_cents: i64,
}

impl SnapshotPoint {
    pub fn is_liquid(&self, policy: &LiquidityPolicy) -> bool {
        match (self.best_bid, self.best_ask) {
            (Some(bid), Some(ask)) => {
                self.bid_qty >= policy.min_touch_qty
                    && self.ask_qty >= policy.min_touch_qty
                    && ask.raw() - bid.raw() <= policy.max_spread_cents
                    && ask.raw() > bid.raw()
            }
            _ => false,
        }
    }

    /// YES mid in tenth-of-cent precision avoided: integer mid floor'd in
    /// cents x 2 space to keep exactness (mid_x2 = bid + ask).
    fn yes_mid_x2(&self) -> Option<i64> {
        match (self.best_bid, self.best_ask) {
            (Some(b), Some(a)) => Some(b.raw() + a.raw()),
            _ => None,
        }
    }
}

/// CLV in basis points vs the LATEST liquid pre-benchmark snapshot:
/// `((benchmark_value - entry) / entry) x 10_000`, computed in the
/// position's own price space (NO entries mirror to 100 - yes_mid).
/// Returns None when no liquid pre-benchmark snapshot exists.
pub fn clv_bps(
    entry_price: Cents,
    side: Side,
    benchmark_at: UtcTimestamp,
    snapshots: &[SnapshotPoint],
    policy: &LiquidityPolicy,
) -> Option<i64> {
    if entry_price.raw() <= 0 {
        return None;
    }
    let best = snapshots
        .iter()
        .filter(|s| s.at.epoch_millis() < benchmark_at.epoch_millis())
        .filter(|s| s.is_liquid(policy))
        .max_by_key(|s| s.at.epoch_millis())?;
    let yes_mid_x2 = best.yes_mid_x2()?;
    // Own-side mid x2: YES = bid+ask; NO = 200 - (bid+ask).
    let own_mid_x2 = match side {
        Side::Yes => yes_mid_x2,
        Side::No => 200 - yes_mid_x2,
    };
    // bps = (own_mid - entry) / entry * 10_000, all integer:
    // own_mid_x2/2 - entry => (own_mid_x2 - 2*entry) / 2.
    let num = (own_mid_x2 - 2 * entry_price.raw()) * 10_000;
    Some(num / (2 * entry_price.raw()))
}
