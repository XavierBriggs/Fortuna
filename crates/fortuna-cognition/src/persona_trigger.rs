//! Track E E.3b: the persona trigger layer (design §7) — declarative &
//! schedulable, DECOUPLED from the persona. A persona does not know *why* it
//! ran; this layer decides *when* a `(persona, region_key)` run fires, and
//! funnels every source through one per-`(persona, region)` serialization +
//! debounce so duplicate/concurrent triggers coalesce into ONE in-flight run.
//!
//! Sources (design §7):
//! - **Signal-driven** — a signal of a kind the persona READS arrives
//!   ([`PersonaTriggerSpec::fires_on_signal`]; the kinds come from the persona
//!   definition, not a separate rule list — config, not per-domain code).
//! - **Scheduled / cadence** — a fire-once-per-period [`Cadence`] generalizing
//!   the daemon's `DailyScheduler` ("every 6h", "daily 05:00 UTC").
//! - **Manual / operator** — a direct request for `(persona, region)`.
//!
//! Serialization REUSES the existing [`crate::signals::TriggerEngine`] (keyed by
//! [`persona_region_key`]) — the same one-in-flight + post-completion debounce
//! the decision cycle uses; this layer never modifies it (extend, don't break).

use crate::persona::PersonaMeta;
use crate::signals::{TriggerDecision, TriggerEngine, TriggerEngineConfig};
use fortuna_core::clock::UtcTimestamp;
use serde::Deserialize;
use std::collections::BTreeMap;
use thiserror::Error;

/// A cadence config is invalid (caught at config-load, not silently dead).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CadenceError {
    #[error("DailyAtHourUtc hour must be 0..=23, got {0} (would silently never fire)")]
    HourOutOfRange(u32),
}

/// A fire-once-per-period schedule (design §7), generalizing `DailyScheduler`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cadence {
    /// Fire once per fixed N-hour window (e.g. `every_hours: 6`). N is clamped
    /// to >= 1.
    EveryHours { hours: u32 },
    /// Fire once per UTC day, on or after the given hour (e.g. daily 05:00 UTC =
    /// `daily_at_hour_utc: 5`). Not eligible before that hour on a given day.
    DailyAtHourUtc { hour: u32 },
}

impl Cadence {
    /// Reject a config that would silently never fire (e.g. `DailyAtHourUtc`
    /// hour >= 24). The composition calls this at config-load so a typo is a
    /// startup rejection, not a dead trigger. `EveryHours { hours: 0 }` is
    /// clamped to 1 at use, so it is always valid.
    pub fn validate(&self) -> Result<(), CadenceError> {
        match self {
            Cadence::EveryHours { .. } => Ok(()),
            Cadence::DailyAtHourUtc { hour } if *hour >= 24 => {
                Err(CadenceError::HourOutOfRange(*hour))
            }
            Cadence::DailyAtHourUtc { .. } => Ok(()),
        }
    }

    /// The monotone period key this `now` falls in. `None` = not yet eligible to
    /// fire in the current period (a daily cadence before its hour).
    fn period_key(&self, now: UtcTimestamp) -> Option<i64> {
        let secs = now.epoch_millis().div_euclid(1000);
        match self {
            Cadence::EveryHours { hours } => {
                let h = (*hours as i64).max(1);
                Some(secs.div_euclid(3600).div_euclid(h))
            }
            Cadence::DailyAtHourUtc { hour } => {
                let day = secs.div_euclid(86_400);
                let hour_of_day = secs.rem_euclid(86_400).div_euclid(3600);
                if hour_of_day >= *hour as i64 {
                    Some(day)
                } else {
                    None
                }
            }
        }
    }
}

/// Tracks the last-fired period per schedule key so a cadence fires at most once
/// per period (deterministic: all time is the caller's injected `Clock`).
///
/// SCOPE: fire-once-per-period holds within THIS scheduler's process lifetime —
/// it is in-process state (like the daemon's `DailyScheduler`), NOT persisted.
/// A daemon restart starts with an empty map, so a daily cadence may re-fire its
/// current period once after a restart (acceptable, and matching the daemon's
/// existing schedulers). Cross-restart durability is deferred (see GAPS).
#[derive(Debug, Default)]
pub struct CadenceScheduler {
    last_fired: BTreeMap<String, i64>,
}

