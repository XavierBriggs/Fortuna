//! Ingestion scheduler (design §4.2 "Ingestion scheduler"): the keystone that
//! drives the dumb adapters on a cadence, runs Layer-1 structural validation
//! on every fetched item (refuse-and-quarantine), tracks per-source health and
//! telemetry, and tags what may wake a decision cycle.
//!
//! THE HARD GATE (re-gate 2026-06-13): the `StructuralValidator` MUST run on
//! the live ingest path — a future-dated / republished / over-volume item is
//! DROPPED-and-recorded here, never passed downstream verbatim. That wiring is
//! `tick`'s centerpiece and is tested adversarially.
//!
//! Design split: the deterministic core is `tick(now) -> TickOutcome` plus
//! `next_wake()` / `rearm()`. It takes the injected `Clock`'s `now` as a
//! parameter and never sleeps — so it replays under SimClock. The actual async
//! run-loop (sleep-until-next-wake, stop signal) is the D10 `drive()` seam,
//! which consumes `TickOutcome` and routes accepted→normalizer,
//! dropped→metrics, alerts→Slack.

use std::time::Duration;

use chrono::Timelike;
use fortuna_cognition::signals::{RawSignal, Source};
use fortuna_core::clock::UtcTimestamp;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config::EventWindow;
use crate::validate::{Candidate, StructuralConfig, StructuralValidator, Verdict};

/// A function that extracts a source-claimed time from a signal for the
/// Layer-1 future-dated check — the per-adapter `nws_/rss_/calendar_claimed_time`.
pub type ClaimedTimeFn = fn(&RawSignal) -> Option<UtcTimestamp>;

/// A function that extracts a source's advertised next-release time from a
/// signal — the OPT-IN release-aware cadence hint (F4b, contract §3.4). The
/// analogue of [`ClaimedTimeFn`], but read AFTER a fetch to schedule the next
/// poll just after the advertised publish. The only wired implementor is
/// `aeolus::aeolus_next_run_at`; a source with no hint keeps its steady cadence.
pub type ReleaseHintFn = fn(&RawSignal) -> Option<UtcTimestamp>;

/// Schedule the next poll JUST AFTER an advertised release (F4b, §3.4), clamped
/// to a sane band so a missing/past/absurd hint can never break the steady
/// cadence. Pure (epoch-millis in, epoch-millis out — no clock read): given the
/// advertised `next_run_ms`, poll at `next_run_ms + lead` (arrive just after the
/// publish), but never sooner than `now + MIN_FLOOR` (a past/imminent hint →
/// poll at the floor soon) and never later than `now + 2·base` (an absurdly-far
/// hint → cap at ~2 steady intervals as a heartbeat).
fn release_aware_due_ms(next_run_ms: i64, now_ms: i64, base: Duration, lead: Duration) -> i64 {
    let target = next_run_ms.saturating_add(lead.as_millis() as i64);
    let floor = now_ms.saturating_add(MIN_FLOOR.as_millis() as i64);
    let cap = now_ms.saturating_add((base.as_millis() as i64).saturating_mul(2));
    target.clamp(floor, cap)
}

/// Arrive just AFTER the advertised publish, not exactly at it.
const RELEASE_LEAD: Duration = Duration::from_secs(90);
/// Never poll sooner than this after `now`, even on a past/imminent hint.
const MIN_FLOOR: Duration = Duration::from_secs(30);

/// Per-source runtime schedule + policy.
#[derive(Debug, Clone)]
pub struct SourceSchedule {
    /// Steady-state poll interval.
    pub base_interval: Duration,
    /// Time-of-day windows (UTC) during which polling boosts to
    /// `boosted_interval`. Day-set restriction (only CPI days, etc.) is a
    /// Phase-B refinement; D9 applies the window every day.
    pub event_windows: Vec<EventWindow>,
    pub boosted_interval: Duration,
    /// Consecutive fetch failures that trip quarantine.
    pub quarantine_after: u32,
    /// First backoff step; doubles per failure, capped at `backoff_cap`.
    pub backoff_base: Duration,
    pub backoff_cap: Duration,
    /// Trust tier (from the source_registry) and the trigger floor: a signal
    /// `wakes_decision_cycle` only when `trust_tier >= trigger_floor`.
    pub trust_tier: u8,
    pub trigger_floor: u8,
    /// Domain tags (weather | macro | …) from the source_registry admission;
    /// surfaced in telemetry.
    pub domain_tags: Vec<String>,
}

impl SourceSchedule {
    /// A conservative default for a steady source (no boosting).
    pub fn steady(base_interval: Duration, trust_tier: u8, trigger_floor: u8) -> SourceSchedule {
        SourceSchedule {
            base_interval,
            event_windows: Vec::new(),
            boosted_interval: base_interval,
            quarantine_after: 5,
            backoff_base: Duration::from_secs(30),
            backoff_cap: Duration::from_secs(3600),
            trust_tier,
            trigger_floor,
            domain_tags: Vec::new(),
        }
    }
}

/// Per-source health. Quarantine is loud and only an operator `rearm` clears it
/// (no auto-resume — the spirit of I2's human re-arm).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Degraded { consecutive_failures: u32 },
    Quarantined,
}

/// First-class per-source telemetry (operator request). Everything the deter-
/// ministic core can observe; per-fetch latency and true 304-rate need a
/// Source-trait extension and are added at the real-transport layer (the
/// adapter returns an empty `Vec` on 304, so `empty_polls` is the proxy here).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMetrics {
    pub polls: u64,
    pub empty_polls: u64,
    pub fetch_errors: u64,
    pub accepted: u64,
    pub dropped_future: u64,
    pub dropped_republished: u64,
    pub dropped_over_volume: u64,
    pub quarantines: u64,
    pub rearms: u64,
}

/// One accepted signal, tagged with whether it may wake a decision cycle.
#[derive(Debug, Clone)]
pub struct AcceptedSignal {
    pub source: String,
    pub signal: RawSignal,
    pub wakes_decision_cycle: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    Future,
    Republished,
    OverVolume,
}

/// A refused signal — recorded, never passed downstream (the hard gate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub source: String,
    pub content_hash: String,
    pub reason: DropReason,
}

