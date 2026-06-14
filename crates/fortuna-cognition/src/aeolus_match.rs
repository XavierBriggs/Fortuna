//! F7: world-forward match — from a parsed Aeolus forecast, synthesize the
//! weather market-family it predicts (spec §5.12 world-forward discovery).
//!
//! Each `brackets[]` entry names a temperature-bracket event the forecast speaks
//! to; F7 turns the forecast into a [`WeatherMarketFamily`] whose events are keyed
//! `aeolus:{event_hint}` (the v1 namespace, contract §2) and which carries the
//! RESOLUTION declaration (grading station + authority + `settles_after`) so every
//! synthesized event is SCOREABLE (§5.12 forbids unscoreable beliefs). The family
//! also carries the forecast identity + `model_version` that F8 rides into belief
//! provenance and F9 scores by.
//!
//! SEAM (ledgered in GAPS): matching the synthesized family to LIVE Kalshi markets
//! — does this bracket actually trade right now? — is a venue-discovery concern
//! (the Kalshi adapter), not this cognition transform. F7 produces the
//! FORECAST side (the events the forecast predicts); the venue layer intersects it
//! with the live book. Pure + deterministic; no clock, no panic.

use crate::aeolus_forecast::{AeolusForecast, Authority, Comparison, Variable};
use fortuna_core::clock::UtcTimestamp;

/// One predicted temperature-bracket event (the forecast side of a Kalshi market).
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherEvent {
    /// FORTUNA event id — `aeolus:{event_hint}` (the v1 namespace, contract §2).
    pub event_id: String,
    /// The producer's stable bracket id.
    pub event_hint: String,
    /// The integer-degree threshold (Kalshi temp brackets are integer-degree).
    pub threshold_f: i64,
    /// How the threshold reads (`ge`/`lt`/`in_bracket`).
    pub comparison: Comparison,
    /// Aeolus's OWN probability for the bracket — the cross-check F8 rides into
    /// evidence against FORTUNA's μ/σ-derived p (a large divergence is an alarm).
    pub p_aeolus: f64,
}

/// The weather market-family a single forecast predicts: its events plus the
/// identity + resolution + provenance the downstream slices need.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherMarketFamily {
    pub station: String,
    /// The OFFICIAL grading station (may differ from `station`; never inferred).
    pub nws_station_id: String,
    pub variable: Variable,
    pub target_date: String,
    pub model_version: String,
    pub run_at: UtcTimestamp,
    /// How the events settle — the grader the §5.12 loop depends on.
    pub resolution_authority: Authority,
    /// Earliest the official observation is final — the belief horizon source.
    pub settles_after: UtcTimestamp,
    pub events: Vec<WeatherEvent>,
}

/// Synthesize the [`WeatherMarketFamily`] a forecast predicts: one [`WeatherEvent`]
/// per bracket (`event_id = aeolus:{event_hint}`), carrying the forecast identity,
/// `model_version`, and the resolution declaration. Deterministic; bracket order is
/// preserved.
pub fn match_forecast(fc: &AeolusForecast) -> WeatherMarketFamily {
    let events = fc
        .brackets()
        .iter()
        .map(|b| WeatherEvent {
            event_id: format!("aeolus:{}", b.event_hint),
            event_hint: b.event_hint.clone(),
            threshold_f: b.threshold_f,
            comparison: b.comparison,
            p_aeolus: b.p,
        })
        .collect();
    WeatherMarketFamily {
        station: fc.station().to_string(),
        nws_station_id: fc.resolution().nws_station_id.clone(),
        variable: fc.variable(),
        target_date: fc.target_date().to_string(),
        model_version: fc.distribution().model_version.clone(),
        run_at: fc.run_at(),
        resolution_authority: fc.resolution().authority,
        settles_after: fc.resolution().settles_after,
        events,
    }
}
