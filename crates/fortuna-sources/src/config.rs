//! Per-source operational config (design doc §4.3): the registry decides
//! WHETHER a source may exist; this config decides HOW it behaves. Parsed
//! from the `[sources.<id>]` tables of fortuna.toml.
//!
//! Fail-closed throughout: unknown kinds, unknown fields, non-https URLs,
//! model extraction without a trust cap, and anything Phase A cannot
//! actually run (enabled scrape/mcp sources, enabled model extraction)
//! are hard errors, never defaults. Loosening a Phase-A refusal is a
//! deliberate, reviewable diff here — not a runtime toggle.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::NaiveTime;
use serde::Deserialize;

use crate::error::SourcesError;

/// Adapter class (design §4.2 table). `Scrape` and `Mcp` exist in the
/// taxonomy from day one but are not buildable in Phase A.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Calendar,
    Rss,
    Gdelt,
    Nws,
    Scrape,
    Mcp,
}

impl SourceKind {
    fn parse(s: &str) -> Option<SourceKind> {
        match s {
            "calendar" => Some(SourceKind::Calendar),
            "rss" => Some(SourceKind::Rss),
            "gdelt" => Some(SourceKind::Gdelt),
            "nws" => Some(SourceKind::Nws),
            "scrape" => Some(SourceKind::Scrape),
            "mcp" => Some(SourceKind::Mcp),
            _ => None,
        }
    }

    /// Phase A ships the four structured-feed adapters only (design §10;
    /// loop file: "Scrape/Mcp/extraction are later phases").
    pub fn buildable_in_phase_a(self) -> bool {
        !matches!(self, SourceKind::Scrape | SourceKind::Mcp)
    }
}

/// Extraction mode (design §4.2). Phase A: `Model` may be configured only
/// on a DISABLED source (the design doc's illustrative example); enabling
/// it fails validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtractionMode {
    #[default]
    None,
    Model,
}

/// A boosted-cadence polling window (design §4.2 scheduler). Phase A
/// windows are static config, same-day UTC (`from < to`); `days_ref`
/// names a date set resolved elsewhere (e.g. "bls_cpi_dates").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventWindow {
    pub days_ref: String,
    pub from: NaiveTime,
    pub to: NaiveTime,
    pub interval: Duration,
}

/// One validated `[sources.<id>]` entry. Disabled sources may be partial
/// (the design doc ships an illustrative later-phase entry); ENABLING a
/// source requires it to be complete and Phase-A-runnable.
#[derive(Debug, Clone)]
pub struct SourceConfig {
    pub kind: SourceKind,
    pub url: Option<String>,
    pub base_interval: Option<Duration>,
    pub event_windows: Vec<EventWindow>,
    pub extraction: ExtractionMode,
    pub extraction_trust_cap: Option<u8>,
    pub rate_budget_per_min: Option<u32>,
    pub enabled: bool,
    /// Adapter variant within a kind (the factory needs it): `nws` →
    /// `alerts` | `afd`; `calendar` → `schedule` | `latest`. `rss`/`gdelt`
    /// take none. Required when ENABLING an `nws`/`calendar` source.
    pub feed: Option<String>,
}

