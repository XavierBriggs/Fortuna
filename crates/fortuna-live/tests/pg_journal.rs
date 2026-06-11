//! T4.1 hard requirement 2 (kickoff): Postgres wiring through the DAEMON
//! path — the same SimRunner composition the daemon boots must journal
//! intents into Postgres (PgIntentJournal), and the journal must feed
//! recovery. Written from the kickoff text BEFORE the journal-generic
//! runner existed; runs on the throwaway sqlx test database (NEVER the
//! operator db — .cargo/config.toml [env] routes to fortuna_dev's server).

use fortuna_core::clock::{Clock, SimClock};
use fortuna_exec::{ExecPolicy, IntentJournal, OrderManager};
use fortuna_ledger::PgIntentJournal;
use fortuna_runner::{MemoryAuditSink, SimRunner};
use sqlx::PgPool;
use std::sync::Arc;

mod common;
use common::{runner_config, set_arb_books, strategy, t0};

/// The daemon composition's journal is Postgres: a tick that submits
/// orders must leave the intent trail in the DATABASE (journal-before-
/// network is the exec contract; here we prove the daemon path persists
/// it durably), and a fresh journal handle must recover from it.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn daemon_composition_journals_intents_in_postgres(pool: PgPool) {
    let journal_clock: Arc<dyn Clock> = Arc::new(SimClock::new(t0()));
    let journal = PgIntentJournal::new(pool.clone(), "sim", journal_clock.clone());

    let mut r = SimRunner::new_with_journal(
        runner_config(42),
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
        journal,
    )
    .await
    .unwrap();
    set_arb_books(&r);

    let report = r.tick().await.unwrap();
    assert_eq!(report.orders_submitted, 3, "three legs submitted");

    // The trail is IN POSTGRES: three intents, each with at least
    // Created + SubmitAttempted (journal-before-network), plus outcomes.
    let distinct: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT intent_id) FROM intent_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(distinct, 3, "one journal lineage per leg");
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM intent_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        events >= 6,
        "each leg journals Created before SubmitAttempted (got {events})"
    );

    // Crash-recovery path: a FRESH journal handle on the same database
    // sees the full trail and the order manager recovers from it.
    let fresh = PgIntentJournal::new(pool.clone(), "sim", journal_clock.clone());
    let rows = fresh.load_all().await.unwrap();
    assert!(rows.len() >= 6, "recovery fold input present");
    let recovered = OrderManager::recover(fresh, journal_clock, ExecPolicy::default()).await;
    assert!(
        recovered.is_ok(),
        "recovery from the Pg journal must fold cleanly"
    );
}

/// The default-journal constructor still composes identically (the
/// widening must not change existing behavior): same seed, same books,
/// same submission count, no Postgres anywhere.
#[test]
fn memory_journal_default_unchanged() {
    let mut r = SimRunner::new(
        runner_config(42),
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_arb_books(&r);
    let report = futures::executor::block_on(r.tick()).unwrap();
    assert_eq!(report.orders_submitted, 3);
}
