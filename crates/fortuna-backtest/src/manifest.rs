//! Universe manifest — the engaged-set record for G-DEAD (spec §5).
//!
//! The `UniverseManifest` carries every event/market a producer *could or did*
//! form a belief on, **including dead, voided, delisted, and NO-resolved
//! markets**. G-DEAD (S3) enforces that scored coverage == manifest and that
//! voided/NO-resolved markets are present in the scored set.
//!
//! The manifest is obtained from the source via
//! [`crate::source::HistoricalSource::universe_manifest`] and is independent
//! of the belief/outcome streams so the harness can validate coverage without
//! materializing the full archive.
//!
//! ## G-DEAD algorithm
//!
//! [`enforce_gdead`] performs a pure set-difference check:
//!
//! 1. **Coverage:** build a `HashSet` of `event_linkage` strings from
//!    `scored`. Every `manifest.engaged` market whose linkage is ABSENT from
//!    that set is a dropped engagement → violation.
//!
//! 2. The voided/NO-resolved sub-check is subsumed by the coverage check: any
//!    such market absent from the scored set is reported in the same
//!    `DroppedMarkets` violation. The spec intent is that these markets are the
//!    *most likely* to be silently dropped; the coverage check catches them all.
//!
//! A market NOT in the manifest (legitimately un-forecast) does **not** trigger
//! a violation — the check is one-directional (manifest ⊆ scored, not equality
//! of sets).

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ScoredRow
// ---------------------------------------------------------------------------

/// The per-market scored result the harness produces.
///
/// `event_linkage` is the canonical cross-producer join key (see `source.rs`).
/// `outcome` is the numeric resolution value (`0.0` = NO, `1.0` = YES for
/// binary markets; probabilities are `f64` here — not money).
/// `voided` is `true` when the market was cancelled before resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredRow {
    /// Canonical cross-producer join key.
    pub event_linkage: String,
    /// Resolution value. For binary markets: `1.0` = YES, `0.0` = NO.
    /// `f64` is correct — this is a score label, not money.
    pub outcome: f64,
    /// `true` if the market was voided / cancelled before resolution.
    pub voided: bool,
}

// ---------------------------------------------------------------------------
// GDeadViolation
// ---------------------------------------------------------------------------

/// A G-DEAD integrity gate violation.
///
/// Produced by [`enforce_gdead`] when the scored set does not cover all
/// markets in the [`UniverseManifest`].
///
/// The variant carries the full list of offending `event_linkage` strings so
/// the caller can surface a precise, actionable error message.
#[derive(Debug, PartialEq)]
pub enum GDeadViolation {
    /// One or more engaged markets were absent from the scored set.
    ///
    /// This includes voided and NO-resolved markets: they are the most likely
    /// to be silently dropped to inflate apparent performance.
    ///
    /// The inner `Vec<String>` is sorted for deterministic, diff-friendly
    /// output. Use `.0` to iterate over the offending linkages.
    DroppedMarkets(Vec<String>),
}

impl fmt::Display for GDeadViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GDeadViolation::DroppedMarkets(linkages) => write!(
                f,
                "G-DEAD: {} engaged market(s) absent from scored set: {}",
                linkages.len(),
                linkages.join(", ")
            ),
        }
    }
}

impl std::error::Error for GDeadViolation {}

// ---------------------------------------------------------------------------
// enforce_gdead
// ---------------------------------------------------------------------------

/// Enforce the G-DEAD integrity gate (spec §5).
///
/// Checks that every market in `manifest.engaged` appears in `scored` (by
/// `event_linkage`). Returns `Ok(())` if all engaged markets are accounted
/// for; returns `Err(GDeadViolation::DroppedMarkets(…))` listing every absent
/// linkage otherwise.
///
/// ## What this prevents
///
/// A producer must not silently look good by dropping markets it did badly on.
/// Voided and NO-resolved markets are the canonical examples: a forecast on a
/// voided market (cancelled contract) or a NO-resolved market (the predicted
/// event didn't happen) is easy to quietly omit from the scored set. G-DEAD
/// catches this by comparing the manifest to the scored set.
///
/// ## False-positive guard
///
/// A market the producer NEVER engaged (not in the manifest) does **not**
/// trigger a violation. The check is `manifest ⊆ scored`, never
/// `scored ⊆ manifest`. Legitimate non-forecasts are not survivorship.
///
/// ## No `panic!` / `unwrap` / `expect`
///
/// This function is on the critical integrity path. All error conditions are
/// surfaced via the returned `Result`.
pub fn enforce_gdead(
    scored: &[ScoredRow],
    manifest: &UniverseManifest,
) -> Result<(), GDeadViolation> {
    // Build a fast lookup set from the scored linkages.
    let scored_set: HashSet<&str> = scored.iter().map(|r| r.event_linkage.as_str()).collect();

    // Set-difference: manifest engaged \ scored_set.
    let mut dropped: Vec<String> = manifest
        .engaged
        .iter()
        .filter(|m| !scored_set.contains(m.event_linkage.as_str()))
        .map(|m| m.event_linkage.clone())
        .collect();

    if dropped.is_empty() {
        return Ok(());
    }

    // Sort for deterministic, diff-friendly error messages.
    dropped.sort();
    Err(GDeadViolation::DroppedMarkets(dropped))
}

// ---------------------------------------------------------------------------
// EngagedMarket / UniverseManifest
// ---------------------------------------------------------------------------

/// One market/event in the engaged set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngagedMarket {
    /// Canonical cross-producer join key (see `source.rs` module doc).
    pub event_linkage: String,
    /// `true` if the market resolved (YES or NO).
    pub resolved: bool,
    /// `true` if the market was voided / cancelled before resolution.
    pub voided: bool,
}

/// The full engaged set for a historical source.
///
/// Serializes to a single JSONL line via `serde_json::to_string`.
/// Every field in `engaged` corresponds to a market the producer engaged
/// with; absent markets were never engaged and must **not** trigger a
/// G-DEAD false positive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UniverseManifest {
    pub engaged: Vec<EngagedMarket>,
}
