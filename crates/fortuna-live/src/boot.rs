//! Fail-closed boot validation (T4.1 hard requirement 1).
//!
//! Pure functions over CALLER-SUPPLIED data: the env arrives as a map, the
//! config as a string. Nothing here reads the process environment, the
//! filesystem, or a clock — the binary (main.rs) gathers those and is the
//! only place allowed to.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::Path;
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
    /// Optional explicit data source for composite modes. Absent preserves the
    /// legacy matrix (`venue=sim` or `venue=kalshi` demo). `kalshi_prod` is
    /// read-only live Kalshi production data and must be paired with
    /// `execution = "paper"`.
    #[serde(default)]
    pub data_source: Option<String>,
    /// Optional explicit execution target for composite modes. Absent preserves
    /// legacy execution semantics. For `data_source = "kalshi_prod"`, only
    /// `paper` is bootable; real Kalshi execution remains refused by I7.
    #[serde(default)]
    pub execution: Option<String>,
    pub tick_interval_ms: u64,
    pub halt_poll_ms: u64,
    pub metrics_bind: String,
}

/// Explicit runtime mutation mode. This deliberately sits beside the older
/// `[daemon]` composition fields so boot can reject ambiguous Kalshi configs
/// before any transport or runner is constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    LiveDataOnly,
    DryRun,
    PaperLedger,
    DemoOrders,
    ProductionOrders,
}

impl ExecutionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ExecutionMode::LiveDataOnly => "live_data_only",
            ExecutionMode::DryRun => "dry_run",
            ExecutionMode::PaperLedger => "paper_ledger",
            ExecutionMode::DemoOrders => "demo_orders",
            ExecutionMode::ProductionOrders => "production_orders",
        }
    }

    pub fn allows_order_mutation(self) -> bool {
        matches!(
            self,
            ExecutionMode::PaperLedger
                | ExecutionMode::DemoOrders
                | ExecutionMode::ProductionOrders
        )
    }

    /// Whether the daemon may AUTO-PERSIST a fitted calibration set (Task B1 /
    /// F0). PaperLedger ONLY: live data + local paper execution is the one mode
    /// that warms the synthesis arm's calibration from its own resolved record.
    /// Every other mode (live-data-only, dry-run, demo, production) persists
    /// NOTHING — calibration in those modes is an operator/promotion action, so
    /// the daemon never advances a calibration version on its own (I7). Expressed
    /// as ExecutionMode MEMBERSHIP so a new variant defaults to refusing.
    pub fn auto_persist_calibration(self) -> bool {
        matches!(self, ExecutionMode::PaperLedger)
    }
}

/// The explicit runtime section required for any Kalshi composition.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSection {
    pub stage: String,
    pub execution_mode: ExecutionMode,
    pub orders_enabled: bool,
    #[serde(default)]
    pub production_unlock: bool,
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
    /// CLV LiquidityPolicy: minimum book-touch quantity to count a snapshot as
    /// liquid enough to serve as a CLV benchmark. Default: 1 (spec minimum;
    /// the book-gate touch minimum). Promoted from a hardcoded const (WS1/W6b).
    #[serde(default = "default_clv_min_touch_qty")]
    pub clv_min_touch_qty: i64,
    /// CLV LiquidityPolicy: maximum bid-ask spread in cents for a snapshot to
    /// serve as a tight CLV benchmark. Default: 10c (keeps the CLV signal tight).
    /// Promoted from a hardcoded const (WS1/W6b).
    #[serde(default = "default_clv_max_spread_cents")]
    pub clv_max_spread_cents: i64,
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

fn default_clv_min_touch_qty() -> i64 {
    1
}

