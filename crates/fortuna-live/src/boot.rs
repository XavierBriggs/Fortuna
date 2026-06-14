//! Fail-closed boot validation (T4.1 hard requirement 1).
//!
//! Pure functions over CALLER-SUPPLIED data: the env arrives as a map, the
//! config as a string. Nothing here reads the process environment, the
//! filesystem, or a clock — the binary (main.rs) gathers those and is the
//! only place allowed to.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BootError {
    #[error("required env var {var} is not set")]
    MissingEnv { var: String },
    #[error("env var {var} holds a placeholder value ({hint}); refusing to boot")]
    PlaceholderEnv { var: String, hint: String },
    #[error("config rejected: {reason}")]
    BadConfig { reason: String },
    #[error("venue {venue} cannot boot: {reason}")]
    VenueNotBootable { venue: String, reason: String },
}

/// A secret value: usable by the composition, redacted in Debug/Display.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap a secret value (crate-internal: compose code wraps env-sourced
    /// secrets like the Kalshi demo PEM so they redact in Debug/Display).
    pub(crate) fn new(value: String) -> Secret {
        Secret(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<REDACTED>")
    }
}

/// The validated env surface the daemon boots from. Channel IDs are
/// routing data, not secrets; everything else redacts.
#[derive(Debug)]
pub struct RequiredEnv {
    pub database_url: Secret,
    /// Absent key is RECORDED here; whether the daemon may run with a
    /// stub mind is a config decision ([cognition] allow_stub_mind),
    /// enforced at compose time — recording is not deciding.
    pub anthropic_api_key: Option<Secret>,
    pub slack_bot_token: Secret,
    /// Channel name (lowercase, e.g. "trading") -> channel id.
    pub slack_channels: BTreeMap<String, String>,
    pub deadman_url: Secret,
}

const SLACK_CHANNELS: [(&str, &str); 5] = [
    ("trading", "FORTUNA_SLACK_CHANNEL_TRADING"),
    ("alerts", "FORTUNA_SLACK_CHANNEL_ALERTS"),
    ("review", "FORTUNA_SLACK_CHANNEL_REVIEW"),
    ("digest", "FORTUNA_SLACK_CHANNEL_DIGEST"),
    ("ops", "FORTUNA_SLACK_CHANNEL_OPS"),
];

/// The exact ways a half-edited .env shows up (lowercased substring
/// match). Includes the literal .env.example placeholder spellings; the
/// gitignore/.keys incident proves half-configured states reach disk.
const PLACEHOLDER_MARKS: [&str; 6] = [
    "replace",
    "changeme",
    "your-",
    "your_",
    "<",
    "user:password",
];

pub(crate) fn check_value(var: &str, value: &str) -> Result<String, BootError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(BootError::PlaceholderEnv {
            var: var.to_string(),
            hint: "empty".to_string(),
        });
    }
    let lower = trimmed.to_ascii_lowercase();
    for mark in PLACEHOLDER_MARKS {
        if lower.contains(mark) {
            return Err(BootError::PlaceholderEnv {
                var: var.to_string(),
                hint: format!("contains {mark:?}"),
            });
        }
    }
    Ok(trimmed.to_string())
}

pub(crate) fn required(env: &BTreeMap<String, String>, var: &str) -> Result<String, BootError> {
    let value = env.get(var).ok_or_else(|| BootError::MissingEnv {
        var: var.to_string(),
    })?;
    check_value(var, value)
}

