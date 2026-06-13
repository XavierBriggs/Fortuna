//! D10 drive() seam: run the news-aggregation ingestion scheduler inside the
//! daemon, behind a default-off `[ingestion]` flag.
//!
//! Split for testability:
//! - [`IngestionCore`] ticks the scheduler (the Layer-1 StructuralValidator runs
//!   on the live path — the hard gate) and normalizes accepted signals into
//!   [`SignalEnvelope`]s through the authoritative `normalize_and_dedup`
//!   (registry re-check + dedup). Pure of IO — unit-tested with a scripted
//!   source, no database.
//! - [`IngestionWiring`] wraps the core with the `SignalsRepo` (append-only
//!   signals store) and the Slack router; `tick_and_persist` is what `drive()`
//!   calls each segment. Thin DB/Slack glue, covered at integration.
//!
//! Refused/dropped items (future-dated, republished, over-volume, unregistered)
//! are counted and never persisted. Source quarantine raises an Ops alert.

use std::collections::BTreeMap;

use fortuna_cognition::signals::{
    normalize_and_dedup, DedupIndex, IngestOutcome, SignalEnvelope, SourceRegistry,
};
use fortuna_core::clock::UtcTimestamp;
use fortuna_ledger::SignalsRepo;
use fortuna_ops::{MessageKind, SlackRouter};
use fortuna_sources::{Alert, Dropped, IngestionScheduler};

/// What one tick produced (pre-persistence).
#[derive(Debug, Default)]
pub struct IngestResult {
    pub envelopes: Vec<SignalEnvelope>,
    pub alerts: Vec<Alert>,
    pub dropped: Vec<Dropped>,
    pub duplicates: usize,
    /// Sources the normalizer refused (unregistered/disabled) — never persisted.
    pub refused_sources: Vec<String>,
}

/// The testable ingest core: scheduler + the authoritative registry/dedup.
pub struct IngestionCore {
    scheduler: IngestionScheduler,
    registry: SourceRegistry,
    dedup: DedupIndex,
}

impl IngestionCore {
    pub fn new(scheduler: IngestionScheduler, registry: SourceRegistry) -> IngestionCore {
        IngestionCore {
            scheduler,
            registry,
            dedup: DedupIndex::default(),
        }
    }

    /// Tick the scheduler and normalize accepted signals into envelopes. The
    /// validator already refused future/republished/over-volume items; here the
    /// normalizer applies the FAIL-CLOSED registry allowlist + authoritative
    /// dedup. Per-source grouping preserves `normalize_and_dedup`'s contract.
    pub async fn tick(&mut self, now: UtcTimestamp) -> IngestResult {
        let outcome = self.scheduler.tick(now).await;
        let mut result = IngestResult {
            alerts: outcome.alerts,
            dropped: outcome.dropped,
            ..Default::default()
        };
        let mut by_source: BTreeMap<String, Vec<fortuna_cognition::signals::RawSignal>> =
            BTreeMap::new();
        for a in outcome.accepted {
            by_source.entry(a.source).or_default().push(a.signal);
        }
        let now_ms = now.epoch_millis();
        for (source, raws) in by_source {
            let made = normalize_and_dedup(&source, raws, &self.registry, &mut self.dedup, |n| {
                format!("{source}-{now_ms}-{n}")
            });
            match made {
                IngestOutcome::Accepted {
                    envelopes,
                    duplicates,
                } => {
                    result.duplicates += duplicates;
                    result.envelopes.extend(envelopes);
                }
                IngestOutcome::RefusedUnregistered | IngestOutcome::RefusedDisabled => {
                    result.refused_sources.push(source);
                }
            }
        }
        result
    }

    /// Operator re-enable of a quarantined source (CLI/ops surface in D10's
    /// wiring; exposed here so the daemon can route a rearm command).
    pub fn rearm(&mut self, source_id: &str) -> bool {
        self.scheduler.rearm(source_id)
    }

    pub fn source_ids(&self) -> Vec<&str> {
        self.scheduler.source_ids()
    }
}

/// Per-tick persistence summary for telemetry/logging.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IngestStats {
    pub persisted: usize,
    pub duplicates: usize,
    pub dropped: usize,
    pub alerts: usize,
    pub persist_failures: usize,
}

/// The production wiring: core + the signals store + Slack. `drive()` holds an
/// `Option<IngestionWiring>` and calls `tick_and_persist` each segment.
pub struct IngestionWiring {
    core: IngestionCore,
    signals: SignalsRepo,
    slack: Option<SlackRouter>,
}

impl IngestionWiring {
    pub fn new(
        core: IngestionCore,
        signals: SignalsRepo,
        slack: Option<SlackRouter>,
    ) -> IngestionWiring {
        IngestionWiring {
            core,
            signals,
            slack,
        }
    }

