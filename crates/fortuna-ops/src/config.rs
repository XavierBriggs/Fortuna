//! Whole-shape loader for `config/fortuna.toml` plus env-only [`Secrets`].
//!
//! Spec Section 8: "Config in TOML, secrets in environment files outside the
//! repo." The committed shape is `config/fortuna.example.toml`; this loader
//! deserializes the WHOLE file so the runner hands one validated artifact to
//! every layer. Unknown top-level tables are ignored (other layers may add
//! sections without breaking ops), and `[fees]` is kept as an opaque
//! `toml::Value` passthrough — the venues layer owns its parsing and
//! verification (spec 5.2: fee schedules are data, verified per fill).
//!
//! Fail-closed: any parse or validation failure yields an error, never a
//! partially-usable config.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use fortuna_gates::GateConfig;
use serde::de::{Deserializer, Error as DeError};
use serde::Deserialize;

use crate::OpsError;

/// Slack bot token (`xoxb-…`); Web API posting only (research doc token
/// model: it cannot open Socket Mode connections).
pub const ENV_SLACK_BOT_TOKEN: &str = "FORTUNA_SLACK_BOT_TOKEN";
/// Dead-man monitor ping URL. The URL itself is a secret (monitor ping URLs
/// embed an unguessable id).
pub const ENV_DEADMAN_URL: &str = "FORTUNA_DEADMAN_URL";
/// Postgres connection string for the ledger layer.
pub const ENV_DATABASE_URL: &str = "DATABASE_URL";
/// Prefix for per-channel Slack channel-id variables.
pub const ENV_SLACK_CHANNEL_PREFIX: &str = "FORTUNA_SLACK_CHANNEL_";

/// `FORTUNA_SLACK_CHANNEL_<NAME-UPPERCASED>` for a configured channel name.
///
/// The name is uppercased verbatim with no other normalization, so config
/// channel names should stay lower_snake (`[a-z0-9_]`) to produce valid
/// env-var names.
pub fn slack_channel_env_var(channel_name: &str) -> String {
    format!("{ENV_SLACK_CHANNEL_PREFIX}{}", channel_name.to_uppercase())
}

/// The whole `config/fortuna.toml` shape. Construct via [`FortunaConfig::load_str`]
/// or [`FortunaConfig::load_file`] (both validate; there is no unvalidated
/// constructor). The file never contains secrets — see [`Secrets`].
#[derive(Debug, Clone, Deserialize)]
pub struct FortunaConfig {
    /// `[gates]` — deserialized into the gates crate's own config type and
    /// validated with its own `validate()` (I1: gates are config-driven).
    pub gates: GateConfig,
    /// `[envelopes]` (spec 5.14: the ONLY capital allocation tier).
    ///
    /// DOCUMENTED CHOICE: keys are stored with any `_cents` suffix STRIPPED
    /// (`mech_structural_cents = 300_000` => `"mech_structural" -> 300_000`)
    /// so envelope keys align with strategy names in `[gates.per_strategy]`.
    /// Values remain integer cents. Two raw keys that collide after
    /// stripping are rejected at parse time.
    #[serde(deserialize_with = "de_envelopes")]
    pub envelopes: BTreeMap<String, i64>,
    /// `[sizing]`.
    pub sizing: SizingConfig,
    /// `[fees]` — opaque passthrough; the venues layer parses and verifies
    /// its own section. Ops does not validate its contents.
    pub fees: toml::Value,
    /// `[slack]`.
    pub slack: SlackConfig,
    /// `[cognition]`.
    pub cognition: CognitionConfig,
    /// `[deadman]`.
    pub deadman: DeadmanConfig,
}

/// `[sizing]`. `kelly_fraction` is a probability-space scalar, not money, so
/// `f64` is permitted here.
#[derive(Debug, Clone, Deserialize)]
pub struct SizingConfig {
    pub kelly_fraction: f64,
}

