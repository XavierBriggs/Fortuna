//! F6: the STRICT `aeolus.forecast/v2` envelope parser + the μ/σ→bracket
//! probability backbone — the deterministic foundation of the Aeolus
//! weather→belief pipeline (source contract
//! `docs/design/aeolus-fortuna-source-contract.md` §2, §7).
//!
//! Two pieces, both pure and replay-deterministic:
//!
//! 1. **The μ/σ→p helpers** (`bracket_prob_ge` / `bracket_prob_lt` /
//!    `bracket_range_prob`). They REUSE the pinned deterministic normal CDF
//!    (`persona_beliefs::{normal_cdf, prob_at_least}`, A&S 7.1.26 erf — NOT the
//!    platform `libm`, so byte-identical replay holds across toolchains, §7/I5).
//!    Kalshi temperature brackets are INTEGER degrees: a `ge t` bracket means the
//!    integer daily high ≥ t, i.e. the continuous temperature `T ≥ t − 0.5`
//!    (a half-degree continuity correction). The recorded fixture's `p`'s were
//!    computed WITH this correction (verified: bracket ge87 has p≈0.6719, which
//!    is `1 − Φ((87 − 0.5 − μ)/σ)`; WITHOUT the −0.5 it is ≈0.572). So:
//!      - `bracket_prob_ge(t, μ, σ) = prob_at_least(t − 0.5, μ, σ)` = P(high ≥ t).
//!      - `bracket_prob_lt(t, μ, σ) = 1 − bracket_prob_ge(t, μ, σ)` = P(high < t).
//!      - `bracket_range_prob(floor, cap, μ, σ) = ge(floor) − ge(cap)` =
//!        P(floor ≤ high < cap) — the `in_bracket` range; the CALLER pairs the
//!        two thresholds, a single envelope bracket carries only one.
//!
//!    Every returned probability is clamped into `(f64::EPSILON, 1 − f64::EPSILON)`
//!    so it is a valid belief probability; `None` when σ≤0 or μ/σ non-finite.
//!
//! 2. **The strict envelope parser** (`parse_envelope` / `parse_response`).
//!    `deny_unknown_fields` on EVERY struct + renamed enums make any contract
//!    drift a hard parse error on purpose (§8). On top of serde's structural
//!    rejection it VALIDATES: `schema == "aeolus.forecast/v2"`, `sigma > 0`, ≥1
//!    bracket, every `event_hint` non-empty; and CLAMPS each `brackets[].p` into
//!    `[1e-6, 1−1e-6]` (clamp-not-reject per §2/rev-3 — a stray 0/1 is clamped,
//!    not a parse failure). `family == normal` and `units == degF` are enforced
//!    by the enums (the only accepted rename values).
//!
//! f64 here is forecast-domain (probabilities) only — never money (§7). No
//! `SystemTime`: timestamps go through `UtcTimestamp::parse_iso8601` (which
//! accepts both the `Z` and `+00:00` offset forms — the fixture uses `+00:00`).

use crate::persona_beliefs::prob_at_least;
use fortuna_core::clock::UtcTimestamp;
use serde::Deserialize;
use thiserror::Error;

/// The pinned `brackets[].p` clamp window (contract §2): Aeolus pre-clamps to
/// `[1e-6, 1−1e-6]`; FORTUNA re-clamps as defense-in-depth.
const P_FLOOR: f64 = 1e-6;
const P_CEIL: f64 = 1.0 - 1e-6;

/// The pinned wire schema string. Any other value is rejected (forces lockstep
/// upgrades, §8).
const SCHEMA_V2: &str = "aeolus.forecast/v2";

// ---------------------------------------------------------------------------
// μ/σ → bracket-probability backbone (the deterministic part of F6).
// ---------------------------------------------------------------------------

/// Clamp a probability into `(f64::EPSILON, 1 − f64::EPSILON)` so it is a valid
/// belief probability (mirrors `persona_beliefs::normal_cdf`'s clamp).
fn clamp_prob(p: f64) -> f64 {
    p.clamp(f64::EPSILON, 1.0 - f64::EPSILON)
}

/// P(integer daily high ≥ `t_f`) for a Normal(μ, σ), with the half-degree
/// continuity correction (`T ≥ t − 0.5`). `None` when σ≤0 or μ/σ non-finite.
/// Clamped into `(ε, 1−ε)`.
pub fn bracket_prob_ge(t_f: i64, mu: f64, sigma: f64) -> Option<f64> {
    prob_at_least((t_f as f64) - 0.5, mu, sigma).map(clamp_prob)
}