/// Validate the daemon's env surface. Missing or placeholder values
/// refuse with the offending VAR NAME; values never enter the error.
pub fn validate_env(env: &BTreeMap<String, String>) -> Result<RequiredEnv, BootError> {
    let database_url = Secret(required(env, "DATABASE_URL")?);
    let slack_bot_token = Secret(required(env, "FORTUNA_SLACK_BOT_TOKEN")?);
    let mut slack_channels = BTreeMap::new();
    for (name, var) in SLACK_CHANNELS {
        slack_channels.insert(name.to_string(), required(env, var)?);
    }
    let deadman_url = Secret(required(env, "FORTUNA_DEADMAN_URL")?);
    let anthropic_api_key = match env.get("ANTHROPIC_API_KEY") {
        None => None,
        Some(v) => Some(Secret(check_value("ANTHROPIC_API_KEY", v)?)),
    };
    Ok(RequiredEnv {
        database_url,
        anthropic_api_key,
        slack_bot_token,
        slack_channels,
        deadman_url,
    })
}

/// The `[daemon]` config section (T4.1; strict on what the daemon reads).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonSection {
    pub venue: String,
    /// The validation stage the daemon runs at (spec Section 11, I7). Default
    /// `"sim"` for back-compat (every committed config predating the demo-flip
    /// omits it and means Sim). The boot gate cross-checks this against `venue`:
    /// `venue = "sim"` REQUIRES `stage = "sim"`, and the Kalshi demo runs ONLY
    /// at `stage = "paper"` (LiveMin/Scaled are refused — promotion is a human
    /// action through the forward-validation gate, I7).
    #[serde(default = "default_stage")]
    pub stage: String,
    pub tick_interval_ms: u64,
    pub halt_poll_ms: u64,
    pub metrics_bind: String,
}

/// The default validation stage when `[daemon].stage` is omitted: `"sim"`.
/// Back-compat — pre-demo-flip configs have no `stage` field and mean Sim.
fn default_stage() -> String {
    "sim".into()
}

/// The `[kalshi]` config section (demo-flip Phase 2). Its PRESENCE is required
/// for the Kalshi demo (`venue = "kalshi", stage = "paper"`); `series` is the
/// trading universe `KalshiVenue` lists markets for, and `bracket_sets` is the
/// mech_structural arb world (mirrors `[sim].bracket_sets`). Demo credentials
/// (`KALSHI_API_DEMO_KEY_ID` + `KALSHI_DEMO_PRIVATE_KEY_PATH`) come from the
/// environment, NEVER this section (house secrets rule).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KalshiSection {
    /// Series tickers FORTUNA trades; `KalshiVenue::markets()` is scoped to
    /// these (an empty list yields an empty catalog — refused at boot).
    pub series: Vec<String>,
    /// Bracket sets for mech_structural (the demo's arb world), each a list of
    /// market tickers that partition one event (mirrors `[sim].bracket_sets`).
    pub bracket_sets: Vec<Vec<String>>,
}

/// The `[cognition]` section as the daemon reads it (other consumers may
/// read more; unknown fields here are tolerated for that reason).
#[derive(Debug, Clone, Deserialize)]
pub struct CognitionSection {
    pub daily_budget_cents: i64,
    pub per_cycle_budget_cents: i64,
    /// Booting without ANTHROPIC_API_KEY silently swaps in the stub mind
    /// (mind_from_env contract). That degrade must be OPTED INTO.
    #[serde(default)]
    pub allow_stub_mind: bool,
    /// The SYNTHESIS tier (spec 5.9): the deep belief-formation model (Opus).
    /// `mind_from_env` feeds it to AnthropicMindConfig.model when a key is present.
    /// The model is config; the API KEY is env-only (never here). NAME matches the
    /// committed example's `[cognition].synthesis_model` — a mismatched/misspelled
    /// name would silently drop the operator's choice to the default (a test
    /// asserts the parsed value to catch exactly that).
    #[serde(default = "default_synthesis_model")]
    pub synthesis_model: String,
    /// The MID tier (spec 5.9): the daily RECONCILIATION (and a natural home for
    /// the reviews) runs here — a capable but cheaper model than the synthesis
    /// Opus (Sonnet). A REAL field with its OWN `mind_from_env`, NOT a clone of the
    /// synthesis mind; a misspelled `mid_model` drops to the default, so a test
    /// asserts the parsed value.
    #[serde(default = "default_mid_model")]
    pub mid_model: String,
    /// The TRIAGE/trigger tier (spec 5.9): the fast, cheap model (Haiku). A REAL
    /// field — it was present in the example TOML but UNREAD (tolerated-and-
    /// dropped); now parsed so a misspelled key is caught, not silently defaulted.
    #[serde(default = "default_triage_model")]
    pub triage_model: String,
}