/// The full `[sources]` map. BTreeMap for deterministic iteration order.
#[derive(Debug, Clone, Default)]
pub struct SourcesConfig {
    pub sources: BTreeMap<String, SourceConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDoc {
    sources: BTreeMap<String, RawSource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSource {
    kind: String,
    url: Option<String>,
    base_interval: Option<String>,
    #[serde(default)]
    event_windows: Vec<RawWindow>,
    extraction: Option<String>,
    extraction_trust_cap: Option<u8>,
    rate_budget_per_min: Option<u32>,
    enabled: Option<bool>,
    feed: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWindow {
    days: String,
    from: String,
    to: String,
    interval: String,
}

impl SourcesConfig {
    /// Parse and validate a TOML document containing `[sources.<id>]`
    /// tables. Every entry is validated; the first invalid entry fails
    /// the whole parse (fail-closed: a partially valid config never
    /// half-runs).
    pub fn from_toml_str(doc: &str) -> Result<SourcesConfig, SourcesError> {
        let raw: RawDoc = toml::from_str(doc).map_err(|e| SourcesError::ConfigParse {
            reason: e.to_string(),
        })?;
        let mut sources = BTreeMap::new();
        for (id, raw_src) in raw.sources {
            let validated = validate_source(&id, raw_src)?;
            sources.insert(id, validated);
        }
        Ok(SourcesConfig { sources })
    }
}

fn invalid(source_id: &str, reason: impl Into<String>) -> SourcesError {
    SourcesError::ConfigInvalid {
        source_id: source_id.to_string(),
        reason: reason.into(),
    }
}

fn validate_source(id: &str, raw: RawSource) -> Result<SourceConfig, SourcesError> {
    let kind = SourceKind::parse(&raw.kind)
        .ok_or_else(|| invalid(id, format!("unknown kind `{}`", raw.kind)))?;

    // Mcp is config-gated OFF by default (design §4.2); everything else
    // defaults on, matching the registry's enabled flag semantics.
    let enabled = raw.enabled.unwrap_or(kind != SourceKind::Mcp);

    let extraction = match raw.extraction.as_deref() {
        None | Some("none") => ExtractionMode::None,
        Some("model") => ExtractionMode::Model,
        Some(other) => return Err(invalid(id, format!("unknown extraction mode `{other}`"))),
    };

    // Checks that hold even for disabled sources: a config file should
    // never contain values that could not become valid by flipping
    // `enabled`.
    if let Some(url) = &raw.url {
        if !url.starts_with("https://") {
            return Err(invalid(
                id,
                format!("url must be https (SSRF posture, design §6): `{url}`"),
            ));
        }
    }
    if let Some(cap) = raw.extraction_trust_cap {
        if cap > 10 {
            return Err(invalid(
                id,
                format!("extraction_trust_cap {cap} outside 0..=10"),
            ));
        }
    }
    if extraction == ExtractionMode::Model && raw.extraction_trust_cap.is_none() {
        return Err(invalid(
            id,
            "extraction = \"model\" requires extraction_trust_cap (design §4.2)",
        ));
    }
    let base_interval = raw
        .base_interval
        .as_deref()
        .map(|s| parse_duration(s).map_err(|r| invalid(id, r)))
        .transpose()?;
    let mut event_windows = Vec::with_capacity(raw.event_windows.len());
    for w in raw.event_windows {
        event_windows.push(validate_window(id, w)?);
    }
    if let Some(budget) = raw.rate_budget_per_min {
        if budget == 0 {
            return Err(invalid(
                id,
                "rate_budget_per_min must be > 0 (a source that may never fetch must be disabled instead)",
            ));
        }
    }

    // Checks that gate ENABLING a source.
    if enabled {
        if !kind.buildable_in_phase_a() {
            return Err(invalid(
                id,
                format!(
                    "kind `{}` is not buildable in Phase A; set enabled = false",
                    raw.kind
                ),
            ));
        }
        if extraction == ExtractionMode::Model {
            return Err(invalid(
                id,
                "Phase A: no model in the ingestion path; enabled sources require extraction = \"none\"",
            ));
        }
        if raw.url.is_none() {
            return Err(invalid(id, "enabled source requires url"));
        }
        if base_interval.is_none() {
            return Err(invalid(id, "enabled source requires base_interval"));
        }
        if raw.rate_budget_per_min.is_none() {
            return Err(invalid(id, "enabled source requires rate_budget_per_min"));
        }
        // `feed` (the adapter variant for nws/calendar) is validated by the
        // FACTORY when it builds the adapter — kept lenient here so the design
        // doc's illustrative example still parses.
    }

    Ok(SourceConfig {
        kind,
        url: raw.url,
        base_interval,
        event_windows,
        extraction,
        extraction_trust_cap: raw.extraction_trust_cap,
        rate_budget_per_min: raw.rate_budget_per_min,
        enabled,
        feed: raw.feed,
    })
}

fn validate_window(id: &str, raw: RawWindow) -> Result<EventWindow, SourcesError> {
    let from = parse_window_time(&raw.from).map_err(|r| invalid(id, r))?;
    let to = parse_window_time(&raw.to).map_err(|r| invalid(id, r))?;
    if from >= to {
        return Err(invalid(
            id,
            format!(
                "event window `{}`..`{}` must satisfy from < to (same-day UTC; split midnight-crossing windows)",
                raw.from, raw.to
            ),
        ));
    }
    let interval = parse_duration(&raw.interval).map_err(|r| invalid(id, r))?;
    Ok(EventWindow {
        days_ref: raw.days,
        from,
        to,
        interval,
    })
}

/// Duration grammar: positive integer + one of s|m|h|d ("10s", "30m",
/// "1d"). Anything else is an error — no silent unit guessing.
fn parse_duration(s: &str) -> Result<Duration, String> {
    if !s.is_ascii() || s.len() < 2 {
        return Err(format!("bad duration `{s}` (expected <n><s|m|h|d>)"));
    }
    let (num, unit) = s.split_at(s.len() - 1);
    let n: u64 = num
        .parse()
        .map_err(|_| format!("bad duration `{s}` (expected <n><s|m|h|d>)"))?;
    if n == 0 {
        return Err(format!("zero duration `{s}`"));
    }
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86_400,
        _ => return Err(format!("bad duration unit in `{s}` (use s|m|h|d)")),
    };
    Ok(Duration::from_secs(secs))
}

/// Window times are `HH:MMZ` — the trailing Z is REQUIRED so config can
/// never be read as local time (point-in-time discipline, spec 5.11).
fn parse_window_time(s: &str) -> Result<NaiveTime, String> {
    let Some(hm) = s.strip_suffix('Z') else {
        return Err(format!("window time `{s}` must be UTC with a trailing Z"));
    };
    NaiveTime::parse_from_str(hm, "%H:%M").map_err(|_| format!("bad window time `{s}` (HH:MMZ)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The design doc's §4.3 example, verbatim (comments included).
    const DESIGN_DOC_EXAMPLE: &str = r#"
[sources.bls_release]
kind = "calendar"             # calendar | rss | gdelt | nws | scrape | mcp
url = "https://api.bls.gov/..."
base_interval = "1d"
# Phase A: static windows. Phase B may derive them from release_scheduled signals.
event_windows = [{ days = "bls_cpi_dates", from = "12:25Z", to = "12:40Z", interval = "10s" }]
extraction = "none"
rate_budget_per_min = 6

[sources.rotten_tomatoes_pages]   # later phase, illustrative
kind = "scrape"
extraction = "model"
extraction_trust_cap = 4
enabled = false
"#;

    #[test]
    fn design_doc_example_parses_verbatim() {
        let cfg = SourcesConfig::from_toml_str(DESIGN_DOC_EXAMPLE).unwrap();
        assert_eq!(cfg.sources.len(), 2);

        let bls = &cfg.sources["bls_release"];
        assert_eq!(bls.kind, SourceKind::Calendar);
        assert!(bls.enabled);
        assert_eq!(bls.url.as_deref(), Some("https://api.bls.gov/..."));
        assert_eq!(bls.base_interval, Some(Duration::from_secs(86_400)));
        assert_eq!(bls.rate_budget_per_min, Some(6));
        assert_eq!(bls.extraction, ExtractionMode::None);
        assert_eq!(bls.event_windows.len(), 1);
        let w = &bls.event_windows[0];
        assert_eq!(w.days_ref, "bls_cpi_dates");
        assert_eq!(w.from, NaiveTime::from_hms_opt(12, 25, 0).unwrap());
        assert_eq!(w.to, NaiveTime::from_hms_opt(12, 40, 0).unwrap());
        assert_eq!(w.interval, Duration::from_secs(10));

        // The later-phase illustrative entry is accepted ONLY because it
        // is disabled.
        let rt = &cfg.sources["rotten_tomatoes_pages"];
        assert_eq!(rt.kind, SourceKind::Scrape);
        assert!(!rt.enabled);
        assert_eq!(rt.extraction, ExtractionMode::Model);
        assert_eq!(rt.extraction_trust_cap, Some(4));
    }

    #[test]
    fn phase_a_refuses_enabled_scrape_and_mcp() {
        for kind in ["scrape", "mcp"] {
            let doc = format!(
                r#"
[sources.x]
kind = "{kind}"
url = "https://example.com/a"
base_interval = "30m"
rate_budget_per_min = 2
enabled = true
"#
            );
            let err = SourcesConfig::from_toml_str(&doc).unwrap_err();
            assert!(
                err.to_string().contains("not buildable in Phase A"),
                "{kind}: {err}"
            );
        }
    }

    #[test]
    fn phase_a_refuses_enabled_model_extraction() {
        let doc = r#"
[sources.x]
kind = "rss"
url = "https://example.com/feed.xml"
base_interval = "30m"
rate_budget_per_min = 2
extraction = "model"
extraction_trust_cap = 4
"#;
        let err = SourcesConfig::from_toml_str(doc).unwrap_err();
        assert!(
            err.to_string().contains("no model in the ingestion path"),
            "{err}"
        );
    }

    #[test]
    fn enabled_source_requires_completeness() {
        // Missing rate_budget_per_min on an otherwise valid enabled source.
        let doc = r#"
[sources.x]
kind = "rss"
url = "https://example.com/feed.xml"
base_interval = "30m"
"#;
        let err = SourcesConfig::from_toml_str(doc).unwrap_err();
        assert!(err.to_string().contains("rate_budget_per_min"), "{err}");
    }

    #[test]
    fn mcp_defaults_to_disabled() {
        let doc = r#"
[sources.x]
kind = "mcp"
"#;
        let cfg = SourcesConfig::from_toml_str(doc).unwrap();
        assert!(!cfg.sources["x"].enabled);
    }

    #[test]
    fn https_only_even_when_disabled() {
        let doc = r#"
[sources.x]
kind = "rss"
url = "http://example.com/feed.xml"
enabled = false
"#;
        let err = SourcesConfig::from_toml_str(doc).unwrap_err();
        assert!(err.to_string().contains("https"), "{err}");
    }

    #[test]
    fn duration_grammar() {
        assert_eq!(parse_duration("10s").unwrap(), Duration::from_secs(10));
        assert_eq!(parse_duration("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(parse_duration("1d").unwrap(), Duration::from_secs(86_400));
        for bad in ["10", "s", "", "1w", "0s", "-5m", "1.5h"] {
            assert!(parse_duration(bad).is_err(), "`{bad}` should be rejected");
        }
    }

    #[test]
    fn window_times_require_utc_and_ordering() {
        // Missing Z.
        let doc = r#"
[sources.x]
kind = "nws"
url = "https://api.weather.gov/x"
base_interval = "1h"
rate_budget_per_min = 2
event_windows = [{ days = "d", from = "12:25", to = "12:40Z", interval = "10s" }]
"#;
        let err = SourcesConfig::from_toml_str(doc).unwrap_err();
        assert!(err.to_string().contains("trailing Z"), "{err}");

        // from >= to.
        let doc = r#"
[sources.x]
kind = "nws"
url = "https://api.weather.gov/x"
base_interval = "1h"
rate_budget_per_min = 2
event_windows = [{ days = "d", from = "12:40Z", to = "12:25Z", interval = "10s" }]
"#;
        let err = SourcesConfig::from_toml_str(doc).unwrap_err();
        assert!(err.to_string().contains("from < to"), "{err}");
    }

    #[test]
    fn unknown_kind_rejected() {
        let doc = r#"
[sources.x]
kind = "webhook"
"#;
        let err = SourcesConfig::from_toml_str(doc).unwrap_err();
        assert!(err.to_string().contains("unknown kind"), "{err}");
    }

    #[test]
    fn unknown_field_rejected() {
        // Typo'd field name must not be silently ignored (fail-closed).
        let doc = r#"
[sources.x]
kind = "rss"
url = "https://example.com/feed.xml"
base_interval = "30m"
rate_budget = 6
"#;
        assert!(SourcesConfig::from_toml_str(doc).is_err());
    }

    #[test]
    fn trust_cap_range_enforced() {
        let doc = r#"
[sources.x]
kind = "scrape"
extraction = "model"
extraction_trust_cap = 11
enabled = false
"#;
        let err = SourcesConfig::from_toml_str(doc).unwrap_err();
        assert!(err.to_string().contains("0..=10"), "{err}");
    }

    #[test]
    fn model_extraction_requires_cap_even_disabled() {
        let doc = r#"
[sources.x]
kind = "scrape"
extraction = "model"
enabled = false
"#;
        let err = SourcesConfig::from_toml_str(doc).unwrap_err();
        assert!(err.to_string().contains("extraction_trust_cap"), "{err}");
    }

    #[test]
    fn zero_rate_budget_rejected() {
        let doc = r#"
[sources.x]
kind = "rss"
url = "https://example.com/feed.xml"
base_interval = "30m"
rate_budget_per_min = 0
"#;
        let err = SourcesConfig::from_toml_str(doc).unwrap_err();
        assert!(err.to_string().contains("must be > 0"), "{err}");
    }
}
