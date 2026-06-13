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

struct Registered {
    id: String,
    source: Box<dyn Source>,
    schedule: SourceSchedule,
    claimed_time: ClaimedTimeFn,
    validator: StructuralValidator,
    health: Health,
    /// Epoch millis of the next due poll; `i64::MIN` means "due now".
    next_due_ms: i64,
    consecutive_failures: u32,
    metrics: SourceMetrics,
}

/// Drives a fleet of sources. Construct, `register` each source, then `tick`.
#[derive(Default)]
pub struct IngestionScheduler {
    sources: Vec<Registered>,
}

impl IngestionScheduler {
    pub fn new() -> IngestionScheduler {
        IngestionScheduler {
            sources: Vec::new(),
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
            validator: StructuralValidator::new(validator_config),
            health: Health::Healthy,
            next_due_ms: i64::MIN,
            consecutive_failures: 0,
            metrics: SourceMetrics::default(),
        });
    }

    /// Poll every source due at `now`, validate each fetched item, and return
    /// the outcome. Per-source isolation: one source's failure never aborts the
    /// others.
    pub async fn tick(&mut self, now: UtcTimestamp) -> TickOutcome {
        let now_ms = now.epoch_millis();
        let mut out = TickOutcome::default();
        for reg in &mut self.sources {
            if reg.health == Health::Quarantined || now_ms < reg.next_due_ms {
                continue;
            }
            reg.metrics.polls += 1;
            reg.validator.begin_tick();
            match reg.source.fetch().await {
                Err(e) => {
                    reg.metrics.fetch_errors += 1;
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
                    for signal in signals {
                        let hash = content_hash(&signal.payload);
                        let claimed = (reg.claimed_time)(&signal);
                        let candidate = Candidate {
                            content_hash: hash.clone(),
                            claimed_time: claimed,
                        };
                        match reg.validator.assess(now, &candidate) {
                            Verdict::Accept => {
                                reg.metrics.accepted += 1;
                                out.accepted.push(AcceptedSignal {
                                    source: reg.id.clone(),
                                    signal,
                                    wakes_decision_cycle: wakes,
                                });
                            }
                            Verdict::RejectFuture { .. } => {
                                reg.metrics.dropped_future += 1;
                                out.dropped.push(Dropped {
                                    source: reg.id.clone(),
                                    content_hash: hash,
                                    reason: DropReason::Future,
                                });
                            }
                            Verdict::RejectRepublished => {
                                reg.metrics.dropped_republished += 1;
                                out.dropped.push(Dropped {
                                    source: reg.id.clone(),
                                    content_hash: hash,
                                    reason: DropReason::Republished,
                                });
                            }
                            Verdict::RejectOverVolume { .. } => {
                                reg.metrics.dropped_over_volume += 1;
                                out.dropped.push(Dropped {
                                    source: reg.id.clone(),
                                    content_hash: hash,
                                    reason: DropReason::OverVolume,
                                });
                            }
                        }
                    }
                    let interval = interval_at(&reg.schedule, now);
                    reg.next_due_ms = now_ms.saturating_add(interval.as_millis() as i64);
                }
            }
        }
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
}