    /// Tick, persist accepted envelopes to the append-only signals store, and
    /// raise an Ops alert per quarantine. A persist failure is COUNTED, not
    /// fatal — ingestion is opt-in and off the money path, so one bad insert
    /// must not take down the daemon (it surfaces in `persist_failures`).
    pub async fn tick_and_persist(&mut self, now: UtcTimestamp) -> IngestStats {
        let result = self.core.tick(now).await;
        let mut stats = IngestStats {
            duplicates: result.duplicates,
            dropped: result.dropped.len(),
            alerts: result.alerts.len(),
            ..Default::default()
        };
        for env in &result.envelopes {
            match self
                .signals
                .insert(
                    &env.signal_id,
                    &env.source,
                    &env.kind,
                    &env.received_at.to_iso8601(),
                    &env.content_hash,
                    &env.payload,
                )
                .await
            {
                Ok(()) => stats.persisted += 1,
                Err(_) => stats.persist_failures += 1,
            }
        }
        if let Some(slack) = &self.slack {
            for alert in &result.alerts {
                if let Alert::Quarantined { source, reason } = alert {
                    let _ = slack
                        .send(
                            MessageKind::Ops,
                            &format!("ingestion source `{source}` quarantined: {reason}"),
                        )
                        .await;
                }
            }
        }
        stats
    }

    pub fn rearm(&mut self, source_id: &str) -> bool {
        self.core.rearm(source_id)
    }
}

/// Construct the production ingestion wiring from config + the live DB (the
/// main.rs composition step, behind the `[ingestion].enabled` flag). Loads
/// trust tiers from `source_registry` (the authoritative allowlist), parses the
/// `[sources.<id>]` tables, and builds the scheduler via the factory.
/// Ingestion-alert Slack routing is deferred (quarantines are counted/logged);
/// it can be added by passing a router here later.
pub async fn build_ingestion_wiring(
    config_text: &str,
    section: &crate::boot::IngestionSection,
    pool: sqlx::PgPool,
    clock: std::sync::Arc<dyn fortuna_core::clock::Clock>,
) -> Result<IngestionWiring, IngestionBuildError> {
    use fortuna_cognition::signals::{SourceEntry, TrustTier};

    let sources_cfg = fortuna_sources::SourcesConfig::from_toml_str(config_text)
        .map_err(|e| IngestionBuildError::Config(e.to_string()))?;

    let rows = fortuna_ledger::SourceRegistryRepo::new(pool.clone())
        .load_all()
        .await
        .map_err(|e| IngestionBuildError::Registry(e.to_string()))?;

    // Build BOTH the tier map (for the factory) and the cognition SourceRegistry
    // (for the authoritative normalize_and_dedup) from the same rows.
    let mut registry = fortuna_cognition::signals::SourceRegistry::new();
    let mut tiers = std::collections::BTreeMap::new();
    for r in rows {
        let tier = r.trust_tier.clamp(0, 10) as u8;
        tiers.insert(r.source_id.clone(), tier);
        if let Ok(tt) = TrustTier::new(tier) {
            registry.upsert(SourceEntry {
                source_id: r.source_id,
                trust_tier: tt,
                domain_tags: r.domain_tags,
                enabled: r.enabled,
            });
        }
    }

    let factory_cfg = fortuna_sources::FactoryConfig {
        fetch_timeout: std::time::Duration::from_secs(20),
        user_agent: section.user_agent.clone(),
        trigger_floor: section.trigger_floor,
        volume_envelope: section.volume_envelope,
    };
    let scheduler = fortuna_sources::build_scheduler(
        &sources_cfg,
        &factory_cfg,
        |id| tiers.get(id).copied(),
        clock,
    )
    .map_err(|e| IngestionBuildError::Factory(e.to_string()))?;

    let core = IngestionCore::new(scheduler, registry);
    Ok(IngestionWiring::new(core, SignalsRepo::new(pool), None))
}

/// Errors from constructing the ingestion wiring at boot.
#[derive(Debug, thiserror::Error)]
pub enum IngestionBuildError {
    #[error("[sources] config: {0}")]
    Config(String),
    #[error("source_registry load: {0}")]
    Registry(String),
    #[error("source factory: {0}")]
    Factory(String),
}