/// `[slack]`: channel NAMES only. Channel IDs are secrets-adjacent runtime
/// values and come from env (`FORTUNA_SLACK_CHANNEL_<NAME>`); the research
/// doc mandates configuring by immutable `C…` id, not display name.
#[derive(Debug, Clone, Deserialize)]
pub struct SlackConfig {
    pub channels: Vec<String>,
}

/// `[cognition]`.
#[derive(Debug, Clone, Deserialize)]
pub struct CognitionConfig {
    pub synthesis_model: String,
    pub triage_model: String,
    pub daily_budget_cents: i64,
    pub shadow_budget_cents: i64,
}

/// `[deadman]`. Spec Section 8: ping an external monitor every minute.
#[derive(Debug, Clone, Deserialize)]
pub struct DeadmanConfig {
    pub ping_interval_secs: u64,
}

fn de_envelopes<'de, D>(deserializer: D) -> Result<BTreeMap<String, i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = BTreeMap::<String, i64>::deserialize(deserializer)?;
    let mut stripped = BTreeMap::new();
    for (key, cents) in raw {
        let name = key.strip_suffix("_cents").unwrap_or(&key).to_string();
        if name.is_empty() {
            return Err(D::Error::custom(format!(
                "[envelopes] key {key:?} is empty after stripping the _cents suffix"
            )));
        }
        if stripped.insert(name.clone(), cents).is_some() {
            return Err(D::Error::custom(format!(
                "[envelopes] keys {name:?} and \"{name}_cents\" collide after stripping the _cents suffix"
            )));
        }
    }
    Ok(stripped)
}

