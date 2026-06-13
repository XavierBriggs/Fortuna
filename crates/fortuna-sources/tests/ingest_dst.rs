//! Ingestion-scheduler Deterministic Simulation Tests (D9). The five named
//! scenarios from the build plan, each ENUMERATED (not randomized — they are
//! specific failure modes) and driven by SimClock + a scripted, fault-injecting
//! source through the PUBLIC `IngestionScheduler` API. Run by the battery:
//!   cargo test -p fortuna-sources --test ingest_dst -- --nocapture
//!
//! The load-bearing property under all of them: the StructuralValidator is on
//! the live path (refuse-and-quarantine), and one source's faults never corrupt
//! the fleet or the deterministic outcome.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use fortuna_cognition::signals::{RawSignal, SignalError, Source};
use fortuna_core::clock::UtcTimestamp;
use fortuna_sources::{
    Alert, DropReason, Health, IngestionScheduler, SourceSchedule, StructuralConfig,
};

fn ts(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(ms).unwrap()
}

fn sig(id: &str) -> RawSignal {
    RawSignal {
        kind: "test".into(),
        payload: serde_json::json!({ "id": id }),
        received_at: ts(0),
    }
}

fn no_claimed(_: &RawSignal) -> Option<UtcTimestamp> {
    None
}

struct ScriptedSource {
    id: String,
    script: Mutex<VecDeque<Result<Vec<RawSignal>, SignalError>>>,
}