fn default_clv_max_spread_cents() -> i64 {
    10
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

/// The `[killswitch]` section (I4 revocation, open audit C2; default-OFF). When
/// `revocation_file` is set, the daemon wraps its `PgHaltPoller` in a
/// `RevocationHaltPoller` over that path: a kill sentinel WRITTEN by the
/// standalone `fortuna-killswitch` (the `KILLSWITCH_REVOKED` sibling of its
/// `--journal`) becomes a global halt that refuses FUTURE order placement until
/// the operator clears it out-of-band and RESTARTS (I2). Absent => no wrap (the
/// daemon is byte-identical to today — fail closed). The path is the OPERATOR's
/// responsibility to keep equal to the killswitch's sentinel path (ASSUMPTIONS).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KillswitchSection {
    /// Filesystem path of the kill sentinel to watch. MUST equal the
    /// `KILLSWITCH_REVOKED` sibling of the killswitch `--journal` path (i.e.
    /// `revocation_path(<journal>)`). Absent => the daemon does not watch any
    /// sentinel (the revocation poller is not wired).
    pub revocation_file: Option<String>,
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

/// One registered persona in the opt-in `[[personas.persona]]` array: its id,
/// the directory holding `persona.md` + `schema.json`, and the operator cadences
/// (a persona does NOT carry its own cadences — they are trigger config, design
/// §7). The cadence shape is `fortuna_cognition::persona_trigger::Cadence`
/// (validated at boot — an unknown/never-firing cadence is a startup rejection).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaEntryConfig {
    pub id: String,
    pub dir: String,
    pub cadences: Vec<fortuna_cognition::persona_trigger::Cadence>,
}

/// The `[personas]` section (opt-in, default OFF). Its PRESENCE with
/// `enabled = true` wires the persona-analysis step into `drive()`; absent or
/// `enabled = false` => the daemon is byte-identical to today (fail closed).
/// `#[serde(default)]` so omitting the WHOLE section deserializes to the
/// disabled default, and the four tuning knobs each default if omitted.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PersonasSection {
    pub enabled: bool,
    pub debounce_ms: i64,
    pub window_hours: u32,
    pub max_signals: i64,
    pub budget_cents_per_day: i64,
    #[serde(rename = "persona")]
    pub personas: Vec<PersonaEntryConfig>,
}

impl Default for PersonasSection {
    fn default() -> Self {
        Self {
            enabled: false,
            debounce_ms: 0,
            window_hours: 48,
            max_signals: 200,
            budget_cents_per_day: 500,
            personas: Vec::new(),
        }
    }
}

/// The `[discovery]` section (opt-in, default OFF). Its PRESENCE with
/// `enabled = true` wires the WORLD-FORWARD discovery step into `drive()`: on a
/// segment it reads the fresh signals, hands them to one `world_forward_discovery`
/// call (the §5.12 budget cap + the unscoreable rule live INSIDE it), persists each
/// candidate as a `watch:` event, and fans the SCOREABLE candidates out to beliefs.
/// Absent or `enabled = false` => the daemon is byte-identical to today (fail
/// closed). `#[serde(default)]` so omitting the WHOLE section deserializes to the
/// disabled default, and the tuning knobs each default if omitted.
///
/// COMMIT 1 added the world-forward fields; COMMIT 2 (market-back) extends this
/// section in place with the deterministic prefilter knobs (category allowlist,
/// volume floor, category-calibration floor; spec 5.12 "deterministic prefilter").
/// The prefilter knobs are INERT in prod until the live Kalshi catalog is wired
/// (T4.2) — `catalog` is empty, so the market-back step is a no-op even when the
/// section is enabled — but they are read into `PrefilterConfig` at boot regardless
/// (no dead config).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DiscoverySection {
    pub enabled: bool,
    pub budget_cents_per_day: i64,
    pub window_hours: u32,
    pub max_signals: i64,
    /// Signal kinds the world-forward loop reads each segment (empty => the step
    /// reads nothing and is a no-op, like an empty persona reads-set).
    pub signal_kinds: Vec<String>,
    /// MARKET-BACK prefilter (spec 5.12): only catalog listings whose category is
    /// in this allowlist survive the deterministic prefilter. Empty => no category
    /// passes (the market-back step is inert even with a non-empty catalog).
    pub category_allowlist: Vec<String>,
    /// MARKET-BACK prefilter (spec 5.12): the venue volume floor; a listing with
    /// fewer contracts is excluded (illiquid markets are not worth normalizing).
    pub min_volume_contracts: i64,
    /// MARKET-BACK prefilter (spec 5.12): the category-calibration floor; a listing
    /// whose category sits below this quality is excluded (the record says we cannot
    /// price it yet). The per-category quality map itself is queried at the
    /// composition (T2.8 calibration record) — empty in prod until that is wired.
    pub min_category_quality: f64,
}