impl CadenceScheduler {
    pub fn new() -> CadenceScheduler {
        CadenceScheduler {
            last_fired: BTreeMap::new(),
        }
    }

    /// Is `cadence` due for `key` at `now`? Due iff a period exists and differs
    /// from the last fired for this key; marks it fired (fire-once-per-period).
    /// Marks fired when the trigger FIRES, not when the run completes — a run that
    /// later throttles/skips/degrades still consumes the period (the trigger
    /// fired; the run's outcome is the runner's concern, design §7/§8).
    pub fn due(&mut self, key: &str, cadence: &Cadence, now: UtcTimestamp) -> bool {
        match cadence.period_key(now) {
            None => false,
            Some(period) => {
                if self.last_fired.get(key) == Some(&period) {
                    false
                } else {
                    self.last_fired.insert(key.to_string(), period);
                    true
                }
            }
        }
    }
}

/// The declarative trigger spec for one persona (design §7). Decoupled from the
/// persona's method; `reads_signal_kinds` comes from the persona definition,
/// `cadences` are operator trigger config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaTriggerSpec {
    pub persona_id: String,
    pub reads_signal_kinds: Vec<String>,
    pub cadences: Vec<Cadence>,
}

impl PersonaTriggerSpec {
    /// Build from a loaded persona's metadata + the operator's cadence config.
    pub fn from_meta(meta: &PersonaMeta, cadences: Vec<Cadence>) -> PersonaTriggerSpec {
        PersonaTriggerSpec {
            persona_id: meta.id.clone(),
            reads_signal_kinds: meta.reads_signal_kinds.clone(),
            cadences,
        }
    }

    /// Does an arriving signal of `kind` trigger this persona (it reads that
    /// kind)? Signals it does not read never trigger it (design §7).
    pub fn fires_on_signal(&self, kind: &str) -> bool {
        self.reads_signal_kinds.iter().any(|k| k == kind)
    }
}

/// The per-`(persona, region)` serialization key — the debounce/coalescing unit.
/// The components are joined by the ASCII Unit Separator (0x1F), which cannot
/// appear in a persona id or an expanded region key, so two distinct
/// `(persona, region)` pairs never collide on one gate key (e.g. `("a","b:c")`
/// and `("a:b","c")` map to different keys).
pub fn persona_region_key(persona_id: &str, region_key: &str) -> String {
    format!("{persona_id}\u{1f}{region_key}")
}

/// Serialization + debounce gate for persona runs, keyed by `(persona, region)`.
/// Reuses the existing decision-cycle [`TriggerEngine`] machinery (unmodified):
/// AT MOST ONE run in flight per `(persona, region)`; concurrent/duplicate
/// requests coalesce, and a completion opens a debounce window.
#[derive(Debug)]
pub struct PersonaTriggerGate {
    engine: TriggerEngine,
}

impl PersonaTriggerGate {
    /// `debounce_ms`: the post-completion coalescing window (a signal burst is
    /// one run, not five).
    pub fn new(debounce_ms: i64) -> PersonaTriggerGate {
        PersonaTriggerGate {
            engine: TriggerEngine::new(TriggerEngineConfig {
                debounce_ms,
                rules: Vec::new(), // serialization-only; persona rules live in the spec
            }),
        }
    }

    /// Request a run for `(persona, region)`: `Fire` if none in flight and
    /// outside debounce; otherwise coalesced (counted).
    pub fn request(
        &mut self,
        persona_id: &str,
        region_key: &str,
        now: UtcTimestamp,
    ) -> TriggerDecision {
        self.engine
            .request_cycle(&persona_region_key(persona_id, region_key), now)
    }

    /// Mark a run started (after a `Fire`).
    pub fn begin(&mut self, persona_id: &str, region_key: &str) {
        self.engine
            .begin_cycle(&persona_region_key(persona_id, region_key));
    }

    /// Mark a run complete; returns how many triggers coalesced while it ran
    /// (reported, never silent — the caller audits and decides on a follow-up).
    pub fn complete(&mut self, persona_id: &str, region_key: &str, now: UtcTimestamp) -> u64 {
        self.engine
            .complete_cycle(&persona_region_key(persona_id, region_key), now)
    }
}
