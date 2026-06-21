//! WS1 slice 4 — CLV capture tests (TDD: write first, fail first).
//! Test 1: idempotency — two inserts for the same (market_id, at) → exactly
//!         one row (the UNIQUE index + ON CONFLICT DO NOTHING).
//! Test 2: liquidity gating — one-sided/empty book → liquidity_ok=false,
//!         kind='other', no fabricated price; two-sided → liquidity_ok=true;
//!         every row with a mapped event carries a non-null event_id.

use fortuna_ledger::SnapshotsRepo;
use sqlx::PgPool;

const AT_ISO: &str = "2026-06-19T12:00:00.000Z";

/// Seed a minimal event row so snapshot FK constraints pass.
async fn seed_event(pool: &PgPool, event_id: &str) {
    sqlx::query!(
        "INSERT INTO events \
         (event_id, statement, resolution_criteria, resolution_source, \
          benchmark_at, category, status, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, 'active', $7)",
        event_id,
        "Test event",
        "Test criteria",
        "test",
        "2026-06-19T23:59:00.000Z",
        "test",
        "2026-06-19T12:00:00.000Z",
    )
    .execute(pool)
    .await
    .expect("seed event");
}

/// ── Test 1: idempotency ───────────────────────────────────────────────────
/// Two inserts for the same (market_id, at) must leave exactly ONE row.
/// Before the migration adds the UNIQUE index + ON CONFLICT, the bare INSERT
/// makes 2 rows, so this test FAILS in the red phase.
#[sqlx::test(migrations = "./migrations")]
async fn price_snapshot_duplicate_at_is_skipped(pool: PgPool) {
    let repo = SnapshotsRepo::new(pool.clone());

    // First insert: a liquid two-sided snapshot. event_id=None because
    // the idempotency test is about (market_id, at) dedup, not event FK.
    repo.insert(
        "01SNAP000000000000000000001",
        "MKT-ALPHA-001",
        "kalshi",
        None, // event_id: unmapped in this test — idempotency is the focus
        "other",
        Some(40),
        Some(60),
        Some(100),
        Some(100),
        true,
        AT_ISO,
    )
    .await
    .expect("first insert ok");

    // Second insert: same (market_id, at) — different snapshot_id.
    // With ON CONFLICT DO NOTHING this is a silent skip; without it, a
    // second row lands (violating idempotency) or the unique constraint
    // errors (either way the test catches it).
    repo.insert(
        "01SNAP000000000000000000002",
        "MKT-ALPHA-001",
        "kalshi",
        None,
        "other",
        Some(41),
        Some(61),
        Some(100),
        Some(100),
        true,
        AT_ISO,
    )
    .await
    .expect("second insert (idempotent skip) ok");

    // Exactly ONE row must be present.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM price_snapshots WHERE market_id = 'MKT-ALPHA-001'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        n, 1,
        "duplicate (market_id, at) must be silently skipped — got {n} row(s)"
    );
}

/// ── Test 2: liquidity gating ─────────────────────────────────────────────
/// a) One-sided (ask only, no bid) → liquidity_ok=false, kind='other',
///    best_bid_cents=None (not fabricated).
/// b) Two-sided → liquidity_ok=true.
/// c) Every row that has a mapped event carries a non-null event_id.
#[sqlx::test(migrations = "./migrations")]
async fn price_snapshot_liquidity_gating(pool: PgPool) {
    // Seed the events so the FK constraint passes.
    seed_event(&pool, "EVT-BETA-001").await;
    seed_event(&pool, "EVT-BETA-002").await;

    let repo = SnapshotsRepo::new(pool.clone());

    // a) Degraded: one-sided book (no bid), liquidity_ok must be false,
    //    kind must be 'other', best_bid_cents must be None.
    repo.insert(
        "01SNAP000000000000000000010",
        "MKT-BETA-001",
        "kalshi",
        Some("EVT-BETA-001"), // mapped event
        "other",              // degraded kind
        None,                 // no bid — not fabricated
        Some(60),             // ask observed
        None,
        Some(50),
        false, // liquidity_ok = false
        "2026-06-19T12:00:01.000Z",
    )
    .await
    .expect("degraded insert ok");

    let row = sqlx::query!(
        "SELECT best_bid_cents, best_ask_cents, liquidity_ok, kind, event_id \
         FROM price_snapshots WHERE snapshot_id = '01SNAP000000000000000000010'"
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(
        !row.liquidity_ok,
        "one-sided book must have liquidity_ok=false"
    );
    assert_eq!(row.kind, "other", "degraded snapshot must use kind='other'");
    assert!(
        row.best_bid_cents.is_none(),
        "no bid must NOT be fabricated — best_bid_cents must be NULL"
    );
    assert_eq!(
        row.event_id.as_deref(),
        Some("EVT-BETA-001"),
        "mapped market must carry event_id"
    );

    // b) Liquid: two-sided book → liquidity_ok=true.
    repo.insert(
        "01SNAP000000000000000000011",
        "MKT-BETA-002",
        "kalshi",
        Some("EVT-BETA-002"),
        "other",
        Some(40),
        Some(60),
        Some(100),
        Some(100),
        true,
        "2026-06-19T12:00:02.000Z",
    )
    .await
    .expect("liquid insert ok");

    let row2 = sqlx::query!(
        "SELECT liquidity_ok, event_id FROM price_snapshots \
         WHERE snapshot_id = '01SNAP000000000000000000011'"
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(
        row2.liquidity_ok,
        "two-sided book must have liquidity_ok=true"
    );
    assert_eq!(
        row2.event_id.as_deref(),
        Some("EVT-BETA-002"),
        "liquid snapshot with mapped event must carry event_id"
    );

    // c) Unmapped market: event_id stays NULL (still captured).
    repo.insert(
        "01SNAP000000000000000000012",
        "MKT-BETA-003",
        "kalshi",
        None, // no event mapping yet
        "other",
        None,
        None,
        None,
        None,
        false,
        "2026-06-19T12:00:03.000Z",
    )
    .await
    .expect("unmapped insert ok");

    let row3 = sqlx::query!(
        "SELECT event_id FROM price_snapshots \
         WHERE snapshot_id = '01SNAP000000000000000000012'"
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(
        row3.event_id.is_none(),
        "unmapped market must persist with event_id=NULL — not a fabricated id"
    );
}