impl Default for DiscoverySection {
    fn default() -> Self {
        Self {
            enabled: false,
            budget_cents_per_day: 500,
            window_hours: 48,
            max_signals: 200,
            signal_kinds: Vec::new(),
            category_allowlist: Vec::new(),
            min_volume_contracts: 0,
            min_category_quality: 0.0,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawToml {
    runtime: Option<RuntimeSection>,
    daemon: Option<DaemonSection>,
    cognition: Option<CognitionSection>,
    sim: Option<SimSection>,
    killswitch: Option<KillswitchSection>,
    kalshi: Option<KalshiSection>,
    synthesis: Option<crate::compose::SynthesisSection>,
    mech_extremes: Option<crate::compose::MechExtremesSection>,
    funding_forecast: Option<crate::compose::FundingForecastSection>,
    perp_event_basis: Option<crate::compose::PerpEventBasisSection>,
    perp_event_basis_v2: Option<crate::compose::PerpEventBasisV2Section>,
    review: Option<crate::compose::ReviewSection>,
    ingestion: Option<IngestionSection>,
    personas: Option<PersonasSection>,
    discovery: Option<DiscoverySection>,
}

/// The parsed daemon-relevant config.
#[derive(Debug, Clone)]
pub struct DaemonToml {
    /// Explicit runtime mode. Required when `venue = "kalshi"`; optional for
    /// legacy sim configs so old DST/test fixtures keep parsing.
    pub runtime: Option<RuntimeSection>,
    pub daemon: DaemonSection,
    pub cognition: CognitionSection,
    pub sim: Option<SimSection>,
    /// Optional `[killswitch]` (I4 revocation, open audit C2; default-OFF).
    /// `revocation_file` set => the daemon wraps its halt poller in a
    /// `RevocationHaltPoller` over that sentinel path (a kill REVOKES future
    /// order placement). Absent / unset => no wrap (byte-identical to today).
    pub killswitch: Option<KillswitchSection>,
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
    /// Optional `[perp_event_basis_v2]` opt-in (TRACK C slice-3b-v2). Its PRESENCE
    /// composes the propose-only perp/bracket BASIS-V2 strategy over the
    /// config-supplied ladder (the DC σ/horizon/EV knobs are optional, defaulted),
    /// ALONGSIDE rung-0. Absent => not composed (fail closed). It additionally
    /// gates the funding-rates POLLER spawn in main (the v2 arm scores funding
    /// beliefs, so its presence opts the daemon into filling the funding store).
    pub perp_event_basis_v2: Option<crate::compose::PerpEventBasisV2Section>,
    /// Optional `[review]` opt-in (T4.1/M2). Its PRESENCE composes the weekly/
    /// monthly review cadence (GO/NO-GO thresholds; advisory only, I7). Absent
    /// => no review fires (fail closed).
    pub review: Option<crate::compose::ReviewSection>,
    /// Optional `[ingestion]` opt-in (D10, default OFF). `enabled = true`
    /// spawns the news-aggregation ingestion loop (validator live on the
    /// ingest path). Absent / `enabled = false` => no ingestion.
    pub ingestion: Option<IngestionSection>,
    /// Optional `[personas]` opt-in (default OFF). Its PRESENCE with
    /// `enabled = true` wires the persona-analysis step into `drive()` (the
    /// loader is fail-closed: a file whose hash != the active registry row is
    /// refused). Absent / `enabled = false` => no persona step (the daemon is
    /// byte-identical to today).
    pub personas: Option<PersonasSection>,
    /// Optional `[discovery]` opt-in (spec 5.12, default OFF). Its PRESENCE with
    /// `enabled = true` wires the WORLD-FORWARD discovery step into `drive()`
    /// (read fresh signals, run `world_forward_discovery`, persist `watch:` events,
    /// and fan scoreable candidates to beliefs; the daily cost cap throttles first
    /// under pressure). Absent / `enabled = false` => no discovery step (the
    /// daemon is byte-identical to today).
    pub discovery: Option<DiscoverySection>,
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
            runtime: raw.runtime,
            daemon,
            cognition,
            sim: raw.sim,
            killswitch: raw.killswitch,
            kalshi: raw.kalshi,
            synthesis: raw.synthesis,
            mech_extremes: raw.mech_extremes,
            funding_forecast: raw.funding_forecast,
            perp_event_basis: raw.perp_event_basis,
            perp_event_basis_v2: raw.perp_event_basis_v2,
            review: raw.review,
            ingestion: raw.ingestion,
            personas: raw.personas,
            discovery: raw.discovery,
        })
    }