impl FortunaConfig {
    /// Parse and validate a whole config from a TOML string.
    pub fn load_str(toml_str: &str) -> Result<Self, OpsError> {
        let config: FortunaConfig = toml::from_str(toml_str).map_err(|e| OpsError::Config {
            reason: e.to_string(),
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Parse and validate a whole config from a file path.
    pub fn load_file(path: impl AsRef<Path>) -> Result<Self, OpsError> {
        let raw = std::fs::read_to_string(path)?;
        Self::load_str(&raw)
    }

    fn validate(&self) -> Result<(), OpsError> {
        let err = |reason: String| Err(OpsError::Config { reason });

        self.gates.validate().map_err(|e| OpsError::Config {
            reason: e.to_string(),
        })?;

        // (0, 1]: full Kelly is the ceiling; zero or negative sizing is a
        // misconfiguration, and NaN fails both comparisons (fail-closed).
        let f = self.sizing.kelly_fraction;
        if !(f > 0.0 && f <= 1.0) {
            return err(format!("sizing.kelly_fraction must be in (0, 1], got {f}"));
        }

        for (name, cents) in &self.envelopes {
            if *cents <= 0 {
                return err(format!(
                    "envelopes.{name} must be positive cents, got {cents}"
                ));
            }
        }

        if self.slack.channels.is_empty() {
            return err("slack.channels must not be empty".to_string());
        }
        let mut seen = BTreeSet::new();
        for channel in &self.slack.channels {
            if channel.is_empty() {
                return err("slack.channels entries must be non-empty".to_string());
            }
            if !seen.insert(channel) {
                return err(format!("slack.channels contains duplicate {channel:?}"));
            }
        }

        if self.cognition.synthesis_model.is_empty() || self.cognition.triage_model.is_empty() {
            return err("cognition model names must be non-empty".to_string());
        }
        if self.cognition.daily_budget_cents < 0 || self.cognition.shadow_budget_cents < 0 {
            return err("cognition budgets must be non-negative cents".to_string());
        }

        if self.deadman.ping_interval_secs == 0 {
            return err("deadman.ping_interval_secs must be >= 1".to_string());
        }

        Ok(())
    }
}

/// Secrets, read exclusively from the environment (never from config files,
/// never logged). All fields are optional at construction; call
/// [`Secrets::require`] at the point of use so each binary demands only the
/// secrets it actually needs.
///
/// Empty-string values are treated as MISSING: an empty token or URL can
/// never be valid, and treating it as present would defer the failure to a
/// confusing venue-side error.
///
/// The `Debug` impl redacts every value (names/presence stay visible for
/// operability). There are intentionally no serde derives on this type.
#[derive(Clone)]
pub struct Secrets {
    slack_bot_token: Option<String>,
    /// Configured channel name -> Slack channel id (`C…`). Only channels
    /// whose env var was present land here.
    slack_channel_ids: BTreeMap<String, String>,
    deadman_url: Option<String>,
    database_url: Option<String>,
}

impl Secrets {
    /// Read from the process environment: `FORTUNA_SLACK_BOT_TOKEN`,
    /// `FORTUNA_SLACK_CHANNEL_<NAME-UPPERCASED>` for each configured channel,
    /// `FORTUNA_DEADMAN_URL`, `DATABASE_URL`.
    pub fn from_env(slack: &SlackConfig) -> Self {
        Self::from_lookup(slack, |name| std::env::var(name).ok())
    }

    /// Deterministic constructor over an arbitrary lookup (tests inject a
    /// map here; `from_env` injects `std::env::var`).
    pub fn from_lookup(slack: &SlackConfig, lookup: impl Fn(&str) -> Option<String>) -> Self {
        let non_empty = |name: &str| lookup(name).filter(|value| !value.is_empty());
        let mut slack_channel_ids = BTreeMap::new();
        for channel in &slack.channels {
            if let Some(id) = non_empty(&slack_channel_env_var(channel)) {
                slack_channel_ids.insert(channel.clone(), id);
            }
        }
        Secrets {
            slack_bot_token: non_empty(ENV_SLACK_BOT_TOKEN),
            slack_channel_ids,
            deadman_url: non_empty(ENV_DEADMAN_URL),
            database_url: non_empty(ENV_DATABASE_URL),
        }
    }

    /// Look up a secret by its environment-variable name.
    pub fn get(&self, name: &str) -> Option<&str> {
        match name {
            ENV_SLACK_BOT_TOKEN => self.slack_bot_token.as_deref(),
            ENV_DEADMAN_URL => self.deadman_url.as_deref(),
            ENV_DATABASE_URL => self.database_url.as_deref(),
            other => other
                .strip_prefix(ENV_SLACK_CHANNEL_PREFIX)
                .and_then(|suffix| {
                    self.slack_channel_ids.iter().find_map(|(channel, id)| {
                        (channel.to_uppercase() == suffix).then_some(id.as_str())
                    })
                }),
        }
    }

    /// Like [`Secrets::get`] but missing secrets become a typed
    /// [`OpsError::MissingSecret`] carrying the env-var name (never a value).
    pub fn require(&self, name: &str) -> Result<&str, OpsError> {
        self.get(name).ok_or_else(|| OpsError::MissingSecret {
            name: name.to_string(),
        })
    }

    /// Channel name -> channel id map for [`crate::SlackRouter`] construction.
    pub fn slack_channel_ids(&self) -> &BTreeMap<String, String> {
        &self.slack_channel_ids
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const REDACTED: &str = "<redacted>";
        fn mask(value: &Option<String>) -> &'static str {
            if value.is_some() {
                "Some(<redacted>)"
            } else {
                "None"
            }
        }
        let channels: BTreeMap<&String, &'static str> = self
            .slack_channel_ids
            .keys()
            .map(|name| (name, REDACTED))
            .collect();
        f.debug_struct("Secrets")
            .field("slack_bot_token", &mask(&self.slack_bot_token))
            .field("slack_channel_ids", &channels)
            .field("deadman_url", &mask(&self.deadman_url))
            .field("database_url", &mask(&self.database_url))
            .finish()
    }
}