/// A loud health event for the operator (Slack via the D10 seam).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    Quarantined { source: String, reason: String },
    Recovered { source: String },
}

/// The result of one scheduler tick.
#[derive(Debug, Default)]
pub struct TickOutcome {
    pub accepted: Vec<AcceptedSignal>,
    pub dropped: Vec<Dropped>,
    pub alerts: Vec<Alert>,
}

/// Per-source live state + counters, projected for the operator/ROTA
/// (ingestion-observability-contract §2). A pure projection of `Registered` —
/// no wall-clock, no secrets.
#[derive(Debug, Clone)]
pub struct SourceTelemetry {
    pub source_id: String,
    /// Last-seen signal kind; `""` until the first signal is observed.
    pub kind: String,
    /// Domain tags (weather | macro | …) from the source_registry admission.
    pub domain_tags: Vec<String>,
    pub trust_tier: u8,
    /// `"healthy"` | `"degraded"` | `"quarantined"`.
    pub health: &'static str,
    pub last_poll_at: Option<String>,
    pub last_success_at: Option<String>,
    pub next_due_at: Option<String>,
    pub polls: u64,
    pub empty_polls: u64,
    pub fetch_errors: u64,
    pub accepted: u64,
    pub dropped_future: u64,
    pub dropped_republished: u64,
    pub dropped_over_volume: u64,
    pub quarantines: u64,
    pub rearms: u64,
    /// Redacted + capped last fetch error (never secrets/tokens).
    pub last_error: Option<String>,
}

/// Process-wide funnel stage totals since boot (ingestion-observability-contract
/// §2). This slice derives the validate-stage totals from per-source metrics;
/// the loop-side stages (`normalized`/`deduped`/`persisted`/`persist_failures`)
/// stay 0 here and are set by the ingestion loop downstream.
#[derive(Debug, Clone, Default)]
pub struct FunnelCounts {
    pub fetched: u64,
    pub validated_accepted: u64,
    pub validated_dropped: u64,
    pub normalized: u64,
    pub deduped: u64,
    pub persisted: u64,
    pub persist_failures: u64,
}

/// One entry in the live signal feed — DATA, redacted (untrusted content stays
/// quoted, never interpreted; spec 5.11).
#[derive(Debug, Clone)]
pub struct SignalRecord {
    /// `signal.received_at.to_iso8601()`.
    pub at: String,
    pub source_id: String,
    pub kind: String,
    pub claimed_time: Option<String>,
    /// `"accepted"` | `"dropped:future"` | `"dropped:republished"` |
    /// `"dropped:over_volume"`.
    pub status: String,
    /// Redacted + truncated projection of the payload.
    pub summary: String,
}

/// The most recent tick's outcome counts.
#[derive(Debug, Clone, Default)]
pub struct TickTelemetry {
    pub at: String,
    pub accepted: usize,
    pub dropped: usize,
    pub alerts: usize,
}

/// A live ingestion telemetry snapshot. ONE writer (the ingestion loop), many
/// readers (the Prometheus renderer + ROTA handlers). A pure projection — the
/// `generated_at` clock is injected by the caller (never wall-clock here).
/// `Default` is the empty pre-first-tick snapshot a published handle starts at
/// (empty `generated_at` => "not yet generated"; readers degrade gracefully).
#[derive(Debug, Clone, Default)]
pub struct IngestionTelemetry {
    pub generated_at: String,
    pub sources: Vec<SourceTelemetry>,
    pub funnel: FunnelCounts,
    /// Newest-first, bounded to `RECENT_CAP`.
    pub recent: Vec<SignalRecord>,
    pub last_tick: TickTelemetry,
}

struct Registered {
    id: String,
    source: Box<dyn Source>,
    schedule: SourceSchedule,
    claimed_time: ClaimedTimeFn,
    /// OPT-IN release-aware cadence hint (F4b, §3.4); `None` keeps the steady
    /// config cadence byte-for-byte. Set post-`register` via `set_release_hint`.
    release_hint: Option<ReleaseHintFn>,
    validator: StructuralValidator,
    health: Health,
    /// Epoch millis of the next due poll; `i64::MIN` means "due now".
    next_due_ms: i64,
    consecutive_failures: u32,
    metrics: SourceMetrics,
    /// Epoch millis of the last poll (set when the source is polled).
    last_poll_ms: Option<i64>,
    /// Epoch millis of the last successful fetch.
    last_success_ms: Option<i64>,
    /// Redacted + capped last fetch error; cleared on success.
    last_error: Option<String>,
    /// Last-seen signal kind.
    last_kind: Option<String>,
}

/// Drives a fleet of sources. Construct, `register` each source, then `tick`.
#[derive(Default)]
pub struct IngestionScheduler {
    sources: Vec<Registered>,
    /// Ring buffer of the most recent signal records (newest at the back),
    /// bounded to `RECENT_CAP`. Read newest-first by `telemetry`.
    recent: std::collections::VecDeque<SignalRecord>,
    /// The most recent tick's outcome counts.
    last_tick: TickTelemetry,
}

/// Cap on the live signal-feed ring buffer.
const RECENT_CAP: usize = 256;

impl IngestionScheduler {
    pub fn new() -> IngestionScheduler {
        IngestionScheduler {
            sources: Vec::new(),
            recent: std::collections::VecDeque::new(),
            last_tick: TickTelemetry::default(),
        }
    }

    /// Register a source. It is due on the first tick.
    pub fn register(
        &mut self,
        id: impl Into<String>,
        source: Box<dyn Source>,
        schedule: SourceSchedule,
        claimed_time: ClaimedTimeFn,
        validator_config: StructuralConfig,
    ) {
        self.sources.push(Registered {
            id: id.into(),
            source,
            schedule,
            claimed_time,
            release_hint: None,
            validator: StructuralValidator::new(validator_config),
            health: Health::Healthy,
            next_due_ms: i64::MIN,
            consecutive_failures: 0,
            metrics: SourceMetrics::default(),
            last_poll_ms: None,
            last_success_ms: None,
            last_error: None,
            last_kind: None,
        });
    }

