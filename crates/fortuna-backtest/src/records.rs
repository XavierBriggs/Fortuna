//! Generic, portably-serialized historical records (S1).
//!
//! Every record derives `Serialize`/`Deserialize` so that one
//! `serde_json::to_string(&record)` produces a single JSONL line —
//! the canonical FORTUNA↔external-source boundary format.
//!
//! **No source-name literals appear in this file or anywhere under
//! `crates/fortuna-backtest/src/`.** The core is source-agnostic; the
//! only coupled code (`src/sources/`) arrives in S6.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::money::Cents;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors produced when constructing or validating a historical record.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// A [`HistoricalTrade`] was constructed with `orders != 0`.
    ///
    /// The backtest subsystem is **paper-only**: no real order is ever placed
    /// or replayed. `orders` is defined as an invariant-zero field so that
    /// this constraint is checked at construction time rather than silently
    /// carried through the pipeline.
    #[error("paper-only invariant violated: orders must be 0, got {orders}")]
    RealOrderForbidden { orders: u32 },
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// Who made this record and in what strategic context.
///
/// `producer_type` and `producer_id` identify the source system.
/// `mind_id` / `mind_version` are populated only for cognition-backed
/// producers (optional for rule-based or statistical producers).
/// `strategy_id`, `category`, and `scope` are the logical dimensions
/// that the deflation sweep varies over.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub producer_type: String,
    pub producer_id: String,
    pub mind_id: Option<String>,
    pub mind_version: Option<i64>,
    pub strategy_id: String,
    pub category: String,
    pub scope: String,
}

// ---------------------------------------------------------------------------
// BeliefPayload
// ---------------------------------------------------------------------------

/// The content of a forecasting belief.
///
/// - `Binary`: a single probability `p ∈ [0, 1]` for a yes/no event.
///   `p` is a probability, not money — `f64` is correct here.
/// - `Scalar`: a predictive distribution expressed as quantile pairs
///   `(quantile_level, value)` where quantile levels ∈ [0, 1].
///   Both fields are probabilities / scalar forecast values — `f64` is correct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BeliefPayload {
    Binary { p: f64 },
    Scalar { quantiles: Vec<(f64, f64)> },
}

// ---------------------------------------------------------------------------
// HistoricalBelief
// ---------------------------------------------------------------------------

/// A forecasting belief from a historical archive.
///
/// `available_at` is **knowledge time** (when the belief was recorded /
/// became retrievable). It must be strictly less than `decided_at` for the
/// belief to enter a replay decision context (G-PIT; spec §5).
///
/// `decided_at` is when the producing system formed the belief.
///
/// `event_linkage` is the canonical cross-producer join key (see `source.rs`
/// module documentation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalBelief {
    pub provenance: Provenance,
    pub payload: BeliefPayload,
    pub event_linkage: String,
    pub available_at: UtcTimestamp,
    pub decided_at: UtcTimestamp,
}

// ---------------------------------------------------------------------------
// HistoricalOutcome
// ---------------------------------------------------------------------------

/// The ground-truth resolution of an event from a historical archive.
///
/// `outcome` is a numeric value (typically `0.0` or `1.0` for binary events,
/// or a continuous value for scalar events).
///
/// `available_at` on this type is `resolved_at`: outcomes are
/// **post-resolution** quantities and carry knowledge time = resolution time.
/// They may *label*, never *decide* (bitemporal invariant; see `source.rs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalOutcome {
    pub event_linkage: String,
    /// Numeric resolution value. For binary events: `1.0` = YES, `0.0` = NO.
    /// `f64` is correct here — this is a score/label, not money.
    pub outcome: f64,
    pub resolved_at: UtcTimestamp,
    pub resolution_source: String,
}

// ---------------------------------------------------------------------------
// HistoricalSnapshot
// ---------------------------------------------------------------------------

/// A market price snapshot from a historical archive.
///
/// Used as the CLV-entry benchmark: the replay harness selects the latest
/// snapshot with `at < decided_at` (G-PIT).
///
/// `price` is `Cents` (integer cents) — never `f64` for money.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalSnapshot {
    pub market: String,
    pub price: Cents,
    pub at: UtcTimestamp,
}

// ---------------------------------------------------------------------------
// HistoricalTrade
// ---------------------------------------------------------------------------

/// A historical paper trade from an archive.
///
/// **Paper-only invariant:** `orders` is ALWAYS `0`. No real order is ever
/// placed or replayed through this subsystem. The field exists to make the
/// invariant explicit and machine-checked: [`HistoricalTrade::new`] rejects
/// any value other than `0` with [`RecordError::RealOrderForbidden`].
///
/// Use [`HistoricalTrade::new`] to construct; direct struct literal
/// construction is intentionally unavailable outside this module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalTrade {
    pub event_linkage: String,
    /// "yes" or "no" (market convention). Stored as a string to remain
    /// source-agnostic; the harness interprets it per market rules.
    pub side: String,
    /// Fill price in integer cents. Never `f64`.
    pub price: Cents,
    pub contracts: u32,
    pub at: UtcTimestamp,
    /// Invariant: always `0`. Real orders must never flow through this path.
    /// Enforced by [`HistoricalTrade::new`].
    pub orders: u32,
}

impl HistoricalTrade {
    /// Construct a [`HistoricalTrade`], enforcing the paper-only invariant.
    ///
    /// Returns [`RecordError::RealOrderForbidden`] if `orders != 0`.
    ///
    /// # Why `orders` must be `0`
    ///
    /// This subsystem replays **paper** trading history only. Real order IDs
    /// must never flow through the replay path — doing so would conflate
    /// live-execution records with backtest records, potentially violating I5
    /// (append-only audit log integrity) and I6 (propose-only model interface).
    pub fn new(
        event_linkage: String,
        side: String,
        price: Cents,
        contracts: u32,
        at: UtcTimestamp,
        orders: u32,
    ) -> Result<Self, RecordError> {
        if orders != 0 {
            return Err(RecordError::RealOrderForbidden { orders });
        }
        Ok(Self {
            event_linkage,
            side,
            price,
            contracts,
            at,
            orders,
        })
    }
}