/// P(integer daily high < `t_f`) = `1 − bracket_prob_ge(t_f, …)`. `None` when
/// σ≤0 or μ/σ non-finite. Clamped into `(ε, 1−ε)`.
pub fn bracket_prob_lt(t_f: i64, mu: f64, sigma: f64) -> Option<f64> {
    bracket_prob_ge(t_f, mu, sigma).map(|ge| clamp_prob(1.0 - ge))
}

/// P(`floor_f` ≤ integer daily high < `cap_f`) = `ge(floor) − ge(cap)` for a
/// Normal(μ, σ), with the half-degree correction on both edges. The CALLER
/// supplies the floor/cap pair for an `in_bracket` comparison (a single envelope
/// bracket carries only one threshold). `None` when σ≤0 or μ/σ non-finite; an
/// inverted range (`floor ≥ cap`) collapses to the lower clamp rather than going
/// negative. Clamped into `(ε, 1−ε)`.
pub fn bracket_range_prob(floor_f: i64, cap_f: i64, mu: f64, sigma: f64) -> Option<f64> {
    // Use the UNCLAMPED ge values for the difference so the subtraction is exact
    // in the body of the distribution; clamp only the final result.
    let ge_floor = prob_at_least((floor_f as f64) - 0.5, mu, sigma)?;
    let ge_cap = prob_at_least((cap_f as f64) - 0.5, mu, sigma)?;
    Some(clamp_prob(ge_floor - ge_cap))
}

// ---------------------------------------------------------------------------
// The strict wire types (contract §2). deny_unknown_fields on EVERY struct.
// ---------------------------------------------------------------------------

/// The forecast variable. v2 emits `tmax`/`tmin` ONLY (contract §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Variable {
    #[serde(rename = "tmax")]
    Tmax,
    #[serde(rename = "tmin")]
    Tmin,
}

/// Forecast units. v2 guards against silent °C drift — `degF` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Units {
    #[serde(rename = "degF")]
    DegF,
}

/// Predictive family. v2 supports `normal` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Family {
    #[serde(rename = "normal")]
    Normal,
}

/// How a bracket threshold reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Comparison {
    #[serde(rename = "ge")]
    Ge,
    #[serde(rename = "lt")]
    Lt,
    #[serde(rename = "in_bracket")]
    InBracket,
}

/// How the event settles (contract §2; requires a registered grader, §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Authority {
    #[serde(rename = "nws_observed_high")]
    NwsObservedHigh,
    #[serde(rename = "nws_observed_low")]
    NwsObservedLow,
}

/// The predictive distribution (μ/σ — the load-bearing payload).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Distribution {
    pub family: Family,
    pub mu: f64,
    pub sigma: f64,
    pub model_version: String,
}

/// Self-reported skill telemetry. ALL of `crps`/`crpss_vs_raw`/`n_scored` are
/// nullable — `crpss_vs_raw` ships `null` until the Aeolus scorer lands (§2/rev-3).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    pub crps: Option<f64>,
    pub crpss_vs_raw: Option<f64>,
    pub n_scored: Option<i64>,
    pub window_days: i64,
    pub as_of: UtcTimestamp,
}

/// How the belief is graded at settlement.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub authority: Authority,
    pub nws_station_id: String,
    pub settles_after: UtcTimestamp,
    pub note: String,
}

/// One convenience bracket (Aeolus's own probability + the threshold FORTUNA
/// maps to a market). `p` is CLAMPED into `[1e-6, 1−1e-6]` during validation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bracket {
    pub event_hint: String,
    pub threshold_f: i64,
    pub comparison: Comparison,
    pub p: f64,
}

/// The raw, structurally-parsed v2 envelope (serde shape only). Semantic
/// validation (schema/σ/hints/clamp) runs in `parse_envelope`, which returns the
/// validated `AeolusForecast` wrapper. Kept private-ish: callers consume
/// `AeolusForecast`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnvelope {
    schema: String,
    station: String,
    nws_station_id: String,
    variable: Variable,
    units: Units,
    target_date: String,
    run_at: UtcTimestamp,
    next_run_at: UtcTimestamp,
    valid_until: UtcTimestamp,
    distribution: Distribution,
    skill: Skill,
    resolution: Resolution,
    brackets: Vec<Bracket>,
}

/// The transport wrapper: `{ "forecasts": [ <envelope>, ... ] }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    forecasts: Vec<RawEnvelope>,
}

/// A parsed AND validated v2 forecast envelope. The fields are private; access
/// is via the accessors so the validated invariants (schema pinned, σ>0,
/// non-empty brackets/hints, p clamped) cannot be bypassed after construction.
#[derive(Debug, Clone, PartialEq)]
pub struct AeolusForecast {
    inner: RawEnvelope,
}