    /// Boot checks beyond parsing: venue gating and operational pins.
    pub fn validate_bootable(&self) -> Result<(), BootError> {
        if let Some(runtime) = self.runtime.as_ref() {
            if runtime.stage != self.daemon.stage {
                return Err(BootError::BadConfig {
                    reason: format!(
                        "[runtime].stage {:?} must match [daemon].stage {:?}",
                        runtime.stage, self.daemon.stage
                    ),
                });
            }
            if runtime.execution_mode == ExecutionMode::LiveDataOnly && runtime.orders_enabled {
                return Err(BootError::BadConfig {
                    reason: "orders_enabled = true is invalid when execution_mode = \
                             \"live_data_only\""
                        .to_string(),
                });
            }
            if runtime.orders_enabled && !runtime.execution_mode.allows_order_mutation() {
                return Err(BootError::BadConfig {
                    reason: format!(
                        "orders_enabled = true is invalid when execution_mode = {:?}",
                        runtime.execution_mode.as_str()
                    ),
                });
            }
            if runtime.execution_mode != ExecutionMode::ProductionOrders
                && runtime.production_unlock
            {
                return Err(BootError::BadConfig {
                    reason: "production_unlock may only be set with execution_mode = \
                             \"production_orders\""
                        .to_string(),
                });
            }
        }

        if let Some(data_source) = self.daemon.data_source.as_deref() {
            match data_source {
                "kalshi_prod" => {
                    if self.daemon.venue != "kalshi" || self.daemon.stage != "paper" {
                        return Err(BootError::BadConfig {
                            reason: format!(
                                "data_source = \"kalshi_prod\" requires venue = \"kalshi\" and \
                                 stage = \"paper\" (got venue = {:?}, stage = {:?})",
                                self.daemon.venue, self.daemon.stage
                            ),
                        });
                    }
                    match self.daemon.execution.as_deref() {
                        Some("paper") => {}
                        Some(other) => {
                            return Err(BootError::VenueNotBootable {
                                venue: "kalshi".to_string(),
                                reason: format!(
                                    "data_source=kalshi_prod with execution={other:?} is refused: \
                                     production Kalshi data is bootable only with local paper \
                                     execution until an operator promotion record exists (I7)"
                                ),
                            });
                        }
                        None => {
                            return Err(BootError::BadConfig {
                                reason: "data_source = \"kalshi_prod\" requires explicit \
                                         execution = \"paper\""
                                    .to_string(),
                            });
                        }
                    }
                }
                other => {
                    return Err(BootError::BadConfig {
                        reason: format!(
                            "unknown daemon data_source {other:?} (expected kalshi_prod)"
                        ),
                    });
                }
            }
        } else if let Some(execution) = self.daemon.execution.as_deref() {
            return Err(BootError::BadConfig {
                reason: format!(
                    "execution = {execution:?} requires an explicit data_source; omit both for \
                     legacy venue execution"
                ),
            });
        }

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
                // env-only, gated later in resolve_kalshi_demo_creds (not here — the
                // boot gate is pure over config, never reads the environment).
                "paper" => {
                    let runtime = self.runtime.as_ref().ok_or_else(|| BootError::BadConfig {
                        reason: "venue = \"kalshi\" requires explicit [runtime] with \
                                 execution_mode and orders_enabled"
                            .to_string(),
                    })?;
                    match runtime.execution_mode {
                        ExecutionMode::LiveDataOnly | ExecutionMode::PaperLedger => {
                            if self.daemon.data_source.as_deref() != Some("kalshi_prod")
                                || self.daemon.execution.as_deref() != Some("paper")
                            {
                                return Err(BootError::BadConfig {
                                    reason: format!(
                                        "execution_mode = {:?} requires data_source = \
                                         \"kalshi_prod\" and execution = \"paper\"",
                                        runtime.execution_mode.as_str()
                                    ),
                                });
                            }
                        }
                        ExecutionMode::DryRun => {
                            if runtime.orders_enabled {
                                return Err(BootError::BadConfig {
                                    reason: "execution_mode = \"dry_run\" must keep \
                                             orders_enabled = false"
                                        .to_string(),
                                });
                            }
                        }
                        ExecutionMode::DemoOrders => {
                            if self.daemon.data_source.is_some() || self.daemon.execution.is_some()
                            {
                                return Err(BootError::BadConfig {
                                    reason: "execution_mode = \"demo_orders\" must use the \
                                             Kalshi demo endpoint path, not composite \
                                             data_source/execution fields"
                                        .to_string(),
                                });
                            }
                            if !runtime.orders_enabled {
                                return Err(BootError::BadConfig {
                                    reason: "execution_mode = \"demo_orders\" requires \
                                             orders_enabled = true"
                                        .to_string(),
                                });
                            }
                        }
                        ExecutionMode::ProductionOrders => {
                            if !runtime.production_unlock {
                                return Err(BootError::VenueNotBootable {
                                    venue: "kalshi".to_string(),
                                    reason: "execution_mode = \"production_orders\" requires \
                                             [runtime].production_unlock = true and still needs \
                                             the I7 promotion gate"
                                        .to_string(),
                                });
                            }
                            return Err(BootError::VenueNotBootable {
                                venue: "kalshi".to_string(),
                                reason: "production_orders remains refused by the daemon: live \
                                         capital promotion is an operator action through I7"
                                    .to_string(),
                            });
                        }
                    }
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
        // Opt-in [personas]: when enabled, every configured cadence must be a
        // shape that can fire (Cadence::validate rejects e.g. daily_at_hour_utc
        // >= 24, which would silently never fire). A bad cadence is a STARTUP
        // rejection, not a dead trigger — mapped onto the crate's boot error
        // exactly as the other sections refuse. Disabled / absent => no check
        // (the step never runs, so a stale cadence cannot mislead).
        if let Some(personas) = self.personas.as_ref() {
            if personas.enabled {
                for entry in &personas.personas {
                    for cadence in &entry.cadences {
                        cadence.validate().map_err(|e| BootError::BadConfig {
                            reason: format!(
                                "[personas] persona {:?} has an invalid cadence: {e}",
                                entry.id
                            ),
                        })?;
                    }
                }
            }
        }
        // Opt-in [discovery]: when enabled, the world-forward loop spends against a
        // HARD daily cost cap (spec 5.12). A non-positive cap means the loop could
        // never run (or, mishandled, would mean "no limit") — refuse it at boot, a
        // startup rejection, not a silently-throttled-forever step. Disabled /
        // absent => no check (the step never runs).
        if let Some(discovery) = self.discovery.as_ref() {
            if discovery.enabled && discovery.budget_cents_per_day <= 0 {
                return Err(BootError::BadConfig {
                    reason: format!(
                        "[discovery] budget_cents_per_day must be positive when enabled (got {})",
                        discovery.budget_cents_per_day
                    ),
                });
            }
        }
        Ok(())
    }
}

/// Write the live `DATABASE_URL` to `{runtime_dir}/current-demo-db-url`.
///
/// This is the **F11 pointer-write**: the demo boot path calls this once the
/// daemon pool is connected, so the chain-view UI (and operator tooling) can
/// discover which DB the running paper-demo is connected to without reading
/// environment variables.
///
/// The write is **atomic**: the URL is written to a `.tmp` sibling file then
/// renamed into place, so readers never see a partial write.
///
/// # Errors
/// Returns `io::Error` for any filesystem failure; callers propagate via `?`.
pub fn write_demo_db_pointer(runtime_dir: &Path, db_url: &str) -> Result<(), io::Error> {
    let dest = runtime_dir.join("current-demo-db-url");
    let tmp = runtime_dir.join("current-demo-db-url.tmp");
    std::fs::write(&tmp, db_url)?;
    std::fs::rename(&tmp, &dest)?;
    Ok(())
}

/// F11 daemon-boot pointer-write: call `write_demo_db_pointer` **only** when
/// `mode == ExecutionMode::PaperLedger` (the paper-demo/paper-on-live context).
///
/// This is the entry point the daemon binary calls at boot (after the Postgres
/// pool is connected) — NOT the CLI `start` command. Moving the call here
/// ensures the pointer reflects the URL the daemon ACTUALLY connected with,
/// eliminates the stale-pointer risk from a CLI-side pre-spawn write, and
/// removes the need for `fortuna-cli` to depend on `fortuna-live` at runtime.
///
/// For all other modes the call is a no-op (returns `Ok(())`). Fail-closed:
/// a new `ExecutionMode` variant defaults to doing nothing.
///
/// # Errors
/// Propagates `io::Error` from `write_demo_db_pointer` for callers to handle
/// via `?`.
pub fn maybe_write_demo_db_pointer(
    runtime_dir: &Path,
    db_url: &str,
    mode: ExecutionMode,
) -> Result<(), io::Error> {
    if mode == ExecutionMode::PaperLedger {
        write_demo_db_pointer(runtime_dir, db_url)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal config that BOOTS (venue=sim with a non-empty [sim] world,
    /// positive budgets, a valid halt poll + metrics bind). The persona tests
    /// append a `[personas]` block to this and re-check validate_bootable.
    const BOOTABLE_BASE: &str = r#"
[daemon]
venue = "sim"
tick_interval_ms = 1000
halt_poll_ms = 500
metrics_bind = "127.0.0.1:9187"

[cognition]
daily_budget_cents = 1500
per_cycle_budget_cents = 50

[sim]
bracket_sets = [["SIM-BKT-LO", "SIM-BKT-MID", "SIM-BKT-HI"]]
"#;

    const KALSHI_PAPER_LIVE_BASE: &str = r#"
[runtime]
stage = "paper"
execution_mode = "live_data_only"
orders_enabled = false

[daemon]
venue = "kalshi"
stage = "paper"
data_source = "kalshi_prod"
execution = "paper"
tick_interval_ms = 1000
halt_poll_ms = 500
metrics_bind = "127.0.0.1:9187"

[cognition]
daily_budget_cents = 1500
per_cycle_budget_cents = 50

[kalshi]
series = ["KXBTC"]
bracket_sets = [["KXBTC-LO", "KXBTC-HI"]]
"#;

    #[test]
    fn config_without_killswitch_section_parses_to_none_and_boots() {
        // Omitting [killswitch] entirely => killswitch == None (the daemon is
        // byte-identical to today; the revocation poller is not wired).
        let dcfg = DaemonToml::parse(BOOTABLE_BASE).expect("base config parses");
        assert!(
            dcfg.killswitch.is_none(),
            "no [killswitch] section => killswitch is None (default-off)"
        );
        dcfg.validate_bootable()
            .expect("a config with no [killswitch] boots");
    }

    #[test]
    fn kalshi_requires_explicit_runtime_section() {
        let text = KALSHI_PAPER_LIVE_BASE
            .replace(
                "[runtime]\nstage = \"paper\"\nexecution_mode = \"live_data_only\"\norders_enabled = false\n\n",
                "",
            );
        let dcfg = DaemonToml::parse(&text).expect("shape parses");
        let err = dcfg
            .validate_bootable()
            .expect_err("kalshi configs must name an explicit runtime mode");
        match err {
            BootError::BadConfig { reason } => assert!(reason.contains("[runtime]")),
            other => panic!("expected BadConfig naming [runtime], got {other:?}"),
        }
    }

    #[test]
    fn live_data_only_accepts_paper_live_with_orders_disabled() {
        let dcfg = DaemonToml::parse(KALSHI_PAPER_LIVE_BASE).expect("parses");
        dcfg.validate_bootable()
            .expect("live_data_only + orders_enabled=false is the safe demo mode");
        let runtime = dcfg.runtime.expect("runtime parsed");
        assert_eq!(runtime.execution_mode, ExecutionMode::LiveDataOnly);
        assert!(!runtime.orders_enabled);
    }

    #[test]
    fn live_data_only_refuses_orders_enabled() {
        let text = KALSHI_PAPER_LIVE_BASE.replace(
            "execution_mode = \"live_data_only\"\norders_enabled = false",
            "execution_mode = \"live_data_only\"\norders_enabled = true",
        );
        let dcfg = DaemonToml::parse(&text).expect("parses");
        let err = dcfg
            .validate_bootable()
            .expect_err("live_data_only must not permit order mutation");
        match err {
            BootError::BadConfig { reason } => assert!(reason.contains("orders_enabled")),
            other => panic!("expected BadConfig for orders_enabled, got {other:?}"),
        }
    }

    #[test]
    fn demo_orders_refuses_production_data_composite_path() {
        let text = KALSHI_PAPER_LIVE_BASE.replace(
            "execution_mode = \"live_data_only\"\norders_enabled = false",
            "execution_mode = \"demo_orders\"\norders_enabled = true",
        );
        let dcfg = DaemonToml::parse(&text).expect("parses");
        let err = dcfg
            .validate_bootable()
            .expect_err("demo_orders must not use kalshi_prod/paper composite fields");
        match err {
            BootError::BadConfig { reason } => assert!(reason.contains("demo endpoint")),
            other => panic!("expected BadConfig for demo endpoint mismatch, got {other:?}"),
        }
    }

    #[test]
    fn production_orders_still_refuses_even_with_unlock() {
        let text = KALSHI_PAPER_LIVE_BASE.replace(
            "execution_mode = \"live_data_only\"\norders_enabled = false",
            "execution_mode = \"production_orders\"\norders_enabled = true\nproduction_unlock = true",
        );
        let dcfg = DaemonToml::parse(&text).expect("parses");
        let err = dcfg
            .validate_bootable()
            .expect_err("production orders remain I7/operator gated");
        match err {
            BootError::VenueNotBootable { reason, .. } => {
                assert!(reason.contains("production_orders"))
            }
            other => panic!("expected VenueNotBootable for production orders, got {other:?}"),
        }
    }

    #[test]
    fn killswitch_revocation_file_parses() {
        // A [killswitch] with revocation_file set => the daemon wraps its halt
        // poller. The value round-trips so a misspelled key is caught here, not
        // silently dropped (deny_unknown_fields on the section).
        let text = format!(
            "{BOOTABLE_BASE}\n\
             [killswitch]\n\
             revocation_file = \"/var/run/fortuna/KILLSWITCH_REVOKED\"\n"
        );
        let dcfg = DaemonToml::parse(&text).expect("parses");
        assert_eq!(
            dcfg.killswitch
                .as_ref()
                .and_then(|k| k.revocation_file.as_deref()),
            Some("/var/run/fortuna/KILLSWITCH_REVOKED"),
            "revocation_file round-trips for the main wiring to read"
        );
        dcfg.validate_bootable()
            .expect("a config with [killswitch].revocation_file boots");
    }

    #[test]
    fn config_without_personas_section_parses_to_none_and_boots() {
        // (a) Omitting [personas] entirely => personas == None (the daemon is
        // byte-identical to today; the persona step never wires). validate_
        // bootable accepts it (the persona check is skipped when absent).
        let dcfg = DaemonToml::parse(BOOTABLE_BASE).expect("base config parses");
        assert!(
            dcfg.personas.is_none(),
            "no [personas] section => personas is None (default-off)"
        );
        dcfg.validate_bootable()
            .expect("a config with no [personas] boots");
    }

    #[test]
    fn personas_enabled_with_a_bad_cadence_is_rejected_by_validate() {
        // (b) An ENABLED [personas] with a never-firing cadence (daily 99:00 UTC,
        // which Cadence::validate rejects) is refused at boot — a startup
        // rejection, not a silently-dead trigger. The cadence shape PARSES (it is
        // a valid Cadence::DailyAtHourUtc struct variant, just an out-of-range
        // hour); the REFUSAL comes from validate_bootable calling
        // Cadence::validate. NB Cadence is a struct-variant enum, so the TOML is
        // `daily_at_hour_utc = { hour = 99 }`, NOT `daily_at_hour_utc = 99`.
        let text = format!(
            "{BOOTABLE_BASE}\n\
             [personas]\n\
             enabled = true\n\
             [[personas.persona]]\n\
             id = \"meteorologist\"\n\
             dir = \"config/personas/meteorologist\"\n\
             cadences = [{{ daily_at_hour_utc = {{ hour = 99 }} }}]\n"
        );
        let dcfg =
            DaemonToml::parse(&text).expect("a bad-hour cadence still PARSES (valid variant)");
        let err = dcfg
            .validate_bootable()
            .expect_err("an enabled persona with a never-firing cadence must be REJECTED");
        match err {
            BootError::BadConfig { reason } => {
                assert!(
                    reason.contains("cadence") && reason.contains("meteorologist"),
                    "the refusal names the offending persona + cadence, got: {reason}"
                );
            }
            other => panic!("expected BadConfig for the bad cadence, got {other:?}"),
        }
    }

    #[test]
    fn personas_disabled_with_a_bad_cadence_is_not_validated() {
        // A DISABLED [personas] never wires the step, so a stale/never-firing
        // cadence is NOT a boot refusal (it cannot mislead a step that does not
        // run). This proves the cadence check is gated on `enabled`, matching how
        // [ingestion] gates its loop. NON-VACUOUS vs the enabled test above.
        let text = format!(
            "{BOOTABLE_BASE}\n\
             [personas]\n\
             enabled = false\n\
             [[personas.persona]]\n\
             id = \"meteorologist\"\n\
             dir = \"config/personas/meteorologist\"\n\
             cadences = [{{ daily_at_hour_utc = {{ hour = 99 }} }}]\n"
        );
        let dcfg = DaemonToml::parse(&text).expect("parses");
        assert!(dcfg.personas.is_some(), "the section is present (disabled)");
        dcfg.validate_bootable()
            .expect("a DISABLED persona section is not cadence-checked");
    }

    #[test]
    fn config_without_discovery_section_parses_to_none_and_boots() {
        // Omitting [discovery] entirely => discovery == None (the daemon is
        // byte-identical to today; the world-forward step never wires).
        // validate_bootable accepts it (the budget check is skipped when absent).
        let dcfg = DaemonToml::parse(BOOTABLE_BASE).expect("base config parses");
        assert!(
            dcfg.discovery.is_none(),
            "no [discovery] section => discovery is None (default-off)"
        );
        dcfg.validate_bootable()
            .expect("a config with no [discovery] boots");
    }

    #[test]
    fn discovery_enabled_with_zero_budget_is_rejected_by_validate() {
        // An ENABLED [discovery] with a non-positive daily cost cap is refused at
        // boot — a startup rejection, not a silently-throttled-forever loop. The
        // shape PARSES (a valid i64); the REFUSAL comes from validate_bootable.
        let text = format!(
            "{BOOTABLE_BASE}\n\
             [discovery]\n\
             enabled = true\n\
             budget_cents_per_day = 0\n"
        );
        let dcfg = DaemonToml::parse(&text).expect("a zero-budget config still PARSES");
        let err = dcfg
            .validate_bootable()
            .expect_err("an enabled discovery with a non-positive budget must be REJECTED");
        match err {
            BootError::BadConfig { reason } => {
                assert!(
                    reason.contains("discovery") && reason.contains("budget_cents_per_day"),
                    "the refusal names the offending [discovery] knob, got: {reason}"
                );
            }
            other => panic!("expected BadConfig for the zero budget, got {other:?}"),
        }
    }

    #[test]
    fn discovery_disabled_with_zero_budget_is_not_validated() {
        // A DISABLED [discovery] never wires the step, so a non-positive budget is
        // NOT a boot refusal (it cannot mislead a step that does not run). Proves
        // the budget check is gated on `enabled`. NON-VACUOUS vs the enabled test.
        let text = format!(
            "{BOOTABLE_BASE}\n\
             [discovery]\n\
             enabled = false\n\
             budget_cents_per_day = 0\n"
        );
        let dcfg = DaemonToml::parse(&text).expect("parses");
        assert!(
            dcfg.discovery.is_some(),
            "the section is present (disabled)"
        );
        dcfg.validate_bootable()
            .expect("a DISABLED discovery section is not budget-checked");
    }
}
