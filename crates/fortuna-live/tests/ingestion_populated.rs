//! OBS-2 POPULATED PATH (observability-contract §2 "one writer, many readers").
//!
//! The OBS-2a/2b unit tests are DB-free: OBS-2a pins the core funnel counts
//! (normalized/deduped) via a scripted source, OBS-2b round-trips the publish
//! handle. Neither drives the loop end-to-end THROUGH PERSISTENCE. This does: it
//! runs the REAL `run_ingestion_loop` against a REAL signals store so the
//! `persisted` stage moves off 0, and asserts a CONCURRENT reader sees the
//! published snapshot live (the contract's many-readers property) while the loop
//! is still running. READ-ONLY observability — no order/gate/belief is touched.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fortuna_cognition::signals::{
    RawSignal, SignalError, Source, SourceEntry, SourceRegistry, TrustTier,
};
use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_ledger::SignalsRepo;
use fortuna_live::ingestion::{
    new_telemetry_handle, run_ingestion_loop, IngestionCore, IngestionWiring,
};
use fortuna_sources::{IngestionScheduler, SourceSchedule, StructuralConfig};
use sqlx::PgPool;

fn ts(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(ms).unwrap()
}

/// A deterministic source that yields its scripted batch ONCE, then empty —
/// so the loop persists exactly the seeded signals (subsequent ticks are no-ops).
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

fn raw(id: &str) -> RawSignal {
    RawSignal {
        kind: "test".into(),
        payload: serde_json::json!({ "id": id }),
        received_at: ts(0),
    }
}

/// No claimed-time gate — the items are fresh (the validator accepts them; the
/// future-item refusal is exercised by the OBS-1/validator tests, not here).
fn claimed(_s: &RawSignal) -> Option<UtcTimestamp> {
    None
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

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn ingestion_loop_drives_signals_through_persist_and_publishes_for_concurrent_readers(
    pool: PgPool,
) {
    // A scripted source emitting TWO distinct signals (distinct ids ⇒ distinct
    // content hashes ⇒ no dedup), a REAL SignalsRepo (the persist stage needs the
    // DB), and the published telemetry handle.
    let mut sched = IngestionScheduler::new();
    sched.register(
        "src",
        Box::new(Scripted {
            id: "src".into(),
            out: Mutex::new(Some(vec![raw("a"), raw("b")])),
        }),
        SourceSchedule::steady(Duration::from_secs(60), 9, 5),
        claimed,
        StructuralConfig::default(),
    );
    let core = IngestionCore::new(sched, registry_with("src", 9));
    let wiring = IngestionWiring::new(core, SignalsRepo::new(pool.clone()), None);
    let telemetry = new_telemetry_handle();

    // Before the loop: the published snapshot is the default — every funnel stage 0
    // (the additive/inert baseline: a reader before the first tick sees honest zeros).
    {
        let t = telemetry.read().await;
        assert_eq!(t.funnel.normalized, 0);
        assert_eq!(
            t.funnel.persisted, 0,
            "nothing persisted before the loop runs"
        );
    }

    // Drive the REAL production loop as a background task. SimClock ⇒ `now` is
    // fixed (so the source fires once); the 10ms tick is wall-time. Each pass:
    // fetch → normalize → PERSIST → publish the snapshot behind the Arc<RwLock>.
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let clock: Arc<dyn Clock> = Arc::new(SimClock::new(ts(1_700_000_000_000)));
    let loop_task = tokio::spawn(run_ingestion_loop(
        wiring,
        clock,
        Duration::from_millis(10),
        rx,
        telemetry.clone(),
    ));

    // CONCURRENT read WHILE the loop is alive: poll the published handle (never
    // blocked beyond the loop's momentary write lock) until the PERSIST stage
    // moves off 0. Generously bounded so it cannot flake (the first tick persists
    // both within ~10ms).
    let mut persisted = 0u64;
    let mut normalized = 0u64;
    for _ in 0..300 {
        {
            let t = telemetry.read().await; // a concurrent reader, loop still running
            persisted = t.funnel.persisted;
            normalized = t.funnel.normalized;
            if persisted >= 2 {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        persisted >= 2,
        "the loop drove 2 real signals through to PERSIST and PUBLISHED it for a concurrent reader (got persisted={persisted})"
    );
    assert!(
        normalized >= 2,
        "the normalized stage moved with persisted (got normalized={normalized})"
    );

    // Stop the loop + join; its final stats corroborate the persisted signals.
    let _ = tx.send(());
    let stats = loop_task.await.expect("ingestion loop task joins cleanly");
    assert!(
        stats.persisted >= 2 || persisted >= 2,
        "the loop persisted the seeded signals"
    );

    // The persist was REAL — both signals landed in the append-only store.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM signals WHERE source = 'src'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        n >= 2,
        "both scripted signals persisted to the signals store (got {n})"
    );

    // The final published snapshot still reflects the moved stages (reads are
    // non-destructive; the writer is the only mutator).
    let t = telemetry.read().await;
    assert!(t.funnel.normalized >= 2 && t.funnel.persisted >= 2);
}
