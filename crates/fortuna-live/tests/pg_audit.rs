//! T4.1 hard requirement 2 (kickoff), audit half: the daemon path writes
//! its audit trail to POSTGRES, and an audit write failure HALTS trading
//! (I5: "no audit, no trading" — already the runner's contract; here we
//! prove it holds when the sink is the real Postgres writer). Written
//! red-first against a PgAuditSink that did not exist.

use fortuna_core::clock::{Clock, SimClock};
use fortuna_ledger::PgIntentJournal;
use fortuna_live::audit_bridge::PgAuditSink;
use fortuna_runner::SimRunner;
use sqlx::PgPool;
use std::sync::Arc;

mod common;
use common::{runner_config, set_arb_books, strategy, t0};

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn daemon_audit_rows_land_in_postgres_and_audit_death_halts(pool: PgPool) {
    let clock: Arc<dyn Clock> = Arc::new(SimClock::new(t0()));
    let journal = PgIntentJournal::new(pool.clone(), "sim", clock.clone());
    let sink = PgAuditSink::spawn(pool.clone(), clock.clone(), 7);

    let mut r = SimRunner::new_with_journal(
        runner_config(42),
        vec![strategy()],
        Box::new(sink),
        t0(),
        journal,
    )
    .await
    .unwrap();
    set_arb_books(&r);

    let report = r.tick().await.unwrap();
    assert_eq!(report.orders_submitted, 3, "the composed tick trades");
    assert!(!report.halted, "healthy audit store: no halt");

    // The trail is IN POSTGRES, written through the sync bridge in tick
    // order. gate_decision rows exist for the submitted legs.
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(total > 0, "audit rows persisted");
    let gate_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit WHERE kind = 'gate_decision'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        gate_rows >= 3,
        "every submitted leg passed the gates on the record (got {gate_rows})"
    );

    // I5 through the daemon path: kill the audit store out from under the
    // sink; the NEXT write failure must halt trading, synchronously. A
    // settlement notice is the deterministic audit trigger (a quiet tick
    // writes nothing, which is why re-setting books alone proved
    // insufficient red-first: held exposure blocks a re-entry proposal).
    sqlx::query("DROP TABLE audit CASCADE")
        .execute(&pool)
        .await
        .unwrap();
    r.venue()
        .settle_market(&common::mkt("BKT-MID"), fortuna_core::market::Side::Yes)
        .unwrap();
    let report2 = r.tick().await.unwrap();
    assert!(
        report2.halted,
        "audit write failure must halt trading in the SAME tick (I5)"
    );
    let report3 = r.tick().await.unwrap();
    assert_eq!(
        report3.orders_submitted, 0,
        "halted: no further orders after audit death"
    );
}
