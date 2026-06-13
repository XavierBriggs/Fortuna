//! Source factory (D10): build an [`IngestionScheduler`] with real adapters
//! from the validated `[sources]` config. This is the composition glue the
//! daemon's `drive()` seam uses — it turns config rows into registered,
//! host-pinned, politeness-limited, validator-guarded sources.
//!
//! The factory is the single place that maps `(kind, feed)` → adapter +
//! claimed-time extractor, so adding a source kind touches exactly here and the
//! adapter module — nowhere in the daemon.

use std::sync::Arc;
use std::time::Duration;

use fortuna_cognition::signals::Source;
use fortuna_core::clock::Clock;

use crate::aeolus::{aeolus_claimed_time, AeolusSource};
use crate::calendar::{calendar_claimed_time, CalendarFeed, CalendarSource};
use crate::config::{SourceConfig, SourceKind, SourcesConfig};
use crate::error::SourcesError;
use crate::fetch::{FetchCaps, FetchClient, HostPin, ReqwestFetchTransport};
use crate::nws::{nws_claimed_time, NwsFeed, NwsSource};
use crate::nws_climate::{nws_climate_claimed_time, NwsClimateSource};
use crate::rss::{rss_claimed_time, RssSource};
use crate::scheduler::{ClaimedTimeFn, IngestionScheduler, SourceSchedule};
use crate::validate::StructuralConfig;

/// Knobs the factory applies uniformly (the daemon supplies them).
#[derive(Debug, Clone)]
pub struct FactoryConfig {
    /// HTTP request timeout per fetch.
    pub fetch_timeout: Duration,
    /// User-Agent every request sends (identify the app; include contact).
    pub user_agent: String,
    /// Global trigger floor: tier >= floor may wake a decision cycle.
    pub trigger_floor: u8,
    /// Per-tick accepted-item envelope (the AFD-firehose containment, §7).
    pub volume_envelope: usize,
}

/// Build a scheduler from config. `tier_of` supplies each source's trust tier
/// from the `source_registry` (fail-closed: a source with no registry tier is
/// refused — it must be admitted before it runs). `domain_of` supplies each
/// source's domain tags (weather | macro | …) from the same registry admission
/// (parallel to `tier_of` supplying the tier), surfaced in telemetry.
/// `secret_resolver` maps an env-var NAME (an `auth_env` from config) to its
/// value — so the LIB never reads env directly; the caller (the binary) owns
/// env access (Aeolus contract §3.1, house secrets rule). Only ENABLED,
/// Phase-A-buildable sources are registered.
pub fn build_scheduler(
    config: &SourcesConfig,
    factory: &FactoryConfig,
    tier_of: impl Fn(&str) -> Option<u8>,
    domain_of: impl Fn(&str) -> Vec<String>,
    secret_resolver: impl Fn(&str) -> Option<String>,
    clock: Arc<dyn Clock>,
) -> Result<IngestionScheduler, SourcesError> {
    let mut scheduler = IngestionScheduler::new();
    for (id, src) in &config.sources {
        if !src.enabled {
            continue;
        }
        let tier = tier_of(id).ok_or_else(|| SourcesError::ConfigInvalid {
            source_id: id.clone(),
            reason: "enabled source has no source_registry tier (admit it first)".to_string(),
        })?;
        let domain_tags = domain_of(id);
        let (source, claimed): (Box<dyn Source>, ClaimedTimeFn) =
            build_adapter(id, src, factory, &secret_resolver, clock.clone())?;
        let schedule = source_schedule(src, factory, tier, domain_tags);
        let validator_cfg = StructuralConfig {
            volume_envelope: factory.volume_envelope,
            ..Default::default()
        };
        scheduler.register(id.clone(), source, schedule, claimed, validator_cfg);
    }
    Ok(scheduler)
}

