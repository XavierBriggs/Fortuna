//! Belief ledger logic (spec 5.5): the model's primary artifact.
//!
//! Beliefs are IMMUTABLE rows; an update is a NEW belief superseding the
//! old. Probabilities live strictly inside (0,1) — a model claiming
//! certainty is schema-invalid output, rejected, never repaired silently
//! (5.9). Abandonment (the event died) excludes a belief from calibration
//! ENTIRELY: a voided market is the world breaking the question, not the
//! model being wrong. The freshness policy decides which beliefs the
//! comparator may even look at; staleness near the benchmark costs the
//! most, so the pre-benchmark window tightens the cadence.
//!
//! Persistence is the ledger repo's job; everything here is pure.

use fortuna_core::clock::UtcTimestamp;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// serde adapter for a MODEL-EMITTED horizon: a full RFC3339 datetime OR a bare
/// `YYYY-MM-DD` date (normalized to UTC midnight). Used only on cognition fields
/// the model writes; venue/audit timestamps keep the strict default deserializer.
pub(crate) fn de_horizon<'de, D: Deserializer<'de>>(d: D) -> Result<UtcTimestamp, D::Error> {
    let s = String::deserialize(d)?;
    UtcTimestamp::parse_iso8601_or_date(&s).map_err(serde::de::Error::custom)
}

/// As [`de_horizon`], for an OPTIONAL horizon (`null`/absent ⇒ `None`).
pub(crate) fn de_horizon_opt<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<UtcTimestamp>, D::Error> {
    match Option::<String>::deserialize(d)? {
        None => Ok(None),
        Some(s) => UtcTimestamp::parse_iso8601_or_date(&s)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Error)]
pub enum BeliefError {
    #[error("probability {field} = {got} is not strictly inside (0,1)")]
    BadProbability { field: &'static str, got: f64 },
    #[error("event_id is empty")]
    EmptyEvent,
}

/// Status vocabulary (matches the beliefs table CHECK).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeliefStatus {
    Open,
    Resolved,
    Superseded,
    Abandoned,
}

impl BeliefStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            BeliefStatus::Open => "open",
            BeliefStatus::Resolved => "resolved",
            BeliefStatus::Superseded => "superseded",
            BeliefStatus::Abandoned => "abandoned",
        }
    }
}

/// What the mind emits (pre-persistence). `evidence` and `provenance`
/// follow the spec's JSON shapes; both are DATA (5.11 discipline).
/// Unknown fields are REJECTED (I6 schema discipline).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeliefDraft {
    pub event_id: String,
    /// Post-calibration probability.
    pub p: f64,
    /// Pre-calibration probability.
    pub p_raw: f64,
    /// The model emits this; it is parsed leniently (a bare `YYYY-MM-DD` date is
    /// normalized to UTC midnight) because the LLM routinely emits a date-only
    /// event horizon, and one such field must not reject an otherwise-valid
    /// belief. Serialization is always the strict full ISO8601 form.
    #[serde(deserialize_with = "de_horizon")]
    pub horizon: UtcTimestamp,
    pub evidence: serde_json::Value,
    /// Stamped by the HARNESS after the model call ({model_id,
    /// context_manifest_hash, cost_cents, ...}) — the model cannot know
    /// its own prompt hash, so this is never part of the model's schema.
    #[serde(default)]
    pub provenance: serde_json::Value,
}

fn check_probability(field: &'static str, v: f64) -> Result<(), BeliefError> {
    if v.is_finite() && v > 0.0 && v < 1.0 {
        Ok(())
    } else {
        Err(BeliefError::BadProbability { field, got: v })
    }
}

impl BeliefDraft {
    /// Schema-invalid output is rejected, never repaired (spec 5.9).
    pub fn validate(&self) -> Result<(), BeliefError> {
        if self.event_id.trim().is_empty() {
            return Err(BeliefError::EmptyEvent);
        }
        check_probability("p", self.p)?;
        check_probability("p_raw", self.p_raw)?;
        Ok(())
    }
}

/// Brier score: squared error of the probability against the realized
/// outcome. Lower is better; 0.25 is the score of a fair coin claim.
pub fn brier_score(p: f64, outcome: bool) -> f64 {
    let o = if outcome { 1.0 } else { 0.0 };
    (p - o) * (p - o)
}