    /// Opt a registered source into release-aware cadence (F4b, §3.4): after
    /// each fetch its next poll is scheduled just after the advertised next
    /// release (via `hint`) instead of the steady config interval. Sources
    /// without a hint are unchanged. No-op (never panics) if `id` is unknown —
    /// the factory calls this only for ids it just registered.
    pub fn set_release_hint(&mut self, id: &str, hint: ReleaseHintFn) {
        if let Some(reg) = self.sources.iter_mut().find(|r| r.id == id) {
            reg.release_hint = Some(hint);
        }
    }

    /// Poll every source due at `now`, validate each fetched item, and return
    /// the outcome. Per-source isolation: one source's failure never aborts the
    /// others.
    pub async fn tick(&mut self, now: UtcTimestamp) -> TickOutcome {
        let now_ms = now.epoch_millis();
        let mut out = TickOutcome::default();
        // Records produced this tick. Built locally because `self.recent` cannot
        // be borrowed while `self.sources` is mutably borrowed by the loop;
        // drained into the ring buffer after the loop.
        let mut fresh: Vec<SignalRecord> = Vec::new();
        for reg in &mut self.sources {
            if reg.health == Health::Quarantined || now_ms < reg.next_due_ms {
                continue;
            }
            reg.metrics.polls += 1;
            reg.last_poll_ms = Some(now_ms);
            reg.validator.begin_tick();
            match reg.source.fetch().await {
                Err(e) => {
                    reg.metrics.fetch_errors += 1;
                    reg.last_error = Some(redact_error(&e.to_string()));
                    reg.consecutive_failures += 1;
                    if reg.consecutive_failures >= reg.schedule.quarantine_after {
                        reg.health = Health::Quarantined;
                        reg.metrics.quarantines += 1;
                        out.alerts.push(Alert::Quarantined {
                            source: reg.id.clone(),
                            reason: e.to_string(),
                        });
                    } else {
                        reg.health = Health::Degraded {
                            consecutive_failures: reg.consecutive_failures,
                        };
                        let backoff = backoff_for(&reg.schedule, reg.consecutive_failures);
                        reg.next_due_ms = now_ms.saturating_add(backoff.as_millis() as i64);
                    }
                }
                Ok(signals) => {
                    reg.last_success_ms = Some(now_ms);
                    reg.last_error = None;
                    if reg.consecutive_failures > 0 {
                        reg.consecutive_failures = 0;
                        reg.health = Health::Healthy;
                        out.alerts.push(Alert::Recovered {
                            source: reg.id.clone(),
                        });
                    }
                    if signals.is_empty() {
                        reg.metrics.empty_polls += 1;
                    }
                    let wakes = reg.schedule.trust_tier >= reg.schedule.trigger_floor;
                    // F4b: the MAX advertised next-release epoch-ms seen this
                    // poll (opt-in; `None` for sources without a release hint and
                    // for any item that does not carry one — steady cadence then).
                    let mut release_at: Option<i64> = None;
                    for signal in signals {
                        let hash = content_hash(&signal.payload);
                        let claimed = (reg.claimed_time)(&signal);
                        // Read the release hint BEFORE the match moves `signal`.
                        if let Some(hint) = reg.release_hint {
                            if let Some(next_run) = hint(&signal) {
                                let ms = next_run.epoch_millis();
                                release_at = Some(release_at.map_or(ms, |cur| cur.max(ms)));
                            }
                        }
                        // Capture the small, redacted bits BEFORE the match (the
                        // `signal` itself is moved into AcceptedSignal in the
                        // Accept arm; the payload is never dumped wholesale).
                        let kind = signal.kind.clone();
                        let at = signal.received_at.to_iso8601();
                        let claimed_iso = claimed.map(|t| t.to_iso8601());
                        let summary = summarize(&signal.payload);
                        reg.last_kind = Some(kind.clone());
                        let candidate = Candidate {
                            content_hash: hash.clone(),
                            claimed_time: claimed,
                        };
                        let status = match reg.validator.assess(now, &candidate) {
                            Verdict::Accept => {
                                reg.metrics.accepted += 1;
                                out.accepted.push(AcceptedSignal {
                                    source: reg.id.clone(),
                                    signal,
                                    wakes_decision_cycle: wakes,
                                });
                                "accepted"
                            }
                            Verdict::RejectFuture { .. } => {
                                reg.metrics.dropped_future += 1;
                                out.dropped.push(Dropped {
                                    source: reg.id.clone(),
                                    content_hash: hash,
                                    reason: DropReason::Future,
                                });
                                "dropped:future"
                            }
                            Verdict::RejectRepublished => {
                                reg.metrics.dropped_republished += 1;
                                out.dropped.push(Dropped {
                                    source: reg.id.clone(),
                                    content_hash: hash,
                                    reason: DropReason::Republished,
                                });
                                "dropped:republished"
                            }
                            Verdict::RejectOverVolume { .. } => {
                                reg.metrics.dropped_over_volume += 1;
                                out.dropped.push(Dropped {
                                    source: reg.id.clone(),
                                    content_hash: hash,
                                    reason: DropReason::OverVolume,
                                });
                                "dropped:over_volume"
                            }
                        };
                        fresh.push(SignalRecord {
                            at,
                            source_id: reg.id.clone(),
                            kind,
                            claimed_time: claimed_iso,
                            status: status.to_string(),
                            summary,
                        });
                    }
                    // F4b release-aware cadence (§3.4): when a release hint was
                    // seen, poll just after the advertised next run (clamped to a
                    // sane band); otherwise the steady config interval — the
                    // `None` arm is byte-identical to the pre-F4b behavior, so a
                    // source without a hint is completely unchanged.
                    reg.next_due_ms = match release_at {
                        Some(next_run_ms) => release_aware_due_ms(
                            next_run_ms,
                            now_ms,
                            reg.schedule.base_interval,
                            RELEASE_LEAD,
                        ),
                        None => {
                            let interval = interval_at(&reg.schedule, now);
                            now_ms.saturating_add(interval.as_millis() as i64)
                        }
                    };
                }
            }
        }
        self.recent.extend(fresh);
        while self.recent.len() > RECENT_CAP {
            self.recent.pop_front();
        }
        self.last_tick = TickTelemetry {
            at: now.to_iso8601(),
            accepted: out.accepted.len(),
            dropped: out.dropped.len(),
            alerts: out.alerts.len(),
        };
        out
    }