fn build_adapter(
    id: &str,
    src: &SourceConfig,
    factory: &FactoryConfig,
    secret_resolver: &impl Fn(&str) -> Option<String>,
    clock: Arc<dyn Clock>,
) -> Result<(Box<dyn Source>, ClaimedTimeFn), SourcesError> {
    let url = src
        .url
        .as_deref()
        .ok_or_else(|| invalid(id, "missing url"))?;
    let client = build_client(id, url, src, factory, secret_resolver)?;
    let feed = src.feed.as_deref();
    match (src.kind, feed) {
        (SourceKind::Nws, Some("alerts")) => Ok((
            Box::new(NwsSource::new(
                id,
                NwsFeed::AlertsActive,
                url,
                client,
                clock,
            )),
            nws_claimed_time,
        )),
        (SourceKind::Nws, Some("afd")) => Ok((
            Box::new(NwsSource::new(id, NwsFeed::AfdProducts, url, client, clock)),
            nws_claimed_time,
        )),
        (SourceKind::Nws, Some("climate")) => Ok((
            // The observed daily-extreme grader (F2): the CLI products list URL.
            // Two-hop fetch + its own claimed-time (issuanceTime).
            Box::new(NwsClimateSource::new(id, url, client, clock)),
            nws_climate_claimed_time,
        )),
        (SourceKind::Nws, _) => Err(invalid(
            id,
            "nws source requires feed = \"alerts\" | \"afd\" | \"climate\"",
        )),
        (SourceKind::Rss, _) => Ok((
            Box::new(RssSource::new(id, url, client, clock)),
            rss_claimed_time,
        )),
        (SourceKind::Calendar, Some("schedule")) => Ok((
            Box::new(CalendarSource::new(
                id,
                CalendarFeed::Schedule,
                url,
                client,
                clock,
            )),
            calendar_claimed_time,
        )),
        (SourceKind::Calendar, Some("latest")) => Ok((
            Box::new(CalendarSource::new(
                id,
                CalendarFeed::LatestReleases,
                url,
                client,
                clock,
            )),
            calendar_claimed_time,
        )),
        (SourceKind::Calendar, _) => Err(invalid(
            id,
            "calendar source requires feed = \"schedule\" | \"latest\"",
        )),
        (SourceKind::Aeolus, _) => Ok((
            Box::new(AeolusSource::new(id, url, client, clock)),
            aeolus_claimed_time,
        )),
        (SourceKind::Gdelt, _) => Err(invalid(
            id,
            "gdelt adapter not built yet (D7 deferred; use rss against GDELT format=rss)",
        )),
        (SourceKind::Scrape | SourceKind::Mcp, _) => {
            Err(invalid(id, "scrape/mcp not buildable in Phase A"))
        }
    }
}

fn build_client(
    id: &str,
    url: &str,
    src: &SourceConfig,
    factory: &FactoryConfig,
    secret_resolver: &impl Fn(&str) -> Option<String>,
) -> Result<FetchClient<ReqwestFetchTransport>, SourcesError> {
    let pin = HostPin::from_url(url)?;
    let mut transport = ReqwestFetchTransport::new(factory.fetch_timeout, &factory.user_agent)?;
    // Per-source auth header (Aeolus contract §3.1): when config names a header
    // + env var, resolve the SECRET via the caller-supplied resolver (the lib
    // never reads env) and inject it sensitive/redacted. A named env that does
    // not resolve is fail-closed: an authenticated source that cannot find its
    // key must not silently fetch unauthenticated.
    if let (Some(header), Some(env)) = (src.auth_header.as_deref(), src.auth_env.as_deref()) {
        let secret = secret_resolver(env).ok_or_else(|| {
            invalid(
                id,
                &format!(
                    "auth_env `{env}` did not resolve to a secret (set it in the environment)"
                ),
            )
        })?;
        transport = transport.with_auth_header(header, &secret)?;
    } else if src.auth_header.is_some() != src.auth_env.is_some() {
        // Half-configured auth is a config error, not a silent unauthenticated
        // fetch — both or neither.
        return Err(invalid(
            id,
            "auth requires BOTH auth_header and auth_env (set both, or neither)",
        ));
    }
    let budget = src.rate_budget_per_min.unwrap_or(6);
    Ok(FetchClient::new(
        transport,
        pin,
        budget,
        FetchCaps::default(),
    ))
}

fn source_schedule(
    src: &SourceConfig,
    factory: &FactoryConfig,
    tier: u8,
    domain_tags: Vec<String>,
) -> SourceSchedule {
    let base = src.base_interval.unwrap_or(Duration::from_secs(1800));
    // Boost to the tightest configured window interval during any window.
    let boosted = src
        .event_windows
        .iter()
        .map(|w| w.interval)
        .min()
        .unwrap_or(base);
    SourceSchedule {
        base_interval: base,
        event_windows: src.event_windows.clone(),
        boosted_interval: boosted,
        quarantine_after: 5,
        backoff_base: Duration::from_secs(30),
        backoff_cap: Duration::from_secs(3600),
        trust_tier: tier,
        trigger_floor: factory.trigger_floor,
        domain_tags,
    }
}

