//! [`HistoricalSource`] trait — the generic source contract for the backtest
//! subsystem.
//!
//! ## Bitemporal invariant (knowledge time vs event time)
//!
//! Every record carries an `available_at` field that is **knowledge time**:
//! the instant at which the information became available to the decision-maker
//! (i.e. `fetched_at`, the moment the belief was written to the archive).
//!
//! `available_at` is **NEVER** event time, observed time, `target_date`, or
//! any other domain-clock. Mapping `available_at` to a target/event instant
//! is the one mapping error that silently breaks look-ahead prevention: the
//! G-PIT gate (spec §5) enforces `available_at < decided_at` strictly, so
//! if `available_at` is set to a future event date the gate passes but the
//! belief is future-contaminated.
//!
//! **Post-resolution quantities** (outcomes, realized scores) carry
//! `available_at = resolution time` — they may *label*, never *decide*.
//! The replay harness places them in the outcome-side pool, not the belief
//! pool.
//!
//! ## Canonical `event_linkage` namespace
//!
//! `event_linkage` is the **cross-producer join key** that binds beliefs,
//! outcomes, snapshots, and trades for the same event. It is a string with an
//! explicit namespace convention:
//!
//! ```text
//! event://<category>/<producer-scope>/<event-id>/<horizon-date>
//! ```
//!
//! Example: `event://forecast/station-DFW/bracket-ge40/2026-07-04`
//!
//! **Reconciliation rule:** when two producers use different native keys for
//! the same event (e.g., a weather-forecast producer and a future
//! market-record producer), each must normalise to this namespace before
//! emitting records. The harness joins solely on the normalised key. A
//! mismatch silently drops rows at the as-of join (the failure mode already
//! observed in production with `event_id` namespace drift); the namespace
//! convention is the documented contract to prevent it.
//!
//! ## Sync `Iterator` vs async `Stream`
//!
//! **M2 note for reviewers:** The design spec (§4) shows `impl Stream<Item =
//! Result<…>>` (async). We deliberately use **sync `Iterator`** here because:
//!
//! 1. Replay is **deterministic and single-threaded** — there is no
//!    concurrent IO in the core replay loop; async adds complexity with no
//!    throughput benefit.
//! 2. The Aeolus SQLite adapter (S6) uses `rusqlite`, which is a **sync**
//!    library. Wrapping it in an async `Stream` would require bridging via
//!    `spawn_blocking`, adding overhead and non-determinism.
//! 3. The replay harness (S2) controls its own pacing via the injected
//!    `Clock` — it does not benefit from an async executor's concurrency.
//!
//! This is a deliberate architectural choice, not drift from the spec. Future
//! sources that are natively async can wrap their `Stream` in a
//! `block_in_place` adapter to satisfy this trait without changing the core.

use crate::manifest::UniverseManifest;
use crate::records::{HistoricalBelief, HistoricalOutcome, HistoricalSnapshot, HistoricalTrade};
use thiserror::Error;

// ---------------------------------------------------------------------------
// SourceError
// ---------------------------------------------------------------------------

/// Errors produced by a [`HistoricalSource`] implementation.
#[derive(Debug, Error)]
pub enum SourceError {
    /// An IO error occurred while reading from the underlying storage.
    #[error("IO error reading from source: {reason}")]
    Io { reason: String },

    /// The underlying data is malformed or fails schema validation.
    #[error("source data is malformed: {reason}")]
    Malformed { reason: String },

    /// The source cannot supply a universe manifest.
    #[error("universe manifest unavailable: {reason}")]
    ManifestUnavailable { reason: String },
}

// ---------------------------------------------------------------------------
// HistoricalSource trait
// ---------------------------------------------------------------------------

/// A generic historical data source for the backtest replay harness.
///
/// Implementations yield **streaming iterators** of typed records — the
/// archive is never fully materialised in memory. The harness calls each
/// method once per replay pass and consumes the iterator lazily.
///
/// **Sync `Iterator` design (see module-level M2 note):** Each method returns
/// a `Box<dyn Iterator<…>>` rather than an async `Stream`. This matches the
/// replay harness's single-threaded, deterministic design and the sync
/// `rusqlite` adapter in S6.
///
/// **Bitemporal invariant (see module-level doc):** every record's
/// `available_at` must be knowledge time (`fetched_at`), never event or
/// target time. The harness enforces G-PIT (`available_at < decided_at`) but
/// cannot detect a mapping error — that correctness property belongs to the
/// source implementation.
///
/// **Paper-only:** [`Self::trades`] yields paper trades only (`orders == 0`).
/// Any source that would yield real orders is violating the paper-only
/// invariant; the [`crate::records::HistoricalTrade::new`] constructor
/// enforces this at record construction time.
pub trait HistoricalSource {
    /// Yield all historical beliefs from this source, in any order.
    ///
    /// The harness assembles each decision context by joining beliefs with
    /// outcomes and snapshots on `event_linkage` via an as-of join keyed on
    /// `available_at`; order in this iterator does not affect correctness.
    fn beliefs(&self) -> Box<dyn Iterator<Item = Result<HistoricalBelief, SourceError>> + '_>;

    /// Yield all historical outcome resolutions from this source.
    ///
    /// `available_at` on outcomes equals `resolved_at` (knowledge time =
    /// resolution time). These are placed in the outcome-side pool only.
    fn outcomes(&self) -> Box<dyn Iterator<Item = Result<HistoricalOutcome, SourceError>> + '_>;

    /// Yield all historical market price snapshots from this source.
    ///
    /// Used as the CLV-entry benchmark. The harness selects the latest
    /// snapshot with `at < decided_at` (G-PIT; spec §5).
    fn snapshots(&self) -> Box<dyn Iterator<Item = Result<HistoricalSnapshot, SourceError>> + '_>;

    /// Yield all historical paper trades from this source.
    ///
    /// **Invariant:** every yielded [`HistoricalTrade`] has `orders == 0`.
    /// This is enforced by [`crate::records::HistoricalTrade::new`] at
    /// construction time.
    fn trades(&self) -> Box<dyn Iterator<Item = Result<HistoricalTrade, SourceError>> + '_>;

    /// Return the universe manifest — the full engaged set including dead,
    /// voided, delisted, and NO-resolved markets.
    ///
    /// Called once per replay pass by the harness. The manifest is used by
    /// G-DEAD (S3) to enforce that scored coverage equals the engaged set and
    /// that voided/NO-resolved markets appear in the scored set.
    fn universe_manifest(&self) -> Result<UniverseManifest, SourceError>;
}