impl AeolusForecast {
    pub fn schema(&self) -> &str {
        &self.inner.schema
    }
    pub fn station(&self) -> &str {
        &self.inner.station
    }
    pub fn nws_station_id(&self) -> &str {
        &self.inner.nws_station_id
    }
    pub fn variable(&self) -> Variable {
        self.inner.variable
    }
    pub fn units(&self) -> Units {
        self.inner.units
    }
    pub fn target_date(&self) -> &str {
        &self.inner.target_date
    }
    pub fn run_at(&self) -> UtcTimestamp {
        self.inner.run_at
    }
    pub fn next_run_at(&self) -> UtcTimestamp {
        self.inner.next_run_at
    }
    pub fn valid_until(&self) -> UtcTimestamp {
        self.inner.valid_until
    }
    pub fn distribution(&self) -> &Distribution {
        &self.inner.distribution
    }
    pub fn mu(&self) -> f64 {
        self.inner.distribution.mu
    }
    pub fn sigma(&self) -> f64 {
        self.inner.distribution.sigma
    }
    pub fn skill(&self) -> &Skill {
        &self.inner.skill
    }
    pub fn resolution(&self) -> &Resolution {
        &self.inner.resolution
    }
    pub fn brackets(&self) -> &[Bracket] {
        &self.inner.brackets
    }

    /// The forecast-identity tuple `(station, variable, target_date, run_at)` —
    /// load-bearing for the later dedup slice (contract §2 identity tuple).
    pub fn identity(&self) -> (String, Variable, String, UtcTimestamp) {
        (
            self.inner.station.clone(),
            self.inner.variable,
            self.inner.target_date.clone(),
            self.inner.run_at,
        )
    }
}

/// Strict-parse + validation errors. Structural drift surfaces as `Json`;
/// semantic violations get their own typed variant.
#[derive(Debug, Error)]
pub enum AeolusError {
    #[error("unexpected schema {found:?} (expected {expected:?})")]
    UnknownSchema { expected: String, found: String },
    #[error("non-positive or non-finite sigma: {sigma}")]
    NonPositiveSigma { sigma: f64 },
    #[error("envelope carries no brackets (a broken export, not a no-op)")]
    EmptyBrackets,
    #[error("bracket with empty event_hint")]
    EmptyEventHint,
    #[error("malformed aeolus envelope json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Validate a structurally-parsed envelope into a typed `AeolusForecast`:
/// pin the schema, reject σ≤0/non-finite, require ≥1 bracket and non-empty
/// hints, and CLAMP each `brackets[].p` into `[1e-6, 1−1e-6]`. `family`/`units`
/// are already enforced by the enums; this asserts the remaining semantics.
fn validate(mut raw: RawEnvelope) -> Result<AeolusForecast, AeolusError> {
    if raw.schema != SCHEMA_V2 {
        return Err(AeolusError::UnknownSchema {
            expected: SCHEMA_V2.to_string(),
            found: raw.schema,
        });
    }
    let sigma = raw.distribution.sigma;
    // Accept only finite, strictly-positive sigma. `sigma > 0.0` is already
    // false for NaN; the explicit `is_finite` also rejects +∞.
    if !(sigma.is_finite() && sigma > 0.0) {
        return Err(AeolusError::NonPositiveSigma { sigma });
    }
    if raw.brackets.is_empty() {
        return Err(AeolusError::EmptyBrackets);
    }
    for bracket in &mut raw.brackets {
        if bracket.event_hint.trim().is_empty() {
            return Err(AeolusError::EmptyEventHint);
        }
        // clamp-not-reject (§2/rev-3): a stray 0/1 is clamped, never a failure.
        bracket.p = bracket.p.clamp(P_FLOOR, P_CEIL);
    }
    Ok(AeolusForecast { inner: raw })
}

/// Strict-parse + validate a single v2 envelope from a JSON string.
pub fn parse_envelope(body: &str) -> Result<AeolusForecast, AeolusError> {
    let raw: RawEnvelope = serde_json::from_str(body)?;
    validate(raw)
}

/// Strict-parse + validate the `{ "forecasts": [...] }` wrapper into a vector of
/// validated forecasts. A single malformed/invalid member surfaces its typed
/// error (the whole response is rejected — a broken run is not silently dropped).
pub fn parse_response(body: &str) -> Result<Vec<AeolusForecast>, AeolusError> {
    let response: Response = serde_json::from_str(body)?;
    response.forecasts.into_iter().map(validate).collect()
}
