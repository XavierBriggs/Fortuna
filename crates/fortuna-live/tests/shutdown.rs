//! T4.1 hard requirement 8 + the operator's BINDING shutdown contract
//! (BUILD_PLAN T4.1, 2026-06-11): graceful shutdown cancels working
//! orders through the journaled path and writes the final audit row.
//! `fortuna stop` (T4.4) depends on this existing and being asserted.
//! Tested through the COMPOSITION (the daemon's Pg journal + Pg audit),
//! not through OS signals — main's SIGTERM/SIGINT handler (main.rs)
//! routes to exactly this function via the run loop's stop channel; the
//! daemon_smoke fires that same channel to assert the signal path end to
//! end. Written red-first against a shutdown() that did not exist.

use fortuna_core::clock::{Clock, SimClock};
use fortuna_core::market::Contracts;
use fortuna_ledger::PgIntentJournal;
use fortuna_live::audit_bridge::PgAuditSink;
use fortuna_runner::SimRunner;
use fortuna_venues::sim::{FaultConfig, SimVenue};
use fortuna_venues::PriceLevel;
use sqlx::PgPool;
use std::sync::Arc;

mod common;
use common::{mkt, runner_config, strategy, t0};

/// Books with ONE contract of depth at each ask: the marketable legs
/// partially fill and the REMAINDER RESTS — acked, working orders.
fn set_thin_arb_books(r: &SimRunner<SimVenue, PgIntentJournal>) {
    let lvl = |p: i64, q: i64| PriceLevel {
        price: fortuna_core::money::Cents::new(p),
        qty: Contracts::new(q),
    };
    for (m, bid, ask) in [("BKT-LO", 20, 25), ("BKT-MID", 23, 28), ("BKT-HI", 25, 30)] {
        r.venue()
            .set_book(&mkt(m), vec![lvl(bid, 80)], vec![lvl(ask, 1)])
            .unwrap();
    }
}

async fn compose(pool: &PgPool, faults: FaultConfig) -> SimRunner<SimVenue, PgIntentJournal> {
    let clock: Arc<dyn Clock> = Arc::new(SimClock::new(t0()));
    let journal = PgIntentJournal::new(pool.clone(), "sim", clock.clone());
    let sink = PgAuditSink::spawn(pool.clone(), clock, 7);
    let mut cfg = runner_config(42);
    cfg.faults = Some(faults);
    SimRunner::new_with_journal(cfg, vec![strategy()], Box::new(sink), t0(), journal)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn shutdown_cancels_acked_working_orders_and_audits(pool: PgPool) {
    let mut r = compose(&pool, FaultConfig::none(42)).await;
    set_thin_arb_books(&r);
    let report = r.tick().await.unwrap();
    assert_eq!(report.orders_submitted, 3, "three legs submitted");

    let sd = r.shutdown().await.unwrap();
    assert!(
        sd.cancelled >= 1,
        "resting remainders cancel at the venue (got {sd:?})"
    );
    assert_eq!(sd.unacked, 0, "no ack-delay fault: nothing unacked");

    // The cancels are JOURNALED (Postgres trail) and the final audit row
    // exists exactly once.
    let cancel_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM intent_events \
         WHERE event->>'kind' IN ('cancel_requested', 'cancelled')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        cancel_events >= 1,
        "cancel events journaled (got {cancel_events})"
    );
    let final_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit WHERE kind = 'daemon_shutdown'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(final_rows, 1, "exactly one final audit row");

    // Idempotent: a second shutdown finds nothing working and still
    // returns cleanly (a second audit row is fine; a panic or error is not).
    let sd2 = r.shutdown().await.unwrap();
    assert_eq!(sd2.cancelled, 0);
    assert_eq!(sd2.working, 0);
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn shutdown_cannot_lie_when_cancels_time_out(pool: PgPool) {
    // Discovered red-first while hunting an unacked vector: ack_delay
    // still hands back a venue id, and place-timeouts inside a LEG GROUP
    // are aborted by the runner's all-or-nothing logic before shutdown
    // ever sees them (working=0 — the system self-cleans that window).
    // The deterministic could-not-confirm vector is a CANCEL timeout on
    // an acked resting order: the venue says Timeout, the order may
    // still be live, and shutdown must report UNKNOWN — never cancelled.
    let faults = FaultConfig {
        cancel_timeout_not_cancelled_pm: 1_000_000,
        ..FaultConfig::none(42)
    };
    let mut r = compose(&pool, faults).await;
    set_thin_arb_books(&r);
    let report = r.tick().await.unwrap();
    assert_eq!(report.orders_submitted, 3);

    let sd = r.shutdown().await.unwrap();
    assert!(
        sd.unknown >= 1,
        "timed-out cancels are honestly UNKNOWN (got {sd:?})"
    );
    assert_eq!(sd.cancelled, 0, "nothing falsely counted cancelled: {sd:?}");
    let final_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit WHERE kind = 'daemon_shutdown'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(final_rows, 1);
}
