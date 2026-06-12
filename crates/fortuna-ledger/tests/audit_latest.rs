//! A8 (T4.4 CLI design): `fortuna status` prints the age of the MOST
//! RECENT audit row — a stale age beside a live pidfile is the crash
//! tell. The query is kind-agnostic by design: a kind-filtered
//! approximation would read a healthy daemon writing only cognition/veto
//! rows as stale (the false-crash-tell failure mode the original GAPS
//! deferral named). Written BEFORE the implementation per DoD; orphaned
//! minor F-1 taken back post-pool-release.

use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_ledger::AuditWriter;
use sqlx::PgPool;
use std::sync::Arc;

fn t(iso: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(iso).unwrap()
}

#[sqlx::test]
async fn latest_at_returns_the_newest_row_of_any_kind(pool: PgPool) {
    let clock = Arc::new(SimClock::new(t("2026-06-12T08:00:00.000Z")));
    let audit = AuditWriter::new(pool.clone(), clock.clone(), 7);
    assert!(
        audit.latest_at().await.unwrap().is_none(),
        "empty table => None, never an error"
    );

    audit
        .append("gate_decision", None, None, serde_json::json!({}))
        .await
        .unwrap();
    clock.advance_millis(5_000).unwrap();
    // A DIFFERENT kind, later — the latest must be kind-agnostic.
    audit
        .append("cognition", Some("mind"), None, serde_json::json!({}))
        .await
        .unwrap();

    let latest = audit.latest_at().await.unwrap().expect("rows exist");
    assert_eq!(latest.at, "2026-06-12T08:00:05.000Z", "newest row wins");
    assert_eq!(latest.kind, "cognition", "kind-agnostic: {latest:?}");
}
