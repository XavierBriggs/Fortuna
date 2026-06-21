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

use serde::{Deserialize, Serialize};

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