fn invalid(id: &str, reason: &str) -> SourcesError {
    SourcesError::ConfigInvalid {
        source_id: id.to_string(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fortuna_core::clock::SimClock;
    use fortuna_core::clock::UtcTimestamp;

    fn factory_cfg() -> FactoryConfig {
        FactoryConfig {
            fetch_timeout: Duration::from_secs(20),
            user_agent: "(test, x@example.com)".into(),
            trigger_floor: 5,
            volume_envelope: 512,
        }
    }

    fn clock() -> Arc<dyn Clock> {
        Arc::new(SimClock::new(UtcTimestamp::from_epoch_millis(0).unwrap()))
    }

    const CONFIG: &str = r#"
[sources.nws_alerts]
kind = "nws"
feed = "alerts"
url = "https://api.weather.gov/alerts/active?area=TX"
base_interval = "10m"
rate_budget_per_min = 30

[sources.fed_press]
kind = "rss"
url = "https://www.federalreserve.gov/feeds/press_all.xml"
base_interval = "30m"
rate_budget_per_min = 10

[sources.bls_schedule]
kind = "calendar"
feed = "schedule"
url = "https://www.bls.gov/schedule/news_release/bls.ics"
base_interval = "1d"
rate_budget_per_min = 6

[sources.disabled_one]
kind = "rss"
url = "https://example.com/feed.xml"
base_interval = "30m"
rate_budget_per_min = 6
enabled = false
"#;

    fn tiers(id: &str) -> Option<u8> {
        match id {
            "nws_alerts" => Some(9),
            "fed_press" => Some(9),
            "bls_schedule" => Some(9),
            _ => Some(5),
        }
    }

    /// Test resolver: no secrets needed by the base CONFIG (keyless sources).
    fn no_secrets(_env: &str) -> Option<String> {
        None
    }

    /// Test domain resolver: no domain tags (the existing tests don't care).
    fn no_domains(_id: &str) -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn builds_enabled_sources_skips_disabled() {
        let cfg = SourcesConfig::from_toml_str(CONFIG).unwrap();
        let c = clock();
        let s = build_scheduler(
            &cfg,
            &factory_cfg(),
            tiers,
            no_domains,
            no_secrets,
            c.clone(),
        )
        .unwrap();
        let mut ids = s.source_ids();
        ids.sort_unstable();
        assert_eq!(ids, vec!["bls_schedule", "fed_press", "nws_alerts"]);
    }

    /// F4: the NWS-CLI grader (feed = "climate") and Aeolus are reachable
    /// through the factory and thus REGISTERED in the scheduler — which means
    /// their output is Layer-1 validated (scheduler.register attaches the
    /// StructuralValidator; there is no bypass).
    #[test]
    fn wires_the_climate_grader_and_aeolus() {
        const F4: &str = r#"
[sources.nws_climate]
kind = "nws"
feed = "climate"
url = "https://api.weather.gov/products?type=CLI"
base_interval = "1d"
rate_budget_per_min = 6

[sources.aeolus]
kind = "aeolus"
url = "https://aeolus.example.com/v2/forecasts?station=KNYC&variable=tmax"
base_interval = "6h"
rate_budget_per_min = 10
auth_header = "x-api-key"
auth_env = "AEOLUS_API_TOKEN"
"#;
        let cfg = SourcesConfig::from_toml_str(F4).unwrap();
        let c = clock();
        let secrets = |env: &str| (env == "AEOLUS_API_TOKEN").then(|| "test-token".to_string());
        let tier_of = |id: &str| match id {
            "nws_climate" => Some(10),
            "aeolus" => Some(7),
            _ => None,
        };
        let s = build_scheduler(
            &cfg,
            &factory_cfg(),
            tier_of,
            no_domains,
            secrets,
            c.clone(),
        )
        .unwrap();
        let mut ids = s.source_ids();
        ids.sort_unstable();
        assert_eq!(ids, vec!["aeolus", "nws_climate"]);
    }

    #[test]
    fn enabled_source_without_a_registry_tier_is_refused() {
        let cfg = SourcesConfig::from_toml_str(CONFIG).unwrap();
        let c = clock();
        let err = build_scheduler(
            &cfg,
            &factory_cfg(),
            |_| None,
            no_domains,
            no_secrets,
            c.clone(),
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("no source_registry tier"), "{err}");
    }

    #[test]
    fn enabled_nws_without_feed_is_refused() {
        let cfg = SourcesConfig::from_toml_str(
            r#"
[sources.x]
kind = "nws"
url = "https://api.weather.gov/x"
base_interval = "10m"
rate_budget_per_min = 6
"#,
        )
        .unwrap();
        let c = clock();
        let err = build_scheduler(
            &cfg,
            &factory_cfg(),
            |_| Some(9),
            no_domains,
            no_secrets,
            c.clone(),
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("alerts"), "{err}");
    }

    #[test]
    fn enabled_gdelt_is_refused_with_the_rss_fallback_hint() {
        let cfg = SourcesConfig::from_toml_str(
            r#"
[sources.g]
kind = "gdelt"
url = "https://api.gdeltproject.org/api/v2/doc/doc"
base_interval = "1h"
rate_budget_per_min = 6
"#,
        )
        .unwrap();
        let c = clock();
        let err = build_scheduler(
            &cfg,
            &factory_cfg(),
            |_| Some(4),
            no_domains,
            no_secrets,
            c.clone(),
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("format=rss"), "{err}");
    }

    const AEOLUS_CONFIG: &str = r#"
[sources.aeolus_knyc]
kind = "aeolus"
url = "https://forecasts.aeolus.internal/v2/forecasts?station=KNYC&variable=tmax"
base_interval = "6h"
rate_budget_per_min = 6
auth_header = "x-api-key"
auth_env = "AEOLUS_API_TOKEN"
"#;

    #[test]
    fn builds_aeolus_source_when_secret_resolves() {
        let cfg = SourcesConfig::from_toml_str(AEOLUS_CONFIG).unwrap();
        let c = clock();
        // The caller resolves the env var; the lib never reads env.
        let resolver = |env: &str| (env == "AEOLUS_API_TOKEN").then(|| "secret-key".to_string());
        let s = build_scheduler(
            &cfg,
            &factory_cfg(),
            |_| Some(6),
            no_domains,
            resolver,
            c.clone(),
        )
        .unwrap();
        assert_eq!(s.source_ids(), vec!["aeolus_knyc"]);
    }

    #[test]
    fn aeolus_source_with_unresolved_secret_is_refused() {
        // An authenticated source whose key is absent fails closed (no silent
        // unauthenticated fetch).
        let cfg = SourcesConfig::from_toml_str(AEOLUS_CONFIG).unwrap();
        let c = clock();
        let err = build_scheduler(
            &cfg,
            &factory_cfg(),
            |_| Some(6),
            no_domains,
            |_| None,
            c.clone(),
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("did not resolve"), "{err}");
    }

    #[test]
    fn half_configured_auth_is_refused() {
        // auth_header without auth_env (or vice versa) is a config error.
        let cfg = SourcesConfig::from_toml_str(
            r#"
[sources.x]
kind = "aeolus"
url = "https://forecasts.aeolus.internal/v2/forecasts?station=KNYC&variable=tmax"
base_interval = "6h"
rate_budget_per_min = 6
auth_header = "x-api-key"
"#,
        )
        .unwrap();
        let c = clock();
        let err = build_scheduler(
            &cfg,
            &factory_cfg(),
            |_| Some(6),
            no_domains,
            no_secrets,
            c.clone(),
        )
        .err()
        .unwrap();
        assert!(
            err.to_string().contains("BOTH auth_header and auth_env"),
            "{err}"
        );
    }

    #[test]
    fn domain_of_flows_through_to_telemetry() {
        // The registry-sourced `domain_of` resolver populates each source's
        // domain_tags, which surface in the per-source telemetry projection.
        let cfg = SourcesConfig::from_toml_str(
            r#"
[sources.nws_alerts]
kind = "nws"
feed = "alerts"
url = "https://api.weather.gov/alerts/active?area=TX"
base_interval = "10m"
rate_budget_per_min = 30
"#,
        )
        .unwrap();
        let c = clock();
        let domain_of = |id: &str| match id {
            "nws_alerts" => vec!["weather".to_string()],
            _ => Vec::new(),
        };
        let s = build_scheduler(
            &cfg,
            &factory_cfg(),
            |_| Some(9),
            domain_of,
            no_secrets,
            c.clone(),
        )
        .unwrap();
        let t = s.telemetry(UtcTimestamp::from_epoch_millis(0).unwrap());
        let src = t
            .sources
            .iter()
            .find(|st| st.source_id == "nws_alerts")
            .expect("source present in telemetry");
        assert_eq!(src.domain_tags, vec!["weather"]);
    }
}
