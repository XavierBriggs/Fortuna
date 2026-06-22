//! I5 (parametric coverage): every append-only store carries a mutation guard.
//!
//! `i5_audit_append_only.rs` proves the MECHANISM behaviorally on the `audit`
//! table (raw UPDATE/DELETE rejected by the trigger). This test proves
//! COVERAGE: every table that I5 requires be append-only actually carries a
//! BEFORE UPDATE OR DELETE trigger in the migrated schema. A future migration
//! that adds an append-only store but forgets the guard fails here. Adding a
//! new append-only table = add its name below AND its trigger in the migration.
//!
//! ADDITIONS-ONLY (protected crate): never weaken these assertions.

use sqlx::PgPool;
use std::collections::BTreeSet;

/// The decision / audit / score / belief tables I5 requires be append-only.
/// Sourced from crates/fortuna-ledger/migrations (every `CREATE TRIGGER ...
/// BEFORE UPDATE OR DELETE`). Keep in sync when a new append-only table lands.
const MUST_BE_APPEND_ONLY: &[&str] = &[
    "audit",
    "signals",
    "intent_events",
    "fills",
    "market_event_edges",
    "beliefs",
    "market_snapshots",
    "price_snapshots",
    "settlement_entries",
    "discrepancies",
    "discrepancy_resolutions",
    "journal",
    "lessons",
    "calibration_params",
    "reservation_events",
    "halt_events",
    "scalar_beliefs",
    "belief_scores",
    "personas",
    "domain_analyses",
    "tradability_scores",
    "event_source_evidence",
    "trade_scores",
    "bus_recordings",
    "funding_rates_historical",
    "scorecards",
    "validation_runs",
];

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn i5_all_append_only_tables_reject_mutation(pool: PgPool) {
    // Every user table carrying a non-internal trigger. An append-only guard is
    // exactly such a trigger (BEFORE UPDATE OR DELETE -> raise 'append-only').
    let triggered: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT c.relname::text \
         FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid \
         WHERE NOT t.tgisinternal",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let guarded: BTreeSet<&str> = triggered.iter().map(|s| s.as_str()).collect();

    let missing: Vec<&str> = MUST_BE_APPEND_ONLY
        .iter()
        .copied()
        .filter(|t| !guarded.contains(t))
        .collect();

    assert!(
        missing.is_empty(),
        "I5: these tables MUST be append-only but carry no mutation-guard trigger \
         in the migrated schema: {missing:?}"
    );
}
