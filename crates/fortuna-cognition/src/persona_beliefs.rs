//! Track E E.4: belief consumption (design §9). Two pure pieces:
//!
//! 1. The **μ/σ→p backbone** (`prob_at_least` = `1 − Φ((t−μ)/σ)`) — deterministic
//!    Rust. The runner computes it and FEEDS it to the persona as data, so the
//!    LLM never does arithmetic (§9); the persona's findings reflect it.
//! 2. The **fan-out** `map_persona_analysis`: a persisted domain-analysis artifact
//!    (the multi-outcome `findings` blob) maps onto one BINARY `BeliefDraft` per
//!    threshold/outcome — exactly as `map_aeolus_envelope` maps brackets
//!    (`reconciliation.rs`). Each draft's `evidence` cites the persona and the
//!    `provenance` carries `{persona_id, persona_version, analysis_id,
//!    analysis_content_hash}` so the belief replays to the exact artifact (I5/5.7).
//!
//! Track E builds against the existing binary belief ledger; it depends on NO
//! scalar/multi-outcome claim type (design §9).

use crate::beliefs::BeliefDraft;
use fortuna_core::clock::UtcTimestamp;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::f64::consts::SQRT_2;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum PersonaBeliefError {
    #[error("findings carry no thresholds or outcomes to fan out")]
    EmptyFindings,
    #[error("findings entry {index} is malformed: {reason}")]
    BadEntry { index: usize, reason: String },
    #[error("belief draft for {event_id} is invalid: {reason}")]
    BadBelief { event_id: String, reason: String },
    #[error("two findings entries map to the same event_id {event_id}")]
    DuplicateEvent { event_id: String },
}

/// Standard-normal CDF Φ(z) via the Abramowitz-Stegun 7.1.26 erf approximation
/// (max abs error ≈ 1.5e-7). Clamped away from exact 0/1 so a deep-tail value
/// stays a VALID belief probability (BeliefDraft requires 0 < p < 1); the clamp
/// only bites beyond ~8σ. Pure + deterministic.
pub fn normal_cdf(z: f64) -> f64 {
    (0.5 * (1.0 + erf(z / SQRT_2))).clamp(f64::EPSILON, 1.0 - f64::EPSILON)
}

fn erf(x: f64) -> f64 {
    // A&S 7.1.26 for |x|; erf is odd so carry the sign.
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let y = 1.0 - poly * (-x * x).exp();
    y.copysign(x)
}

/// The μ/σ→p backbone: P(value ≥ threshold) for a Normal(μ, σ). σ must be > 0.
/// Deterministic Rust — the LLM never computes this (§9). The result is in
/// (0,1) (never exactly saturated; see `normal_cdf`).
pub fn prob_at_least(threshold: f64, mu: f64, sigma: f64) -> Option<f64> {
    if sigma > 0.0 && threshold.is_finite() && mu.is_finite() {
        Some(1.0 - normal_cdf((threshold - mu) / sigma))
    } else {
        None
    }
}

/// Fan a persisted analysis's `findings` onto BINARY `BeliefDraft`s — one per
/// `thresholds[]` entry (weather: `{ge, p}`) and/or `outcomes[]` entry (macro:
/// `{label, p}`). The belief's `p` is the persona's stated probability (the
/// artifact is authoritative, mirroring `map_aeolus_envelope`'s use of the
/// envelope `p`). Each draft cites the persona in evidence + provenance so it
/// replays to the artifact. The `event_id` is derived deterministically from
/// `region_key` + the threshold/label (the composition aligns it to a canonical
/// market event via the edges; E.6 wiring). Threshold ids are prefixed `ge`,
/// outcome ids `out:` so the two branches can never collide; a duplicate
/// event_id within one analysis is rejected (`DuplicateEvent`).
pub fn map_persona_analysis(
    persona_id: &str,
    persona_version: i32,
    analysis_id: &str,
    content_hash: &str,
    region_key: &str,
    findings: &Value,
    horizon: UtcTimestamp,
) -> Result<Vec<BeliefDraft>, PersonaBeliefError> {
    let source = format!("persona:{persona_id}@{persona_version}");
    let provenance = json!({
        "persona_id": persona_id,
        "persona_version": persona_version,
        "analysis_id": analysis_id,
        "analysis_content_hash": content_hash,
    });
    let mut drafts = Vec::new();
    let mut seen = BTreeSet::new();

    if let Some(thresholds) = findings.get("thresholds").and_then(Value::as_array) {
        for (index, entry) in thresholds.iter().enumerate() {
            let ge = entry.get("ge").and_then(Value::as_f64).ok_or_else(|| {
                PersonaBeliefError::BadEntry {
                    index,
                    reason: "threshold missing numeric 'ge'".to_string(),
                }
            })?;
            let p = number_p(entry, index)?;
            push_draft(
                &mut drafts,
                &mut seen,
                region_key,
                &format!("ge{}", trim_num(ge)),
                p,
                horizon,
                &source,
                analysis_id,
                &provenance,
            )?;
        }
    }

    if let Some(outcomes) = findings.get("outcomes").and_then(Value::as_array) {
        for (index, entry) in outcomes.iter().enumerate() {
            let label = entry.get("label").and_then(Value::as_str).ok_or_else(|| {
                PersonaBeliefError::BadEntry {
                    index,
                    reason: "outcome missing string 'label'".to_string(),
                }
            })?;
            let p = number_p(entry, index)?;
            // Raw label (injective; distinct labels -> distinct ids), `out:`-prefixed
            // so it can never collide with a `ge…` threshold id.
            push_draft(
                &mut drafts,
                &mut seen,
                region_key,
                &format!("out:{label}"),
                p,
                horizon,
                &source,
                analysis_id,
                &provenance,
            )?;
        }
    }

    if drafts.is_empty() {
        return Err(PersonaBeliefError::EmptyFindings);
    }
    Ok(drafts)
}

fn number_p(entry: &Value, index: usize) -> Result<f64, PersonaBeliefError> {
    entry
        .get("p")
        .and_then(Value::as_f64)
        .ok_or_else(|| PersonaBeliefError::BadEntry {
            index,
            reason: "entry missing numeric 'p'".to_string(),
        })
}

#[allow(clippy::too_many_arguments)]
fn push_draft(
    drafts: &mut Vec<BeliefDraft>,
    seen: &mut BTreeSet<String>,
    region_key: &str,
    suffix: &str,
    p: f64,
    horizon: UtcTimestamp,
    source: &str,
    analysis_id: &str,
    provenance: &Value,
) -> Result<(), PersonaBeliefError> {
    let event_id = format!("{region_key}#{suffix}");
    if !seen.insert(event_id.clone()) {
        return Err(PersonaBeliefError::DuplicateEvent { event_id });
    }
    let d = BeliefDraft {
        event_id: event_id.clone(),
        p,
        p_raw: p,
        horizon,
        evidence: json!([{
            "source": source,
            "ref": analysis_id,
            "crosscheck": "domain-analysis artifact",
        }]),
        provenance: provenance.clone(),
    };
    d.validate().map_err(|e| PersonaBeliefError::BadBelief {
        event_id,
        reason: e.to_string(),
    })?;
    drafts.push(d);
    Ok(())
}

/// Render a finite number without a trailing `.0` when integral (so `65.0`
/// yields a stable `65`; `64.5` yields `64.5`).
fn trim_num(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() && n.abs() < 9.007e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}