    /// The earliest time any non-quarantined source is next due — the D10 loop
    /// sleeps until then. `None` when every source is quarantined.
    pub fn next_wake(&self) -> Option<UtcTimestamp> {
        self.sources
            .iter()
            .filter(|r| r.health != Health::Quarantined)
            .map(|r| r.next_due_ms.max(0))
            .min()
            .and_then(|ms| UtcTimestamp::from_epoch_millis(ms).ok())
    }

    /// Operator re-enable of a quarantined source. Returns false if unknown.
    pub fn rearm(&mut self, source_id: &str) -> bool {
        for reg in &mut self.sources {
            if reg.id == source_id {
                reg.health = Health::Healthy;
                reg.consecutive_failures = 0;
                reg.next_due_ms = i64::MIN;
                reg.metrics.rearms += 1;
                return true;
            }
        }
        false
    }

    /// Test-only: whether a source has an opt-in release-aware cadence hint
    /// (F4b). Lets the factory test assert the wiring without a live fetch.
    #[cfg(test)]
    pub(crate) fn has_release_hint(&self, source_id: &str) -> bool {
        self.sources
            .iter()
            .find(|r| r.id == source_id)
            .map(|r| r.release_hint.is_some())
            .unwrap_or(false)
    }

    pub fn health(&self, source_id: &str) -> Option<&Health> {
        self.sources
            .iter()
            .find(|r| r.id == source_id)
            .map(|r| &r.health)
    }

    pub fn metrics(&self, source_id: &str) -> Option<&SourceMetrics> {
        self.sources
            .iter()
            .find(|r| r.id == source_id)
            .map(|r| &r.metrics)
    }

    /// Registered source ids, in registration order (introspection/telemetry).
    pub fn source_ids(&self) -> Vec<&str> {
        self.sources.iter().map(|r| r.id.as_str()).collect()
    }

    /// A live telemetry snapshot (ingestion-observability-contract §2): per-source
    /// health + counters + timestamps, the process-wide funnel, the recent signal
    /// feed (newest-first, bounded), and the last tick's outcome. A pure
    /// projection — `generated_at` is the injected `Clock`'s `now` (never wall
    /// time), and the `summary`/`last_error` projections are redacted.
    pub fn telemetry(&self, generated_at: UtcTimestamp) -> IngestionTelemetry {
        let iso = |ms: i64| {
            UtcTimestamp::from_epoch_millis(ms)
                .ok()
                .map(|t| t.to_iso8601())
        };
        let mut funnel = FunnelCounts::default();
        let sources = self
            .sources
            .iter()
            .map(|reg| {
                let health = match reg.health {
                    Health::Healthy => "healthy",
                    Health::Degraded { .. } => "degraded",
                    Health::Quarantined => "quarantined",
                };
                let next_due_at = if reg.next_due_ms == i64::MIN {
                    None
                } else {
                    iso(reg.next_due_ms.max(0))
                };
                let m = &reg.metrics;
                funnel.validated_accepted += m.accepted;
                funnel.validated_dropped +=
                    m.dropped_future + m.dropped_republished + m.dropped_over_volume;
                SourceTelemetry {
                    source_id: reg.id.clone(),
                    kind: reg.last_kind.clone().unwrap_or_default(),
                    domain_tags: reg.schedule.domain_tags.clone(),
                    trust_tier: reg.schedule.trust_tier,
                    health,
                    last_poll_at: reg.last_poll_ms.and_then(iso),
                    last_success_at: reg.last_success_ms.and_then(iso),
                    next_due_at,
                    polls: m.polls,
                    empty_polls: m.empty_polls,
                    fetch_errors: m.fetch_errors,
                    accepted: m.accepted,
                    dropped_future: m.dropped_future,
                    dropped_republished: m.dropped_republished,
                    dropped_over_volume: m.dropped_over_volume,
                    quarantines: m.quarantines,
                    rearms: m.rearms,
                    last_error: reg.last_error.clone(),
                }
            })
            .collect();
        // The validate stages are summed above; `fetched` is their sum. The
        // downstream loop stages stay 0 here (set by the ingestion loop).
        funnel.fetched = funnel.validated_accepted + funnel.validated_dropped;
        IngestionTelemetry {
            generated_at: generated_at.to_iso8601(),
            sources,
            funnel,
            recent: self.recent.iter().rev().cloned().collect(),
            last_tick: self.last_tick.clone(),
        }
    }
}

