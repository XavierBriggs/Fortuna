//! A4 tests: per-(market,strategy) trade score from settled fills (TDD).
//!
//! Tests are written FROM the spec text BEFORE the implementation.
//! Covers:
//!  - fills_aggregate: correct aggregation of fee_cents, n_fills, maker_fills,
//!    strategy from the fills table for a market;
//!  - insert -> row has correct pnl_after_fees, fees, n_fills, maker_fills,
//!    strategy;
//!  - second insert same (market_id, strategy) -> Ok(false), still one row;
//!  - the append-only trigger refuses UPDATE (mutation-proof).

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, VenueOrderId};
use fortuna_core::money::Cents;
use fortuna_ledger::{FillsRepo, TradeScoresRepo};
use fortuna_venues::Fill;
use sqlx::PgPool;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-18T10:00:00.000Z").unwrap()
}

/// Build a minimal Fill for tests.
fn make_fill(fill_id: &str, market: &str, fee_cents: i64, is_maker: bool) -> Fill {
    Fill {
        fill_id: fill_id.to_string(),
        venue_order_id: VenueOrderId::new("v-1").unwrap(),
        client_order_id: ClientOrderId::new("co-1").unwrap(),
        market: MarketId::new(market).unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        price: Cents::new(55),
        qty: Contracts::new(1),
        fee: Cents::new(fee_cents),
        is_maker,
        at: t0(),
    }
}

// ─── fills_aggregate: correct aggregation ─────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn fills_aggregate_sums_fees_counts_fills_and_picks_strategy(pool: PgPool) {
    let fills_repo = FillsRepo::new(pool.clone());
    let repo = TradeScoresRepo::new(pool.clone());

    // 1 maker fill @ 2¢ fee, 1 taker fill @ 5¢ fee, both strategy "mech_extremes"
    fills_repo
        .insert(
            "kalshi",
            &make_fill("fill-1", "MKT-A", 2, true),
            None,
            Some("mech_extremes"),
        )
        .await
        .unwrap();
    fills_repo
        .insert(
            "kalshi",
            &make_fill("fill-2", "MKT-A", 5, false),
            None,
            Some("mech_extremes"),
        )
        .await
        .unwrap();

    let agg = repo.fills_aggregate("MKT-A").await.unwrap();
    assert_eq!(agg.fees_cents, 7, "2 + 5 = 7");
    assert_eq!(agg.n_fills, 2);
    assert_eq!(agg.maker_fills, 1);
    assert_eq!(agg.strategy.as_deref(), Some("mech_extremes"));
}

// ─── insert: correct row content ──────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn trade_score_insert_round_trip(pool: PgPool) {
    let fills_repo = FillsRepo::new(pool.clone());
    let repo = TradeScoresRepo::new(pool.clone());

    fills_repo
        .insert(
            "kalshi",
            &make_fill("fill-1", "MKT-B", 2, true),
            None,
            Some("mech_extremes"),
        )
        .await
        .unwrap();
    fills_repo
        .insert(
            "kalshi",
            &make_fill("fill-2", "MKT-B", 5, false),
            None,
            Some("mech_extremes"),
        )
        .await
        .unwrap();

    let agg = repo.fills_aggregate("MKT-B").await.unwrap();
    let realized_pnl: i64 = 400;
    let pnl_after_fees = realized_pnl - agg.fees_cents;

    let inserted = repo
        .insert(
            "ts-1",
            "MKT-B",
            "kalshi",
            agg.strategy.as_deref(),
            None, // producer -> D4
            realized_pnl,
            agg.fees_cents,
            pnl_after_fees,
            agg.n_fills,
            agg.maker_fills,
            "2026-06-18T10:00:00.000Z",
            "2026-06-18T10:01:00.000Z",
        )
        .await
        .unwrap();
    assert!(inserted, "first insert returns true");

    // Verify row content via raw SQL.
    let row = sqlx::query!(
        r#"SELECT realized_pnl_cents, fees_cents, pnl_after_fees_cents,
                  n_fills, maker_fills, strategy
           FROM trade_scores WHERE trade_score_id = 'ts-1'"#
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.realized_pnl_cents, 400);
    assert_eq!(row.fees_cents, 7);
    assert_eq!(row.pnl_after_fees_cents, 393);
    assert_eq!(row.n_fills, 2);
    assert_eq!(row.maker_fills, 1);
    assert_eq!(row.strategy.as_deref(), Some("mech_extremes"));
}

// ─── idempotency: second insert returns Ok(false) ─────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn trade_score_insert_idempotent(pool: PgPool) {
    let repo = TradeScoresRepo::new(pool.clone());

    // First insert.
    let ins1 = repo
        .insert(
            "ts-idem-1",
            "MKT-C",
            "kalshi",
            Some("mech_extremes"),
            None,
            400,
            7,
            393,
            2,
            1,
            "2026-06-18T10:00:00.000Z",
            "2026-06-18T10:01:00.000Z",
        )
        .await
        .unwrap();
    assert!(ins1, "first insert returns true");

    // Second insert — same (market_id, strategy) must be Ok(false).
    let ins2 = repo
        .insert(
            "ts-idem-2", // different PK but same UNIQUE key
            "MKT-C",
            "kalshi",
            Some("mech_extremes"),
            None,
            500,
            8,
            492,
            3,
            2,
            "2026-06-18T11:00:00.000Z",
            "2026-06-18T11:01:00.000Z",
        )
        .await
        .unwrap();
    assert!(
        !ins2,
        "second insert with same (market_id, strategy) -> Ok(false)"
    );

    // Exactly one row remains.
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM trade_scores WHERE market_id = 'MKT-C'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1, "only one row after idempotent second insert");
}

// ─── mutation-proof: the trigger refuses UPDATE ────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn trade_score_append_only_trigger_refuses_update(pool: PgPool) {
    let repo = TradeScoresRepo::new(pool.clone());

    repo.insert(
        "ts-mut",
        "MKT-D",
        "kalshi",
        Some("mech_extremes"),
        None,
        400,
        7,
        393,
        2,
        1,
        "2026-06-18T10:00:00.000Z",
        "2026-06-18T10:01:00.000Z",
    )
    .await
    .unwrap();

    // Raw UPDATE must be refused by the trigger.
    let err =
        sqlx::query!("UPDATE trade_scores SET fees_cents = 99 WHERE trade_score_id = 'ts-mut'")
            .execute(&pool)
            .await;
    assert!(err.is_err(), "trigger should have refused the UPDATE");
}