/// The comparator's freshness verdict for one belief.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    /// Stale beliefs are EXCLUDED from the comparator until refreshed; a
    /// position held under one raises the stranded-state watchdog (5.13).
    Stale {
        reason: String,
    },
}

/// Category-configured maximum ages + the tightened pre-benchmark window
/// (spec 5.5 freshness policy).
#[derive(Debug, Clone)]
pub struct FreshnessPolicy {
    default_max_age_ms: i64,
    category_max_age_ms: BTreeMap<String, i64>,
    /// Window before benchmark_at in which the tightened age applies.
    pre_benchmark_window_ms: i64,
    /// Maximum age while inside that window.
    pre_benchmark_max_age_ms: i64,
}

impl FreshnessPolicy {
    pub fn new(
        default_max_age_ms: i64,
        pre_benchmark_window_ms: i64,
        pre_benchmark_max_age_ms: i64,
    ) -> FreshnessPolicy {
        FreshnessPolicy {
            default_max_age_ms,
            category_max_age_ms: BTreeMap::new(),
            pre_benchmark_window_ms,
            pre_benchmark_max_age_ms,
        }
    }

    pub fn set_category_max_age(&mut self, category: &str, max_age_ms: i64) {
        self.category_max_age_ms
            .insert(category.to_string(), max_age_ms);
    }

    /// Assess one belief. `relevant_signal_at` is the newest signal on
    /// the belief's event (the caller joins via the edges); a signal
    /// arriving AFTER the belief was formed forces a refresh.
    pub fn assess(
        &self,
        category: &str,
        created_at: UtcTimestamp,
        benchmark_at: UtcTimestamp,
        now: UtcTimestamp,
        relevant_signal_at: Option<UtcTimestamp>,
    ) -> Freshness {
        if let Some(sig) = relevant_signal_at {
            if sig.epoch_millis() > created_at.epoch_millis() {
                return Freshness::Stale {
                    reason: format!("relevant signal at {sig} postdates the belief"),
                };
            }
        }
        let age = now.epoch_millis() - created_at.epoch_millis();
        let in_window = benchmark_at.epoch_millis() - now.epoch_millis()
            <= self.pre_benchmark_window_ms
            && now.epoch_millis() < benchmark_at.epoch_millis();
        let max_age = if in_window {
            self.pre_benchmark_max_age_ms
        } else {
            self.category_max_age_ms
                .get(category)
                .copied()
                .unwrap_or(self.default_max_age_ms)
        };
        if age > max_age {
            Freshness::Stale {
                reason: format!(
                    "age {age}ms exceeds max {max_age}ms{}",
                    if in_window {
                        " (pre-benchmark window)"
                    } else {
                        ""
                    }
                ),
            }
        } else {
            Freshness::Fresh
        }
    }
}

/// One calibration bucket: predictions in [lo, hi) with their observed
/// frequency. Empty buckets are omitted (no fake calibration points).
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationBucket {
    pub lo: f64,
    pub hi: f64,
    pub n: usize,
    pub mean_p: f64,
    pub observed_frequency: f64,
}

/// Bucket RESOLVED beliefs into a reliability curve. The grouping
/// dimension (model_id / category / strategy) is the caller's query; this
/// computes one curve from its samples.
pub fn calibration_curve(samples: &[(f64, bool)], buckets: usize) -> Vec<CalibrationBucket> {
    if buckets == 0 {
        return Vec::new();
    }
    let width = 1.0 / buckets as f64;
    let mut out = Vec::new();
    for b in 0..buckets {
        let lo = b as f64 * width;
        let hi = if b == buckets - 1 {
            1.0 + 1e-12
        } else {
            lo + width
        };
        let in_bucket: Vec<&(f64, bool)> = samples
            .iter()
            .filter(|(p, _)| *p >= lo && *p < hi)
            .collect();
        if in_bucket.is_empty() {
            continue;
        }
        let n = in_bucket.len();
        let mean_p = in_bucket.iter().map(|(p, _)| p).sum::<f64>() / n as f64;
        let hits = in_bucket.iter().filter(|(_, o)| *o).count();
        out.push(CalibrationBucket {
            lo,
            hi: hi.min(1.0),
            n,
            mean_p,
            observed_frequency: hits as f64 / n as f64,
        });
    }
    out
}
