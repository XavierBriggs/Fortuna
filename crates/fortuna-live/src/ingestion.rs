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
use fortuna_sources::{Alert, Dropped, IngestionScheduler, IngestionTelemetry};

/// The shared, published telemetry snapshot — ONE writer (the ingestion loop),
/// many readers (the metrics renderer + the ROTA handlers). Mirrors the daemon's
/// existing `Arc<RwLock<DashboardSnapshot>>` pattern (observability-contract §2).
pub type IngestionTelemetryHandle = std::sync::Arc<tokio::sync::RwLock<IngestionTelemetry>>;

/// A fresh, empty published handle (the pre-first-tick state readers see).
pub fn new_telemetry_handle() -> IngestionTelemetryHandle {
    std::sync::Arc::new(tokio::sync::RwLock::new(IngestionTelemetry::default()))
}

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
    /// Running funnel totals this core owns (the stages it performs): items that
    /// became envelopes, and items dropped by the authoritative dedup.
    funnel_normalized: u64,
    funnel_deduped: u64,
}

impl IngestionCore {
    pub fn new(scheduler: IngestionScheduler, registry: SourceRegistry) -> IngestionCore {
        IngestionCore {
            scheduler,
            registry,
            dedup: DedupIndex::default(),
            funnel_normalized: 0,
            funnel_deduped: 0,
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
        self.funnel_normalized += result.envelopes.len() as u64;
        self.funnel_deduped += result.duplicates as u64;
        result
    }

    /// The live telemetry snapshot: the scheduler's OBS-1 surface (per-source
    /// health/counters + the validate-stage funnel + the recent feed) plus the
    /// loop-side funnel stages THIS core performs — `normalized` (items that
    /// became `SignalEnvelope`s) and `deduped` (dropped by the authoritative
    /// dedup). `persisted` / `persist_failures` are added by
    /// [`IngestionWiring::telemetry`] (only it sees the store).
    pub fn telemetry(&self, now: UtcTimestamp) -> IngestionTelemetry {
        let mut t = self.scheduler.telemetry(now);
        t.funnel.normalized = self.funnel_normalized;
        t.funnel.deduped = self.funnel_deduped;
        t
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
    /// Running funnel totals this layer owns (the persistence stage).
    persisted_total: u64,
    persist_failures_total: u64,
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
            persisted_total: 0,
            persist_failures_total: 0,
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
        self.persisted_total += stats.persisted as u64;
        self.persist_failures_total += stats.persist_failures as u64;
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

    /// The full live telemetry snapshot (OBS-2): the core's scheduler + normalize
    /// funnel plus this layer's persistence stages. This is the single read
    /// surface ROTA / the metrics renderer project from; [`run_ingestion_loop`]
    /// PUBLISHES it behind the `Arc<RwLock<IngestionTelemetry>>` once per pass
    /// (contract §2 "one writer, many readers").
    pub fn telemetry(&self, now: UtcTimestamp) -> IngestionTelemetry {
        let mut t = self.core.telemetry(now);
        t.funnel.persisted = self.persisted_total;
        t.funnel.persist_failures = self.persist_failures_total;
        t
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
    // Library-boundary: the BINARY owns env access — the lib never reads env itself.
    // The daemon passes a resolver (`|name| std::env::var(name).ok()`); tests pass a
    // fake map. (audit Major: keep real-world reads at the main.rs edge.)
    secret_resolver: impl Fn(&str) -> Option<String>,
) -> Result<IngestionWiring, IngestionBuildError> {
    use fortuna_cognition::signals::{SourceEntry, TrustTier};

    let sources_cfg = fortuna_sources::SourcesConfig::from_toml_str(config_text)
        .map_err(|e| IngestionBuildError::Config(e.to_string()))?;

    let rows = fortuna_ledger::SourceRegistryRepo::new(pool.clone())
        .load_all()
        .await
        .map_err(|e| IngestionBuildError::Registry(e.to_string()))?;

    // Build BOTH the tier map (for the factory) and the cognition SourceRegistry
    // (for the authoritative normalize_and_dedup) from the same rows. The domain
    // map carries each source's registry-admitted domain tags into the factory
    // (surfaced in telemetry) — the registry is the source of truth, not config.
    let mut registry = fortuna_cognition::signals::SourceRegistry::new();
    let mut tiers = std::collections::BTreeMap::new();
    let mut domains: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for r in rows {
        let tier = r.trust_tier.clamp(0, 10) as u8;
        tiers.insert(r.source_id.clone(), tier);
        domains.insert(r.source_id.clone(), r.domain_tags.clone());
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
        |id| domains.get(id).cloned().unwrap_or_default(),
        // The binary owns env access (the lib never reads env): the daemon passed
        // the resolver; the lib only FORWARDS it (a source's `auth_env` name, e.g.
        // AEOLUS_API_TOKEN, resolves to its secret in the binary, not here).
        secret_resolver,
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
    telemetry: IngestionTelemetryHandle,
) -> IngestStats {
    let mut last = IngestStats::default();
    loop {
        // Stop BEFORE a tick if already signalled (clean shutdown).
        if stop.try_recv() != Err(tokio::sync::oneshot::error::TryRecvError::Empty) {
            break;
        }
        let now = clock.now();
        last = wiring.tick_and_persist(now).await;
        // Publish the snapshot for the readers (OBS-2b, "one writer, many
        // readers"). The write lock is held only for the move; the projection
        // was computed against the same `now` as the tick.
        *telemetry.write().await = wiring.telemetry(now);
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

    // --- OBS-2: the loop-side funnel stages (normalized / deduped) ----------

    /// A source that yields a scripted batch per fetch (one per tick).
    struct ScriptedQueue {
        id: String,
        q: Mutex<std::collections::VecDeque<Vec<RawSignal>>>,
    }
    #[async_trait]
    impl Source for ScriptedQueue {
        fn id(&self) -> &str {
            &self.id
        }
        async fn fetch(&mut self) -> Result<Vec<RawSignal>, SignalError> {
            Ok(self.q.lock().unwrap().pop_front().unwrap_or_default())
        }
    }

    fn core_with_queue(batches: Vec<Vec<RawSignal>>) -> IngestionCore {
        let mut sched = IngestionScheduler::new();
        sched.register(
            "src",
            Box::new(ScriptedQueue {
                id: "src".into(),
                q: Mutex::new(batches.into()),
            }),
            SourceSchedule::steady(Duration::from_secs(60), 9, 5),
            claimed,
            StructuralConfig::default(),
        );
        IngestionCore::new(sched, registry_with("src", 9))
    }

    #[tokio::test]
    async fn core_telemetry_reflects_scheduler_funnel_and_normalized() {
        let mut core = core_with(vec![raw("a", None), raw("b", None)]);
        core.tick(ts(1_000_000)).await;
        let t = core.telemetry(ts(1_000_000));
        // Scheduler-side validate funnel.
        assert_eq!(t.funnel.fetched, 2);
        assert_eq!(t.funnel.validated_accepted, 2);
        assert_eq!(t.funnel.validated_dropped, 0);
        // Loop-side stages this core performs.
        assert_eq!(t.funnel.normalized, 2);
        assert_eq!(t.funnel.deduped, 0);
        // The persistence stages are the wiring layer's — 0 at the core.
        assert_eq!(t.funnel.persisted, 0);
        assert_eq!(t.funnel.persist_failures, 0);
    }

    #[tokio::test]
    async fn core_telemetry_normalized_accumulates_across_ticks() {
        // 2 distinct items tick 1, 1 more tick 2 (advance now so the 60s source
        // is due again). All distinct hashes -> no dedup; normalized = 3.
        let mut core = core_with_queue(vec![
            vec![raw("a", None), raw("b", None)],
            vec![raw("c", None)],
        ]);
        core.tick(ts(0)).await;
        core.tick(ts(60_000)).await;
        let t = core.telemetry(ts(60_000));
        assert_eq!(t.funnel.normalized, 3);
        assert_eq!(t.funnel.fetched, 3);
        assert_eq!(t.funnel.deduped, 0);
    }

    #[tokio::test]
    async fn core_telemetry_a_cross_tick_exact_duplicate_is_caught_by_the_validator() {
        // The SAME payload on two ticks. The Layer-1 validator's recent-hash
        // memory PERSISTS across ticks (republication spans polls), so the 2nd
        // identical item is RejectRepublished at the validator — it never reaches
        // normalize. So it shows in `validated_dropped`, NOT `deduped`; only one
        // envelope is ever produced. (`deduped` counts duplicates that slip past
        // the validator's bounded window and the authoritative dedup catches.)
        let mut core = core_with_queue(vec![vec![raw("dup", None)], vec![raw("dup", None)]]);
        core.tick(ts(0)).await;
        core.tick(ts(60_000)).await;
        let t = core.telemetry(ts(60_000));
        assert_eq!(
            t.funnel.normalized, 1,
            "the duplicate never became a 2nd envelope"
        );
        assert_eq!(
            t.funnel.deduped, 0,
            "the validator caught it before the dedup stage"
        );
        assert_eq!(
            t.funnel.validated_dropped, 1,
            "counted as a republished drop"
        );
    }

    // --- OBS-2b: the published telemetry handle (the read surface for ROTA) ---

    #[tokio::test]
    async fn telemetry_handle_starts_empty_then_reflects_a_published_snapshot() {
        // The pre-first-tick state a reader sees: empty, never-generated.
        let handle = new_telemetry_handle();
        {
            let snap = handle.read().await;
            assert!(snap.generated_at.is_empty(), "not yet generated");
            assert!(snap.sources.is_empty());
            assert_eq!(snap.funnel.normalized, 0);
        }
        // Simulate the loop's per-tick publish: write a real telemetry snapshot
        // (the loop publishes `wiring.telemetry(now)`; the core projection is the
        // representative, DB-free part) and read it back through the handle.
        let mut core = core_with(vec![raw("a", None), raw("b", None)]);
        core.tick(ts(1_000)).await;
        *handle.write().await = core.telemetry(ts(1_000));
        let snap = handle.read().await;
        assert!(!snap.generated_at.is_empty());
        assert_eq!(snap.funnel.normalized, 2);
        assert_eq!(snap.sources.len(), 1);
    }
}