/// The three cognition tiers (spec 5.9). Each is overridable per `[cognition]`;
/// a config that omits one falls back here. The model is config; the API KEY is
/// env-only.
fn default_synthesis_model() -> String {
    "claude-opus-4-8".to_string()
}

fn default_mid_model() -> String {
    "claude-sonnet-4-6".to_string()
}

fn default_triage_model() -> String {
    "claude-haiku-4-5".to_string()
}

impl CognitionSection {
    /// The spec-5.9 tier → model registry: the single source of truth the daemon
    /// consults to build each role's mind on its tier's model (synthesis/mid/triage).
    pub fn model_registry(&self) -> fortuna_cognition::mind::ModelRegistry {
        fortuna_cognition::mind::ModelRegistry::new(
            self.synthesis_model.clone(),
            self.mid_model.clone(),
            self.triage_model.clone(),
        )
    }
}

/// The `[sim]` section: the synthetic market world the Sim-venue daemon
/// trades (the EXIT criterion's continuous week runs over these).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimSection {
    /// Bracket sets for mech_structural; every named market is created
    /// on the sim venue as a weather-category $1-payout bracket.
    pub bracket_sets: Vec<Vec<String>>,
}

/// The `[ingestion]` section (D10, opt-in, default OFF). Its PRESENCE with
/// `enabled = true` spawns the news-aggregation ingestion loop alongside the
/// trading daemon (the Layer-1 validator runs live on the ingest path). Absent
/// or `enabled = false` => no ingestion (fail closed; the daemon is unchanged).
/// The actual source set is the `[sources.<id>]` tables, parsed separately by
/// `fortuna_sources::SourcesConfig`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestionSection {
    pub enabled: bool,
    /// Cadence the ingestion loop ticks at (ms); the scheduler skips not-due
    /// sources internally, so this is a polling granularity, not a per-source
    /// interval. Default 5000.
    #[serde(default = "default_ingestion_tick_ms")]
    pub tick_ms: u64,
    /// Global trigger floor: tier >= floor may wake a decision cycle. Default 5.
    #[serde(default = "default_trigger_floor")]
    pub trigger_floor: u8,
    /// Per-tick accepted-item envelope (the AFD-firehose containment). Default 512.
    #[serde(default = "default_volume_envelope")]
    pub volume_envelope: usize,
    /// User-Agent every fetch sends (identify the app + contact).
    pub user_agent: String,
}

fn default_ingestion_tick_ms() -> u64 {
    5000
}
fn default_trigger_floor() -> u8 {
    5
}
fn default_volume_envelope() -> usize {
    512
}

#[derive(Debug, Deserialize)]
struct RawToml {
    daemon: Option<DaemonSection>,
    cognition: Option<CognitionSection>,
    sim: Option<SimSection>,
    kalshi: Option<KalshiSection>,
    synthesis: Option<crate::compose::SynthesisSection>,
    mech_extremes: Option<crate::compose::MechExtremesSection>,
    funding_forecast: Option<crate::compose::FundingForecastSection>,
    perp_event_basis: Option<crate::compose::PerpEventBasisSection>,
    review: Option<crate::compose::ReviewSection>,
    ingestion: Option<IngestionSection>,
}