/// SHA-256 hex of the canonical JSON payload (serde_json `Map` is sorted-key by
/// default, so this is stable). The validator's republication flag uses it; the
/// AUTHORITATIVE dedup remains the ledger's `UNIQUE(source, content_hash)`.
fn content_hash(payload: &Value) -> String {
    let bytes = serde_json::to_vec(payload).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    let mut s = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Redact + cap a fetch-error message for telemetry. These are non-secret
/// venue/HTTP errors, so nothing is stripped — but the message is TRUNCATED to
/// <= 200 chars so an adversarial/oversized error string can never bloat the
/// snapshot (defence-in-depth for the untrusted-data doctrine, spec 5.11).
fn redact_error(message: &str) -> String {
    message.chars().take(200).collect()
}

/// A short, redacted projection of an (untrusted) payload for the live feed.
/// Tries a small allowlist of human-meaningful string keys at the top level,
/// then under a nested `"properties"` object (NWS GeoJSON), returning the FIRST
/// non-empty string found, truncated to 120 chars. Never serializes the whole
/// payload; on no match returns only a structural hint. The returned value is
/// plain DATA — the renderer quotes it, it is never interpreted (spec 5.11).
fn summarize(payload: &Value) -> String {
    const KEYS: [&str; 7] = [
        "event",
        "title",
        "headline",
        "summary",
        "report_date",
        "variable",
        "name",
    ];
    let pick = |obj: &Value| -> Option<String> {
        for key in KEYS {
            if let Some(s) = obj.get(key).and_then(Value::as_str) {
                if !s.is_empty() {
                    return Some(s.chars().take(120).collect());
                }
            }
        }
        None
    };
    if let Some(s) = pick(payload) {
        return s;
    }
    if let Some(props) = payload.get("properties") {
        if let Some(s) = pick(props) {
            return s;
        }
    }
    if payload.is_object() {
        "<object>".to_string()
    } else if payload.is_array() {
        "<array>".to_string()
    } else {
        "<scalar>".to_string()
    }
}

/// Deterministic exponential backoff, capped. No random jitter — determinism
/// for DST/replay; one daemon's handful of sources do not need herd-avoidance.
fn backoff_for(schedule: &SourceSchedule, consecutive_failures: u32) -> Duration {
    let base = schedule.backoff_base;
    let shift = consecutive_failures.saturating_sub(1).min(16);
    let scaled = base.saturating_mul(1u32 << shift);
    scaled.min(schedule.backoff_cap)
}

/// The poll interval at `now`: boosted if `now`'s UTC time-of-day falls in any
/// event window, else the base interval.
fn interval_at(schedule: &SourceSchedule, now: UtcTimestamp) -> Duration {
    let secs_of_day = utc_seconds_of_day(now);
    for w in &schedule.event_windows {
        let from = w.from.num_seconds_from_midnight();
        let to = w.to.num_seconds_from_midnight();
        if secs_of_day >= from && secs_of_day <= to {
            return schedule.boosted_interval;
        }
    }
    schedule.base_interval
}

/// UTC seconds-since-midnight for a timestamp, from epoch millis (no wall time).
fn utc_seconds_of_day(now: UtcTimestamp) -> u32 {
    let ms = now.epoch_millis();
    let ms_in_day = ms.rem_euclid(86_400_000);
    (ms_in_day / 1000) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::NaiveTime;
    use fortuna_cognition::signals::SignalError;
    use std::sync::Mutex;

    fn ts(ms: i64) -> UtcTimestamp {
        UtcTimestamp::from_epoch_millis(ms).unwrap()
    }

    fn sig(kind: &str, id: &str, claimed_ms: Option<i64>) -> RawSignal {
        let payload = match claimed_ms {
            Some(ms) => serde_json::json!({"id": id, "claimed": ts(ms).to_iso8601()}),
            None => serde_json::json!({"id": id}),
        };
        RawSignal {
            kind: kind.to_string(),
            payload,
            received_at: ts(0),
        }
    }

    /// Reads a `claimed` ISO8601 field if present (test stand-in for the real
    /// per-adapter extractors).
    fn test_claimed(s: &RawSignal) -> Option<UtcTimestamp> {
        s.payload
            .get("claimed")
            .and_then(Value::as_str)
            .and_then(|t| UtcTimestamp::parse_iso8601(t).ok())
    }

    /// A scripted source: each `fetch` pops the next scripted result.
    struct ScriptedSource {
        id: String,
        script: Mutex<std::collections::VecDeque<Result<Vec<RawSignal>, SignalError>>>,
    }

    impl ScriptedSource {
        fn new(id: &str, script: Vec<Result<Vec<RawSignal>, SignalError>>) -> ScriptedSource {
            ScriptedSource {
                id: id.to_string(),
                script: Mutex::new(script.into()),
            }
        }
    }

    #[async_trait]
    impl Source for ScriptedSource {
        fn id(&self) -> &str {
            &self.id
        }
        async fn fetch(&mut self) -> Result<Vec<RawSignal>, SignalError> {
            self.script
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(Vec::new()))
        }
    }

    fn err(reason: &str) -> Result<Vec<RawSignal>, SignalError> {
        Err(SignalError::Fetch {
            source_id: "x".into(),
            reason: reason.into(),
        })
    }

    fn sched(base_secs: u64, tier: u8, floor: u8) -> SourceSchedule {
        SourceSchedule::steady(Duration::from_secs(base_secs), tier, floor)
    }

    // --- the HARD GATE: the validator is wired; rejects are refused ----------

    #[tokio::test]
    async fn future_dated_item_is_refused_not_ingested() {
        // now = 1000s; a signal claiming 1 day in the future (beyond tolerance).
        let now = ts(1_000_000);
        let future = sig("x", "a", Some(1_000_000 + 100_000_000));
        let fresh = sig("x", "b", None);
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            Box::new(ScriptedSource::new("src", vec![Ok(vec![future, fresh])])),
            sched(60, 9, 5),
            test_claimed,
            StructuralConfig::default(),
        );
        let out = s.tick(now).await;
        // The future item is DROPPED, the fresh one ACCEPTED.
        assert_eq!(out.accepted.len(), 1);
        assert_eq!(out.accepted[0].signal.payload["id"], "b");
        assert_eq!(out.dropped.len(), 1);
        assert_eq!(out.dropped[0].reason, DropReason::Future);
        assert_eq!(s.metrics("src").unwrap().dropped_future, 1);
    }

    #[tokio::test]
    async fn republished_and_over_volume_are_refused() {
        let now = ts(0);
        let cfg = StructuralConfig {
            volume_envelope: 1, // accept 1 per tick
            ..Default::default()
        };
        let dup = sig("x", "same", None);
        // 3 items: first accepted, second is over-volume, a later identical is republish.
        let a = sig("x", "a", None);
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            Box::new(ScriptedSource::new(
                "src",
                vec![Ok(vec![a, dup.clone(), dup.clone()])],
            )),
            sched(60, 9, 5),
            test_claimed,
            cfg,
        );
        let out = s.tick(now).await;
        assert_eq!(out.accepted.len(), 1, "only the envelope's worth accepted");
        // The remaining two are refused (over-volume and/or republished).
        assert_eq!(out.dropped.len(), 2);
        let m = s.metrics("src").unwrap();
        assert_eq!(m.accepted, 1);
        assert_eq!(m.dropped_over_volume + m.dropped_republished, 2);
    }

    // --- trigger floor tagging -----------------------------------------------

    #[tokio::test]
    async fn trigger_floor_tags_wake_eligibility() {
        let now = ts(0);
        let mut s = IngestionScheduler::new();
        s.register(
            "high",
            Box::new(ScriptedSource::new(
                "high",
                vec![Ok(vec![sig("x", "a", None)])],
            )),
            sched(60, 9, 5), // tier 9 >= floor 5 -> wakes
            test_claimed,
            StructuralConfig::default(),
        );
        s.register(
            "low",
            Box::new(ScriptedSource::new(
                "low",
                vec![Ok(vec![sig("x", "b", None)])],
            )),
            sched(60, 3, 5), // tier 3 < floor 5 -> does not wake
            test_claimed,
            StructuralConfig::default(),
        );
        let out = s.tick(now).await;
        let high = out.accepted.iter().find(|a| a.source == "high").unwrap();
        let low = out.accepted.iter().find(|a| a.source == "low").unwrap();
        assert!(high.wakes_decision_cycle);
        assert!(!low.wakes_decision_cycle);
    }

    // --- cadence -------------------------------------------------------------

    #[tokio::test]
    async fn not_due_sources_are_skipped_until_their_interval() {
        let now = ts(0);
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            Box::new(ScriptedSource::new(
                "src",
                vec![Ok(vec![sig("x", "a", None)]), Ok(vec![sig("x", "b", None)])],
            )),
            sched(60, 9, 5), // 60s interval
            test_claimed,
            StructuralConfig::default(),
        );
        assert_eq!(s.tick(now).await.accepted.len(), 1); // due on first tick
                                                         // 30s later: not due (interval 60s) -> no poll.
        assert_eq!(s.tick(ts(30_000)).await.accepted.len(), 0);
        // 60s later: due again.
        assert_eq!(s.tick(ts(60_000)).await.accepted.len(), 1);
    }

    #[test]
    fn event_window_boosts_the_interval() {
        let mut sc = sched(3600, 9, 5); // base 1h
        sc.boosted_interval = Duration::from_secs(10);
        sc.event_windows = vec![EventWindow {
            days_ref: "daily".into(),
            from: NaiveTime::from_hms_opt(12, 25, 0).unwrap(),
            to: NaiveTime::from_hms_opt(12, 40, 0).unwrap(),
            interval: Duration::from_secs(10),
        }];
        // 12:30:00 UTC = 45000s into the day.
        let in_window = ts(45_000 * 1000);
        assert_eq!(interval_at(&sc, in_window), Duration::from_secs(10));
        // 06:00 UTC -> base.
        let outside = ts(21_600 * 1000);
        assert_eq!(interval_at(&sc, outside), Duration::from_secs(3600));
    }

    // --- health state machine + isolation ------------------------------------

    #[tokio::test]
    async fn failures_degrade_then_quarantine_loudly_then_rearm() {
        let now0 = ts(0);
        let mut sc = sched(1, 9, 5);
        sc.quarantine_after = 2;
        sc.backoff_base = Duration::from_secs(1);
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            Box::new(ScriptedSource::new(
                "src",
                vec![err("boom"), err("boom"), Ok(vec![sig("x", "a", None)])],
            )),
            sc,
            test_claimed,
            StructuralConfig::default(),
        );
        // First failure -> degraded, backoff 1s.
        let o1 = s.tick(now0).await;
        assert!(o1.alerts.is_empty());
        assert!(matches!(
            s.health("src").unwrap(),
            Health::Degraded {
                consecutive_failures: 1
            }
        ));
        // Second failure (after backoff) -> quarantine + loud alert.
        let o2 = s.tick(ts(2_000)).await;
        assert!(matches!(o2.alerts[0], Alert::Quarantined { .. }));
        assert_eq!(s.health("src").unwrap(), &Health::Quarantined);
        // Quarantined: not polled even when due.
        assert!(s.tick(ts(10_000)).await.accepted.is_empty());
        // Operator re-arm -> healthy, polls again (no Recovered alert: rearm is
        // the operator action, not a degraded->healthy auto-recovery).
        assert!(s.rearm("src"));
        let o3 = s.tick(ts(20_000)).await;
        assert_eq!(o3.accepted.len(), 1);
        assert_eq!(s.health("src").unwrap(), &Health::Healthy);
        assert!(o3.alerts.is_empty());
        assert_eq!(s.metrics("src").unwrap().rearms, 1);
    }

    #[tokio::test]
    async fn degraded_then_success_emits_a_recovered_alert() {
        let mut sc = sched(1, 9, 5);
        sc.quarantine_after = 5; // high, so one failure only degrades
        sc.backoff_base = Duration::from_secs(1);
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            Box::new(ScriptedSource::new(
                "src",
                vec![err("blip"), Ok(vec![sig("x", "a", None)])],
            )),
            sc,
            test_claimed,
            StructuralConfig::default(),
        );
        let o1 = s.tick(ts(0)).await; // fail -> degraded
        assert!(o1.alerts.is_empty());
        let o2 = s.tick(ts(5_000)).await; // success after backoff -> recovered
        assert_eq!(o2.accepted.len(), 1);
        assert!(o2
            .alerts
            .iter()
            .any(|a| matches!(a, Alert::Recovered { .. })));
        assert_eq!(s.health("src").unwrap(), &Health::Healthy);
    }

    #[tokio::test]
    async fn one_failing_source_does_not_block_the_fleet() {
        let now = ts(0);
        let mut s = IngestionScheduler::new();
        s.register(
            "bad",
            Box::new(ScriptedSource::new("bad", vec![err("down")])),
            sched(60, 9, 5),
            test_claimed,
            StructuralConfig::default(),
        );
        s.register(
            "good",
            Box::new(ScriptedSource::new(
                "good",
                vec![Ok(vec![sig("x", "a", None)])],
            )),
            sched(60, 9, 5),
            test_claimed,
            StructuralConfig::default(),
        );
        let out = s.tick(now).await;
        assert_eq!(out.accepted.len(), 1, "the good source still ingests");
        assert_eq!(s.metrics("bad").unwrap().fetch_errors, 1);
    }

    #[tokio::test]
    async fn next_wake_reflects_due_times_and_skips_quarantined() {
        let now = ts(0);
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            Box::new(ScriptedSource::new("src", vec![Ok(vec![])])),
            sched(60, 9, 5),
            test_claimed,
            StructuralConfig::default(),
        );
        s.tick(now).await;
        // After a poll at t=0 with 60s interval, next wake ~ 60_000ms.
        assert_eq!(s.next_wake().unwrap().epoch_millis(), 60_000);
    }

    // --- §2 telemetry data surface -------------------------------------------

    /// A signal whose payload carries an arbitrary JSON object (for the
    /// summary/redaction tests). `claimed` (if present) is the ISO future-time.
    fn payload_sig(kind: &str, payload: Value) -> RawSignal {
        RawSignal {
            kind: kind.to_string(),
            payload,
            received_at: ts(0),
        }
    }

    fn one_source(
        s: &mut IngestionScheduler,
        id: &str,
        script: Vec<Result<Vec<RawSignal>, SignalError>>,
        cfg: StructuralConfig,
    ) {
        s.register(
            id,
            Box::new(ScriptedSource::new(id, script)),
            sched(60, 9, 5),
            test_claimed,
            cfg,
        );
    }

    #[tokio::test]
    async fn telemetry_projects_per_source_health_and_counters() {
        let now = ts(1_000_000);
        let mut s = IngestionScheduler::new();
        one_source(
            &mut s,
            "src",
            vec![Ok(vec![sig("x", "a", None)])],
            StructuralConfig::default(),
        );
        s.tick(now).await;
        let t = s.telemetry(now);
        let src = t
            .sources
            .iter()
            .find(|st| st.source_id == "src")
            .expect("source present in telemetry");
        assert_eq!(src.health, "healthy");
        assert!(src.polls >= 1);
        assert!(src.accepted >= 1);
        assert!(src.last_success_at.is_some());
        assert!(src.last_poll_at.is_some());
        // generated_at is the injected clock, not wall-time.
        assert_eq!(t.generated_at, now.to_iso8601());
    }

    #[tokio::test]
    async fn telemetry_recent_feed_has_accepted_and_dropped_with_status_and_summary() {
        // now = 1000s; an acceptable signal plus a far-future-dated one (drops).
        let now = ts(1_000_000);
        let accept = payload_sig(
            "nws.alert",
            serde_json::json!({"event": "Severe Thunderstorm Warning"}),
        );
        let future = payload_sig(
            "nws.alert",
            serde_json::json!({"event": "Tornado Watch", "claimed": ts(1_000_000 + 100_000_000).to_iso8601()}),
        );
        let mut s = IngestionScheduler::new();
        one_source(
            &mut s,
            "src",
            vec![Ok(vec![accept, future])],
            StructuralConfig::default(),
        );
        s.tick(now).await;
        let t = s.telemetry(now);
        let accepted = t
            .recent
            .iter()
            .find(|r| r.status == "accepted")
            .expect("an accepted record");
        assert_eq!(accepted.summary, "Severe Thunderstorm Warning");
        assert_eq!(accepted.source_id, "src");
        assert_eq!(accepted.kind, "nws.alert");
        let dropped = t
            .recent
            .iter()
            .find(|r| r.status.starts_with("dropped:"))
            .expect("a dropped record");
        assert_eq!(dropped.status, "dropped:future");
        assert_eq!(dropped.summary, "Tornado Watch");
        assert!(dropped.claimed_time.is_some());
    }

    #[tokio::test]
    async fn telemetry_recent_is_bounded_to_cap() {
        let mut s = IngestionScheduler::new();
        // One accepted signal per tick; drive more than RECENT_CAP ticks.
        // A fresh content_hash each tick avoids the republication check.
        let ticks = RECENT_CAP + 50;
        let script: Vec<Result<Vec<RawSignal>, SignalError>> = (0..ticks)
            .map(|i| {
                Ok(vec![payload_sig(
                    "x",
                    serde_json::json!({"event": format!("evt-{i}")}),
                )])
            })
            .collect();
        one_source(&mut s, "src", script, StructuralConfig::default());
        // 60s interval -> advance now by 60s each tick so the source is due.
        for i in 0..ticks {
            s.tick(ts((i as i64) * 60_000)).await;
        }
        let t = s.telemetry(ts((ticks as i64) * 60_000));
        assert_eq!(t.recent.len(), RECENT_CAP);
        // Newest-first: the last accepted event is at the front.
        assert_eq!(t.recent[0].summary, format!("evt-{}", ticks - 1));
    }

    #[tokio::test]
    async fn telemetry_summary_truncates_untrusted_payload() {
        let now = ts(0);
        let big = "A".repeat(5000);
        let sig = payload_sig("x", serde_json::json!({"title": big}));
        let mut s = IngestionScheduler::new();
        one_source(
            &mut s,
            "src",
            vec![Ok(vec![sig])],
            StructuralConfig::default(),
        );
        s.tick(now).await;
        let t = s.telemetry(now);
        let rec = t.recent.first().expect("a record");
        assert!(rec.summary.chars().count() <= 120);
        assert_eq!(rec.summary.chars().count(), 120);
    }

    #[tokio::test]
    async fn telemetry_last_error_set_on_failure_then_cleared_on_success() {
        let mut sc = sched(1, 9, 5);
        sc.quarantine_after = 5; // a single failure only degrades
        sc.backoff_base = Duration::from_secs(1);
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            Box::new(ScriptedSource::new(
                "src",
                vec![err("boom-the-fetch-failed"), Ok(vec![sig("x", "a", None)])],
            )),
            sc,
            test_claimed,
            StructuralConfig::default(),
        );
        // Failing tick -> last_error is set.
        s.tick(ts(0)).await;
        let after_fail = s.telemetry(ts(0));
        let src = after_fail
            .sources
            .iter()
            .find(|st| st.source_id == "src")
            .unwrap();
        assert!(src.last_error.is_some());
        // Success tick (after backoff) -> last_error cleared.
        s.tick(ts(5_000)).await;
        let after_ok = s.telemetry(ts(5_000));
        let src = after_ok
            .sources
            .iter()
            .find(|st| st.source_id == "src")
            .unwrap();
        assert!(src.last_error.is_none());
    }

    #[tokio::test]
    async fn funnel_sums_per_source_metrics() {
        let now = ts(0);
        // volume_envelope 1 -> first accepted, the rest dropped (over-volume).
        let cfg = StructuralConfig {
            volume_envelope: 1,
            ..Default::default()
        };
        let a = payload_sig("x", serde_json::json!({"event": "a"}));
        let b = payload_sig("x", serde_json::json!({"event": "b"}));
        let c = payload_sig("x", serde_json::json!({"event": "c"}));
        let mut s = IngestionScheduler::new();
        one_source(&mut s, "src", vec![Ok(vec![a, b, c])], cfg);
        s.tick(now).await;
        let t = s.telemetry(now);
        assert!(t.funnel.validated_accepted > 0);
        assert!(t.funnel.validated_dropped > 0);
        assert_eq!(
            t.funnel.fetched,
            t.funnel.validated_accepted + t.funnel.validated_dropped
        );
        assert_eq!(t.funnel.validated_accepted, 1);
        assert_eq!(t.funnel.validated_dropped, 2);
    }

    // --- F4b release-aware cadence (opt-in; §3.4) ----------------------------

    /// A test release-hint that reads a `next_run` ISO8601 field (stand-in for
    /// the real `aeolus::aeolus_next_run_at`).
    fn test_release_hint(s: &RawSignal) -> Option<UtcTimestamp> {
        s.payload
            .get("next_run")
            .and_then(Value::as_str)
            .and_then(|t| UtcTimestamp::parse_iso8601(t).ok())
    }

    fn sig_with_next_run(id: &str, next_run_ms: i64) -> RawSignal {
        RawSignal {
            kind: "x".to_string(),
            payload: serde_json::json!({"id": id, "next_run": ts(next_run_ms).to_iso8601()}),
            received_at: ts(0),
        }
    }

    const LEAD_MS: i64 = 90_000; // RELEASE_LEAD
    const FLOOR_MS: i64 = 30_000; // MIN_FLOOR

    #[test]
    fn release_aware_due_target_within_band_is_next_run_plus_lead() {
        // base 1h; next_run 10m out -> 10m + 90s lead, comfortably in
        // (now+floor, now+2h).
        let base = Duration::from_secs(3600);
        let now_ms = 1_000_000;
        let next_run = now_ms + 600_000; // +10m
        let due = release_aware_due_ms(next_run, now_ms, base, Duration::from_secs(90));
        assert_eq!(due, next_run + LEAD_MS);
    }

    #[test]
    fn release_aware_due_past_next_run_clamps_to_floor() {
        // A next_run already in the past -> poll at now+floor (soon), never sooner.
        let base = Duration::from_secs(3600);
        let now_ms = 1_000_000;
        let next_run = now_ms - 500_000; // already past
        let due = release_aware_due_ms(next_run, now_ms, base, Duration::from_secs(90));
        assert_eq!(due, now_ms + FLOOR_MS);
    }

    #[test]
    fn release_aware_due_far_future_caps_at_two_base_intervals() {
        // An absurdly-far next_run -> cap at now + 2*base (a heartbeat).
        let base = Duration::from_secs(3600);
        let now_ms = 1_000_000;
        let next_run = now_ms + 10 * 24 * 3_600_000; // +10 days
        let due = release_aware_due_ms(next_run, now_ms, base, Duration::from_secs(90));
        assert_eq!(due, now_ms + 2 * 3_600_000);
    }

    #[tokio::test]
    async fn release_hint_schedules_next_poll_just_after_advertised_run() {
        // base 1h; a release hint advertising the next run 10m out. After the
        // tick, next_due lands at next_run + lead (inside the band), NOT at the
        // steady now+1h.
        let now = ts(1_000_000);
        let next_run_ms = 1_000_000 + 600_000; // +10m
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            Box::new(ScriptedSource::new(
                "src",
                vec![Ok(vec![sig_with_next_run("a", next_run_ms)])],
            )),
            sched(3600, 9, 5), // 1h steady
            test_claimed,
            StructuralConfig::default(),
        );
        s.set_release_hint("src", test_release_hint);
        let out = s.tick(now).await;
        assert_eq!(out.accepted.len(), 1);
        // next_wake is the release-aware due time, not now+1h.
        assert_eq!(s.next_wake().unwrap().epoch_millis(), next_run_ms + LEAD_MS);
    }

    #[tokio::test]
    async fn release_hint_makes_source_due_after_the_advertised_run_not_before() {
        // The source must NOT be polled at the old steady tick, only after the
        // release-aware due time. base 1h; next_run 10m out -> due at +10m+90s.
        let now = ts(0);
        let next_run_ms = 600_000; // +10m
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            Box::new(ScriptedSource::new(
                "src",
                vec![
                    Ok(vec![sig_with_next_run("a", next_run_ms)]),
                    Ok(vec![sig_with_next_run("b", next_run_ms + 600_000)]),
                ],
            )),
            sched(3600, 9, 5),
            test_claimed,
            StructuralConfig::default(),
        );
        s.set_release_hint("src", test_release_hint);
        assert_eq!(s.tick(now).await.accepted.len(), 1); // first poll
        let due = next_run_ms + LEAD_MS; // 690_000
                                         // Just before due: not polled.
        assert_eq!(s.tick(ts(due - 1)).await.accepted.len(), 0);
        // At due: polled again.
        assert_eq!(s.tick(ts(due)).await.accepted.len(), 1);
    }

    #[tokio::test]
    async fn source_without_release_hint_keeps_exact_steady_cadence() {
        // No set_release_hint call -> identical to the pre-F4b steady cadence:
        // a 60s-interval source is due again at exactly now+60s.
        let now = ts(0);
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            Box::new(ScriptedSource::new(
                "src",
                // Payload even carries a `next_run` field, but with no hint set
                // it is ignored -> steady cadence, proving opt-in.
                vec![Ok(vec![sig_with_next_run("a", 600_000)])],
            )),
            sched(60, 9, 5),
            test_claimed,
            StructuralConfig::default(),
        );
        s.tick(now).await;
        assert_eq!(s.next_wake().unwrap().epoch_millis(), 60_000);
    }

    #[tokio::test]
    async fn release_hint_returning_none_falls_back_to_steady_cadence() {
        // Hint set, but the item carries NO `next_run` field -> hint returns
        // None -> steady cadence (the None arm, byte-identical to pre-F4b).
        let now = ts(0);
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            Box::new(ScriptedSource::new(
                "src",
                vec![Ok(vec![sig("x", "a", None)])], // no next_run field
            )),
            sched(60, 9, 5),
            test_claimed,
            StructuralConfig::default(),
        );
        s.set_release_hint("src", test_release_hint);
        s.tick(now).await;
        assert_eq!(s.next_wake().unwrap().epoch_millis(), 60_000);
    }

    #[test]
    fn set_release_hint_on_unknown_source_is_a_no_op() {
        let mut s = IngestionScheduler::new();
        // No source registered; must not panic.
        s.set_release_hint("nope", test_release_hint);
        assert!(s.source_ids().is_empty());
    }
}