/// The independent ingestion run-loop the daemon spawns alongside `drive()`
/// (NOT inside the deterministic trading cycle — ingestion is its own IO loop).
/// Ticks on a fixed cadence (the scheduler skips not-due sources internally),
/// reading the injected `Clock`; sleeps on real time at the IO edge; exits on
/// the stop signal. Returns the last stats for the shutdown log.
pub async fn run_ingestion_loop(
    mut wiring: IngestionWiring,
    clock: std::sync::Arc<dyn fortuna_core::clock::Clock>,
    tick_interval: std::time::Duration,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> IngestStats {
    let mut last = IngestStats::default();
    loop {
        // Stop BEFORE a tick if already signalled (clean shutdown).
        if stop.try_recv() != Err(tokio::sync::oneshot::error::TryRecvError::Empty) {
            break;
        }
        last = wiring.tick_and_persist(clock.now()).await;
        tokio::select! {
            _ = tokio::time::sleep(tick_interval) => {}
            _ = &mut stop => break,
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fortuna_cognition::signals::{RawSignal, SignalError, Source, SourceEntry, TrustTier};
    use fortuna_sources::{SourceSchedule, StructuralConfig};
    use std::sync::Mutex;
    use std::time::Duration;

    fn ts(ms: i64) -> UtcTimestamp {
        UtcTimestamp::from_epoch_millis(ms).unwrap()
    }

    fn raw(id: &str, claimed_ms: Option<i64>) -> RawSignal {
        let payload = match claimed_ms {
            Some(ms) => serde_json::json!({"id": id, "claimed": ts(ms).to_iso8601()}),
            None => serde_json::json!({"id": id}),
        };
        RawSignal {
            kind: "test".into(),
            payload,
            received_at: ts(0),
        }
    }

    fn claimed(s: &RawSignal) -> Option<UtcTimestamp> {
        s.payload
            .get("claimed")
            .and_then(|v| v.as_str())
            .and_then(|t| UtcTimestamp::parse_iso8601(t).ok())
    }

    struct Scripted {
        id: String,
        out: Mutex<Option<Vec<RawSignal>>>,
    }
    #[async_trait]
    impl Source for Scripted {
        fn id(&self) -> &str {
            &self.id
        }
        async fn fetch(&mut self) -> Result<Vec<RawSignal>, SignalError> {
            Ok(self.out.lock().unwrap().take().unwrap_or_default())
        }
    }

    fn registry_with(id: &str, tier: u8) -> SourceRegistry {
        let mut r = SourceRegistry::new();
        r.upsert(SourceEntry {
            source_id: id.into(),
            trust_tier: TrustTier::new(tier).unwrap(),
            domain_tags: vec![],
            enabled: true,
        });
        r
    }

    fn core_with(signals: Vec<RawSignal>) -> IngestionCore {
        let mut sched = IngestionScheduler::new();
        sched.register(
            "src",
            Box::new(Scripted {
                id: "src".into(),
                out: Mutex::new(Some(signals)),
            }),
            SourceSchedule::steady(Duration::from_secs(60), 9, 5),
            claimed,
            StructuralConfig::default(),
        );
        IngestionCore::new(sched, registry_with("src", 9))
    }

    #[tokio::test]
    async fn tick_normalizes_accepted_into_envelopes() {
        let mut core = core_with(vec![raw("a", None), raw("b", None)]);
        let r = core.tick(ts(1_000_000)).await;
        assert_eq!(r.envelopes.len(), 2);
        assert!(r.envelopes.iter().all(|e| e.source == "src"));
        assert!(r.refused_sources.is_empty());
    }

    #[tokio::test]
    async fn validator_is_live_a_future_item_never_becomes_an_envelope() {
        // One fresh + one far-future item; the validator drops the future one
        // BEFORE it can be normalized/persisted (the hard gate, end to end).
        let mut core = core_with(vec![
            raw("fresh", None),
            raw("future", Some(1_000_000 + 100_000_000)),
        ]);
        let r = core.tick(ts(1_000_000)).await;
        assert_eq!(r.envelopes.len(), 1, "only the fresh item is normalized");
        assert_eq!(r.envelopes[0].payload["id"], "fresh");
        assert_eq!(r.dropped.len(), 1, "the future item was refused");
    }

    #[tokio::test]
    async fn an_unregistered_source_is_refused_by_the_normalizer() {
        // Scheduler has the source, but the registry does NOT — fail-closed.
        let mut sched = IngestionScheduler::new();
        sched.register(
            "ghost",
            Box::new(Scripted {
                id: "ghost".into(),
                out: Mutex::new(Some(vec![raw("a", None)])),
            }),
            SourceSchedule::steady(Duration::from_secs(60), 9, 5),
            claimed,
            StructuralConfig::default(),
        );
        let mut core = IngestionCore::new(sched, SourceRegistry::new());
        let r = core.tick(ts(0)).await;
        assert!(r.envelopes.is_empty());
        assert_eq!(r.refused_sources, vec!["ghost"]);
    }
}