/// The parsed daemon-relevant config.
#[derive(Debug, Clone)]
pub struct DaemonToml {
    pub daemon: DaemonSection,
    pub cognition: CognitionSection,
    pub sim: Option<SimSection>,
    /// Optional `[kalshi]` section (demo-flip Phase 2). REQUIRED (non-empty
    /// `series`) when `venue = "kalshi", stage = "paper"`; absent/empty for the
    /// Sim daemon. Carries the demo trading universe + bracket arb world; the
    /// demo CREDENTIALS are env-only, never here.
    pub kalshi: Option<KalshiSection>,
    /// Optional `[synthesis]` opt-in. Its PRESENCE composes the synthesis
    /// strategy into the daemon (S3b); its fields only FILTER the confirmed
    /// edge set. Absent => the daemon runs mechanically-only (fail closed).
    pub synthesis: Option<crate::compose::SynthesisSection>,
    /// Optional `[mech_extremes]` opt-in. Its PRESENCE composes the
    /// favorite-longshot fade strategy (spec Section 6) enrolled in the
    /// reduce-only model veto. Absent => not composed (fail closed).
    pub mech_extremes: Option<crate::compose::MechExtremesSection>,
    /// Optional `[funding_forecast]` opt-in (slice 4c). Its PRESENCE composes
    /// the zero-capital perp funding belief-producer (propose-nothing). Absent
    /// => not composed (fail closed).
    pub funding_forecast: Option<crate::compose::FundingForecastSection>,
    /// Optional `[perp_event_basis]` opt-in (slice 4c). Its PRESENCE composes
    /// the propose-only perp/bracket basis strategy over the config-supplied
    /// ladder. Absent => not composed (fail closed).
    pub perp_event_basis: Option<crate::compose::PerpEventBasisSection>,
    /// Optional `[review]` opt-in (T4.1/M2). Its PRESENCE composes the weekly/
    /// monthly review cadence (GO/NO-GO thresholds; advisory only, I7). Absent
    /// => no review fires (fail closed).
    pub review: Option<crate::compose::ReviewSection>,
    /// Optional `[ingestion]` opt-in (D10, default OFF). `enabled = true`
    /// spawns the news-aggregation ingestion loop (validator live on the
    /// ingest path). Absent / `enabled = false` => no ingestion.
    pub ingestion: Option<IngestionSection>,
}

impl DaemonToml {
    /// Parse the daemon-relevant sections out of fortuna.toml text.
    /// Missing sections are precise refusals, not defaults (fail-closed).
    pub fn parse(text: &str) -> Result<DaemonToml, BootError> {
        let raw: RawToml = toml::from_str(text).map_err(|e| BootError::BadConfig {
            reason: e.to_string(),
        })?;
        let daemon = raw.daemon.ok_or_else(|| BootError::BadConfig {
            reason: "missing [daemon] section (venue, tick_interval_ms, halt_poll_ms, \
                     metrics_bind are required)"
                .to_string(),
        })?;
        let cognition = raw.cognition.ok_or_else(|| BootError::BadConfig {
            reason: "missing [cognition] section (daily_budget_cents, \
                     per_cycle_budget_cents are required)"
                .to_string(),
        })?;
        Ok(DaemonToml {
            daemon,
            cognition,
            sim: raw.sim,
            kalshi: raw.kalshi,
            synthesis: raw.synthesis,
            mech_extremes: raw.mech_extremes,
            funding_forecast: raw.funding_forecast,
            perp_event_basis: raw.perp_event_basis,
            review: raw.review,
            ingestion: raw.ingestion,
        })
    }

