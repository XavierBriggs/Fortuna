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
use crate::context::{content_hash_of, ContextItem, SectionKind};
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

/// Parse weather grading keys from a `weather:STATION:variable:date` region_key.
///
/// Returns `(nws_station_id, variable, target_date)` if and only if:
/// - the key has exactly 4+ colon-separated segments
/// - segment 0 is "weather"
/// - segment 1 is the station (non-empty)
/// - segment 2 is the variable (non-empty)
/// - segment 3 passes `is_iso_date` (YYYY-MM-DD shaped and parseable)
///
/// Any parse shortfall returns `None` — the caller omits the keys silently.
/// Never panics (positional split only, no indexing past `.get()`).
fn parse_weather_grading_keys(region_key: &str) -> Option<(String, String, String)> {
    let mut segs = region_key.splitn(5, ':');
    let domain = segs.next()?;
    if domain != "weather" {
        return None;
    }
    let station = segs.next().filter(|s| !s.is_empty())?;
    let variable = segs.next().filter(|s| !s.is_empty())?;
    let date = segs.next().filter(|s| is_iso_date(s))?;
    // Reuse is_iso_date's shape check; validate calendar range via timestamp parse.
    UtcTimestamp::parse_iso8601(&format!("{date}T00:00:00.000Z"))
        .ok()
        .map(|_| (station.to_string(), variable.to_string(), date.to_string()))
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
    // WS1.1: producer is always stamped so downstream jobs can route without
    // re-parsing the source.  Weather grading keys (nws_station_id / variable /
    // target_date) are added ONLY on the weather-threshold path, parsed from the
    // region_key by positional `:` split — never on the macro outcomes path.
    let base_provenance = json!({
        "producer": persona_id,
        "persona_id": persona_id,
        "persona_version": persona_version,
        "analysis_id": analysis_id,
        "analysis_content_hash": content_hash,
    });

    // Try to parse weather grading keys up front; `None` → thresholds use
    // base_provenance, never a partial / wrong key.
    let weather_grading_keys = parse_weather_grading_keys(region_key);

    // Provenance with weather grading keys appended — built lazily only when
    // needed and only when the parse succeeded.
    let weather_provenance: Option<Value> =
        weather_grading_keys
            .as_ref()
            .map(|(station, variable, date)| {
                let mut prov = base_provenance.clone();
                if let Some(obj) = prov.as_object_mut() {
                    obj.insert("nws_station_id".to_string(), json!(station));
                    obj.insert("variable".to_string(), json!(variable));
                    obj.insert("target_date".to_string(), json!(date));
                }
                prov
            });

    let mut drafts = Vec::new();
    let mut seen = BTreeSet::new();

    if let Some(thresholds) = findings.get("thresholds").and_then(Value::as_array) {
        // Weather grading keys ride on the threshold path only; if the region_key
        // didn't parse (malformed / macro-shaped), fall back to base_provenance.
        let threshold_prov = weather_provenance.as_ref().unwrap_or(&base_provenance);
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
                threshold_prov,
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
            // so it can never collide with a `ge…` threshold id. Macro outcomes path
            // never receives weather grading keys (base_provenance only).
            push_draft(
                &mut drafts,
                &mut seen,
                region_key,
                &format!("out:{label}"),
                p,
                horizon,
                &source,
                analysis_id,
                &base_provenance,
            )?;
        }
    }

    if drafts.is_empty() {
        return Err(PersonaBeliefError::EmptyFindings);
    }
    Ok(drafts)
}

/// Derive the belief resolution `horizon` for an analysis from its `region_key`:
/// the end of the UTC day (`T23:59:59.999Z`) of the `YYYY-MM-DD` segment embedded
/// in the key. Both shipped persona region_keys carry a date
/// (`weather:KNYC:tmax:2026-06-12`, `macro:US-CPI-MoM:2026-06-12`) — the
/// date-resolving-market convention (design §11: weather's daily resolution, the
/// macro release date). Returns `None` when the key has no parseable date; the
/// daemon then persists the artifact but SKIPS belief fan-out (only beliefs need a
/// horizon). A per-domain refinement (intraday prints, local-vs-UTC day edges) is a
/// documented future tweak — this is the conservative first cut. PURE; never panics.
pub fn belief_horizon(region_key: &str) -> Option<UtcTimestamp> {
    let date = region_key.split(':').find(|seg| is_iso_date(seg))?;
    UtcTimestamp::parse_iso8601(&format!("{date}T23:59:59.999Z")).ok()
}