impl ScriptedSource {
    fn boxed(id: &str, script: Vec<Result<Vec<RawSignal>, SignalError>>) -> Box<ScriptedSource> {
        Box::new(ScriptedSource {
            id: id.to_string(),
            script: Mutex::new(script.into()),
        })
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

fn fetch_err(reason: &str) -> Result<Vec<RawSignal>, SignalError> {
    Err(SignalError::Fetch {
        source_id: "scripted".into(),
        reason: reason.into(),
    })
}

fn fast_sched(quarantine_after: u32) -> SourceSchedule {
    SourceSchedule {
        base_interval: Duration::from_secs(1),
        event_windows: Vec::new(),
        boosted_interval: Duration::from_secs(1),
        quarantine_after,
        backoff_base: Duration::from_secs(1),
        backoff_cap: Duration::from_secs(60),
        trust_tier: 9,
        trigger_floor: 5,
        domain_tags: Vec::new(),
    }
}

/// Scenario 1 — a fetch TIMEOUT (surfaced as a SignalError) during operation
/// degrades the source and is retried, while a healthy peer keeps ingesting.
#[tokio::test]
async fn scenario_timeout_does_not_block_the_fleet() {
    let mut s = IngestionScheduler::new();
    s.register(
        "weather",
        ScriptedSource::boxed("weather", vec![fetch_err("request timed out")]),
        fast_sched(5),
        no_claimed,
        StructuralConfig::default(),
    );
    s.register(
        "macro",
        ScriptedSource::boxed("macro", vec![Ok(vec![sig("cpi")])]),
        fast_sched(5),
        no_claimed,
        StructuralConfig::default(),
    );
    let out = s.tick(ts(0)).await;
    assert_eq!(out.accepted.len(), 1, "the healthy peer still ingests");
    assert!(matches!(
        s.health("weather").unwrap(),
        Health::Degraded {
            consecutive_failures: 1
        }
    ));
    println!("[ingest-dst] timeout: degraded weather, macro ingested ✓");
}

/// Scenario 2 — a 429 STORM: repeated rate-limit failures escalate backoff and
/// eventually quarantine the source LOUDLY.
#[tokio::test]
async fn scenario_429_storm_escalates_to_quarantine() {
    let mut s = IngestionScheduler::new();
    s.register(
        "noisy",
        ScriptedSource::boxed(
            "noisy",
            vec![fetch_err("429"), fetch_err("429"), fetch_err("429")],
        ),
        fast_sched(3),
        no_claimed,
        StructuralConfig::default(),
    );
    // Tick past each backoff; the 3rd failure quarantines.
    s.tick(ts(0)).await;
    s.tick(ts(2_000)).await;
    let out = s.tick(ts(6_000)).await;
    assert_eq!(s.health("noisy").unwrap(), &Health::Quarantined);
    assert!(out
        .alerts
        .iter()
        .any(|a| matches!(a, Alert::Quarantined { .. })));
    println!("[ingest-dst] 429 storm: quarantined + loud alert ✓");
}

/// Scenario 3 — CRASH IN WINDOW + recovery: the scheduler is dropped mid-run
/// and rebuilt from config; it resumes deterministically and keeps ingesting
/// (the validator's in-memory republication buffer resets on crash — bounded
/// fast-path flag; the ledger UNIQUE is the authoritative backstop).
#[tokio::test]
async fn scenario_crash_in_window_then_rebuild_resumes() {
    let build = || {
        let mut s = IngestionScheduler::new();
        s.register(
            "src",
            ScriptedSource::boxed("src", vec![Ok(vec![sig("a")]), Ok(vec![sig("b")])]),
            fast_sched(5),
            no_claimed,
            StructuralConfig::default(),
        );
        s
    };
    let mut s = build();
    assert_eq!(s.tick(ts(0)).await.accepted.len(), 1); // ingested "a"
    drop(s); // crash

    // Rebuild from the same config and resume; no panic, clean resume.
    let mut s2 = build();
    let out = s2.tick(ts(10_000)).await;
    assert_eq!(out.accepted.len(), 1, "rebuilt scheduler resumes ingesting");
    println!("[ingest-dst] crash+rebuild: resumed cleanly ✓");
}

/// Scenario 4 — BURST COALESCING: a source emits far more than its per-tick
/// volume envelope in one fetch; the excess is refused (OverVolume), bounding
/// storage. Containment, not crash.
#[tokio::test]
async fn scenario_burst_is_capped_by_the_volume_envelope() {
    let burst: Vec<RawSignal> = (0..100).map(|i| sig(&format!("n{i}"))).collect();
    let cfg = StructuralConfig {
        volume_envelope: 10,
        ..Default::default()
    };
    let mut s = IngestionScheduler::new();
    s.register(
        "firehose",
        ScriptedSource::boxed("firehose", vec![Ok(burst)]),
        fast_sched(5),
        no_claimed,
        cfg,
    );
    let out = s.tick(ts(0)).await;
    assert_eq!(out.accepted.len(), 10, "capped at the envelope");
    let over = out
        .dropped
        .iter()
        .filter(|d| d.reason == DropReason::OverVolume)
        .count();
    assert_eq!(over, 90, "the rest refused as over-volume");
    assert_eq!(s.metrics("firehose").unwrap().dropped_over_volume, 90);
    println!("[ingest-dst] burst: 10 accepted / 90 refused (over-volume) ✓");
}

/// Scenario 5 — QUARANTINE ESCALATION + operator RE-ARM: a source quarantines,
/// stops being polled, then an operator rearm restores it and it ingests again.
#[tokio::test]
async fn scenario_quarantine_then_operator_rearm() {
    let mut s = IngestionScheduler::new();
    s.register(
        "src",
        ScriptedSource::boxed(
            "src",
            vec![fetch_err("down"), fetch_err("down"), Ok(vec![sig("back")])],
        ),
        fast_sched(2),
        no_claimed,
        StructuralConfig::default(),
    );
    s.tick(ts(0)).await;
    s.tick(ts(2_000)).await;
    assert_eq!(s.health("src").unwrap(), &Health::Quarantined);
    // Quarantined: not polled.
    assert!(s.tick(ts(5_000)).await.accepted.is_empty());
    // Operator re-arm.
    assert!(s.rearm("src"));
    let out = s.tick(ts(10_000)).await;
    assert_eq!(out.accepted.len(), 1, "re-armed source ingests again");
    assert_eq!(s.health("src").unwrap(), &Health::Healthy);
    println!("[ingest-dst] quarantine -> rearm -> ingesting ✓");
}