    /// Boot checks beyond parsing: venue gating and operational pins.
    pub fn validate_bootable(&self) -> Result<(), BootError> {
        match self.daemon.venue.as_str() {
            "sim" => {
                // The Sim venue runs ONLY at Stage::Sim — a "sim"+non-sim-stage
                // config is a mis-wiring (e.g. someone set stage = "paper" but
                // left venue = "sim"); refuse it rather than silently running the
                // sim world at a promoted stage.
                if self.daemon.stage != "sim" {
                    return Err(BootError::BadConfig {
                        reason: format!(
                            "venue = \"sim\" requires stage = \"sim\" (got stage = {:?}); the \
                             Sim venue does not run at a promoted stage",
                            self.daemon.stage
                        ),
                    });
                }
                let empty = self
                    .sim
                    .as_ref()
                    .map(|s| s.bracket_sets.is_empty() || s.bracket_sets.iter().any(Vec::is_empty))
                    .unwrap_or(true);
                if empty {
                    return Err(BootError::BadConfig {
                        reason: "venue = \"sim\" requires a [sim] section with non-empty \
                                 bracket_sets (the market world the daemon trades)"
                            .to_string(),
                    });
                }
            }
            "kalshi" => match self.daemon.stage.as_str() {
                // The demo runs at Paper: mock funds, real venue, pre-promotion.
                // It REQUIRES a [kalshi] section with a non-empty series list
                // (the trading universe KalshiVenue lists markets for; an empty
                // catalog would be a silently-inert daemon). Credentials are
                // env-only, gated later in compose_kalshi_runner (not here — the
                // boot gate is pure over config, never reads the environment).
                "paper" => {
                    let series_ok = self
                        .kalshi
                        .as_ref()
                        .map(|k| !k.series.is_empty())
                        .unwrap_or(false);
                    if !series_ok {
                        return Err(BootError::BadConfig {
                            reason: "venue = \"kalshi\", stage = \"paper\" requires a [kalshi] \
                                     section with a non-empty series list (the demo trading \
                                     universe)"
                                .to_string(),
                        });
                    }
                }
                // Sim stage on the Kalshi venue is a mis-wiring: the sim world is
                // venue = "sim". The Kalshi adapter only boots at Paper here.
                "sim" => {
                    return Err(BootError::VenueNotBootable {
                        venue: "kalshi".to_string(),
                        reason: "venue=kalshi requires stage=paper (the Kalshi demo runs at \
                                 Stage::Paper; the Sim world is venue=\"sim\")"
                            .to_string(),
                    });
                }
                // Live promotion (real funds) is NOT a config flip: it needs the
                // forward-validation gate (I7), an operator action out of band.
                "live_min" | "scaled" => {
                    return Err(BootError::VenueNotBootable {
                        venue: "kalshi".to_string(),
                        reason: format!(
                            "venue=kalshi stage={:?} is refused: promotion past Paper needs the \
                             forward-validation gate (a human action, I7) — the daemon never \
                             auto-promotes to live capital",
                            self.daemon.stage
                        ),
                    });
                }
                other => {
                    return Err(BootError::BadConfig {
                        reason: format!(
                            "venue=kalshi has unknown stage {other:?} (expected sim/paper/\
                             live_min/scaled)"
                        ),
                    });
                }
            },
            other => {
                return Err(BootError::VenueNotBootable {
                    venue: other.to_string(),
                    reason: "unknown venue; sim and kalshi are the only known venues".to_string(),
                });
            }
        }
        if self.daemon.halt_poll_ms == 0 || self.daemon.halt_poll_ms > 500 {
            return Err(BootError::BadConfig {
                reason: format!(
                    "halt_poll_ms = {} violates the <=500ms halt-poll pin (ASSUMPTIONS)",
                    self.daemon.halt_poll_ms
                ),
            });
        }
        if self.daemon.tick_interval_ms == 0 {
            return Err(BootError::BadConfig {
                reason: "tick_interval_ms must be positive".to_string(),
            });
        }
        if self
            .daemon
            .metrics_bind
            .parse::<std::net::SocketAddr>()
            .is_err()
        {
            return Err(BootError::BadConfig {
                reason: format!(
                    "metrics_bind {:?} is not a socket address",
                    self.daemon.metrics_bind
                ),
            });
        }
        if self.cognition.daily_budget_cents <= 0 || self.cognition.per_cycle_budget_cents <= 0 {
            return Err(BootError::BadConfig {
                reason: "cognition budgets must be positive".to_string(),
            });
        }
        Ok(())
    }
}