/// A `YYYY-MM-DD`-SHAPED, ASCII, 10-char segment. Calendar-range validity (e.g.
/// month ≤ 12) is left to the timestamp parse, which rejects an impossible date.
/// Char-iterated, never indexed → no panic.
fn is_iso_date(seg: &str) -> bool {
    seg.len() == 10
        && seg.char_indices().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        })
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

/// Build the high-priority `DomainAnalysis` context item for a persisted artifact
/// (design §9, E.4b) so the synthesis Mind reads the persona's pre-digested findings
/// as ONE high-value item alongside the raw signals. The item's `content_hash` is
/// the artifact's replay anchor, so the assembled-context manifest references the
/// artifact by id + hash (5.7). The body renders the findings as DATA — it is an
/// untrusted-but-pre-digested context item, NOT the trusted method (which still rides
/// only in the Mind system message, §4).
pub fn domain_analysis_context_item(
    persona_id: &str,
    persona_version: i32,
    analysis_id: &str,
    region_key: &str,
    findings: &Value,
    content_hash: &str,
    at: UtcTimestamp,
) -> ContextItem {
    let rendered = serde_json::to_string_pretty(findings).unwrap_or_else(|_| findings.to_string());
    // The artifact's content_hash rides IN the body (traceability); the ITEM
    // content_hash follows the assembler convention (hash of the rendered body),
    // and the item_id is the analysis_id — so the context manifest references the
    // artifact by id, and the body replays from the persisted findings (5.7).
    let body = format!(
        "persona:{persona_id}@{persona_version} domain-analysis for {region_key} \
         (artifact {content_hash}):\n{rendered}"
    );
    ContextItem {
        item_id: analysis_id.to_string(),
        section: SectionKind::DomainAnalysis,
        content_hash: content_hash_of(&body),
        body,
        at,
    }
}

#[cfg(test)]
mod horizon_tests {
    use super::belief_horizon;
    use fortuna_core::clock::UtcTimestamp;

    fn end_of(date: &str) -> Option<UtcTimestamp> {
        UtcTimestamp::parse_iso8601(&format!("{date}T23:59:59.999Z")).ok()
    }

    #[test]
    fn weather_region_key_resolves_to_end_of_its_date() {
        assert_eq!(
            belief_horizon("weather:KNYC:tmax:2026-06-12"),
            end_of("2026-06-12")
        );
    }

    #[test]
    fn macro_region_key_resolves_to_end_of_its_date() {
        assert_eq!(
            belief_horizon("macro:US-CPI-MoM:2026-06-12"),
            end_of("2026-06-12")
        );
    }

    #[test]
    fn a_date_segment_anywhere_in_the_key_is_found() {
        assert_eq!(
            belief_horizon("2026-06-12:extra:segment"),
            end_of("2026-06-12")
        );
    }

    #[test]
    fn no_date_segment_yields_none() {
        assert!(belief_horizon("weather:KNYC:tmax").is_none());
        assert!(belief_horizon("macro").is_none());
        assert!(belief_horizon("").is_none());
    }

    #[test]
    fn date_shaped_but_impossible_calendar_date_is_none() {
        // Passes the shape check; the timestamp parse rejects month 13 / day 99.
        assert!(belief_horizon("x:2026-13-99").is_none());
    }

    #[test]
    fn wrong_shape_is_not_mistaken_for_a_date() {
        assert!(belief_horizon("x:2026/06/12").is_none()); // slashes, not dashes
        assert!(belief_horizon("x:26-06-12").is_none()); // 8 chars, not 10
        assert!(belief_horizon("x:2026-6-12").is_none()); // not zero-padded
    }
}
