//! A2d slice 3 part 1 tests: the realized-funding store (spec
//! docs/design/perp-strategies-and-scalar-claims.md §9.1; I5).
//!
//! Written FROM the spec/prompt text BEFORE the implementation (TDD). They
//! cover, adversarially:
//!   - insert -> read-back: a finalized rate inserts (`true`), `realized_rate`
//!     returns it, `latest_funding_time` returns that boundary;
//!   - IDEMPOTENT re-poll: a re-insert of the SAME (market, funding_time)
//!     returns `false`, no error, the row count stays 1 (a finalized rate
//!     never changes — `ON CONFLICT DO NOTHING`, NOT a mutation);
//!   - a DIFFERENT funding_time for the same market inserts (`true`), count 2,
//!     and `latest_funding_time` picks the MAX boundary;
//!   - `realized_rate` / `latest_funding_time` for an absent key -> `None`;
//!   - the DB-level append-only guard: a raw UPDATE and a raw DELETE are both
//!     refused by the `funding_rates_historical_append_only` trigger (mirrors
//!     how the scalar_beliefs tests prove immutability).
//!
//! Each test gets an isolated, migrated database via #[sqlx::test].
//!
//! Fixture-grounded values are taken from the real PUBLIC capture at
//! docs/research/venue/kinetics-perps-2026-06-10/raw/live_prod_funding_hist_all.json
//! (e.g. KXBCHPERP @ 2026-06-11T04:00:00Z, funding_rate -0.0003971378687289,
//! mark_price "2.0115").

use fortuna_ledger::FundingRatesHistoricalRepo;
use sqlx::PgPool;

// A real (finalized) row from the public funding-history capture.
const TICKER: &str = "KXBCHPERP";
const FT_LATEST: &str = "2026-06-11T04:00:00Z";
const RATE_LATEST: f64 = -0.000_397_137_868_728_9;
const MARK_LATEST: &str = "2.0115"; // per-contract dollar string, verbatim
const FT_EARLIER: &str = "2026-06-10T20:00:00Z";
const RATE_EARLIER: f64 = -0.000_179_146_600_442_7;
const MARK_EARLIER: &str = "1.9540";
const CAPTURED_AT: &str = "2026-06-11T04:05:00.000Z";

// Count rows for a (market, funding_time) — proves the idempotent re-poll
// does not duplicate. A raw COUNT, independent of the repo.
async fn count(pool: &PgPool, ticker: &str, funding_time: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM funding_rates_historical \
         WHERE market_ticker = $1 AND funding_time = $2",
    )
    .bind(ticker)
    .bind(funding_time)
    .fetch_one(pool)
    .await
    .expect("count query")
}

// ─── insert -> read-back round-trip ───────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn insert_then_realized_rate_and_latest(pool: PgPool) {
    let repo = FundingRatesHistoricalRepo::new(pool.clone());

    // A fresh finalized rate inserts: returns true.
    let inserted = repo
        .insert(TICKER, FT_LATEST, RATE_LATEST, MARK_LATEST, CAPTURED_AT)
        .await
        .expect("insert");
    assert!(inserted, "a fresh (market, funding_time) inserts -> true");

    // realized_rate returns exactly the finalized fraction.
    let rate = repo
        .realized_rate(TICKER, FT_LATEST)
        .await
        .expect("realized_rate");
    let rate = rate.expect("the rate is present after insert");
    assert!(
        (rate - RATE_LATEST).abs() < 1e-15,
        "realized_rate round-trips the finalized fraction verbatim"
    );

    // latest_funding_time returns that exact boundary.
    let latest = repo
        .latest_funding_time(TICKER)
        .await
        .expect("latest_funding_time");
    assert_eq!(latest.as_deref(), Some(FT_LATEST));

    // The mark_price string is stored VERBATIM (no float round-trip).
    let mark: String = sqlx::query_scalar(
        "SELECT mark_price FROM funding_rates_historical \
         WHERE market_ticker = $1 AND funding_time = $2",
    )
    .bind(TICKER)
    .bind(FT_LATEST)
    .fetch_one(&pool)
    .await
    .expect("mark_price read");
    assert_eq!(
        mark, MARK_LATEST,
        "mark_price stored as the verbatim string"
    );
}

// ─── idempotent re-poll (a finalized rate never changes) ──────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn reinsert_same_funding_time_is_idempotent(pool: PgPool) {
    let repo = FundingRatesHistoricalRepo::new(pool.clone());

    let first = repo
        .insert(TICKER, FT_LATEST, RATE_LATEST, MARK_LATEST, CAPTURED_AT)
        .await
        .expect("first insert");
    assert!(first, "first insert -> true");
    assert_eq!(count(&pool, TICKER, FT_LATEST).await, 1);

    // Re-poll the SAME (market, funding_time): ON CONFLICT DO NOTHING ->
    // returns false, NO error, and the row count stays 1. This is NOT a
    // mutation (no row is touched), so the append-only trigger never fires —
    // even though the rate/mark/captured_at differ on the re-poll arguments,
    // the stored finalized row is unchanged.
    let second = repo
        .insert(
            TICKER,
            FT_LATEST,
            RATE_LATEST,
            MARK_LATEST,
            "2026-06-11T12:00:00.000Z", // a later poll time, ignored on conflict
        )
        .await
        .expect("idempotent re-insert does not error");
    assert!(!second, "re-insert of the same key -> false");
    assert_eq!(
        count(&pool, TICKER, FT_LATEST).await,
        1,
        "row count stays 1 after an idempotent re-poll"
    );

    // The original captured_at is untouched (DO NOTHING, not DO UPDATE).
    let captured: String = sqlx::query_scalar(
        "SELECT captured_at FROM funding_rates_historical \
         WHERE market_ticker = $1 AND funding_time = $2",
    )
    .bind(TICKER)
    .bind(FT_LATEST)
    .fetch_one(&pool)
    .await
    .expect("captured_at read");
    assert_eq!(captured, CAPTURED_AT, "the first captured_at is preserved");
}

// ─── a different funding_time for the same market inserts; MAX wins ───────────

#[sqlx::test(migrations = "./migrations")]
async fn different_funding_time_inserts_and_latest_is_max(pool: PgPool) {
    let repo = FundingRatesHistoricalRepo::new(pool.clone());

    // Insert the EARLIER boundary first, then the LATER one.
    let a = repo
        .insert(TICKER, FT_EARLIER, RATE_EARLIER, MARK_EARLIER, CAPTURED_AT)
        .await
        .expect("insert earlier");
    assert!(a);
    let b = repo
        .insert(TICKER, FT_LATEST, RATE_LATEST, MARK_LATEST, CAPTURED_AT)
        .await
        .expect("insert later");
    assert!(b, "a different funding_time for the same market -> true");

    // Two distinct rows now exist for this market.
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM funding_rates_historical WHERE market_ticker = $1",
    )
    .bind(TICKER)
    .fetch_one(&pool)
    .await
    .expect("total count");
    assert_eq!(total, 2, "two distinct funding_time rows for the market");

    // latest_funding_time picks the MAX (lexical == chronological on ISO8601).
    let latest = repo
        .latest_funding_time(TICKER)
        .await
        .expect("latest_funding_time");
    assert_eq!(latest.as_deref(), Some(FT_LATEST));

    // Each boundary resolves to its own finalized rate.
    let earlier = repo
        .realized_rate(TICKER, FT_EARLIER)
        .await
        .expect("realized_rate earlier")
        .expect("earlier present");
    assert!((earlier - RATE_EARLIER).abs() < 1e-15);
}

// ─── absent keys resolve to None ──────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn absent_keys_are_none(pool: PgPool) {
    let repo = FundingRatesHistoricalRepo::new(pool);

    // An absent (market, funding_time) -> None (not an error).
    let missing = repo
        .realized_rate(TICKER, FT_LATEST)
        .await
        .expect("realized_rate of an absent key");
    assert!(missing.is_none(), "absent (market, funding_time) -> None");

    // A market with no rows -> latest_funding_time None (the empty-table
    // MAX(...) is SQL NULL, surfaced as None — the poller starts from scratch).
    let no_cursor = repo
        .latest_funding_time("KXNOSUCHPERP")
        .await
        .expect("latest_funding_time of an unseen market");
    assert!(no_cursor.is_none(), "unseen market -> None backfill cursor");
}

// ─── the DB-level append-only guard (immutability) ────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn append_only_trigger_refuses_update_and_delete(pool: PgPool) {
    let repo = FundingRatesHistoricalRepo::new(pool.clone());
    repo.insert(TICKER, FT_LATEST, RATE_LATEST, MARK_LATEST, CAPTURED_AT)
        .await
        .expect("insert");

    // A raw UPDATE (re-writing the finalized rate) is refused by the trigger.
    let upd = sqlx::query(
        "UPDATE funding_rates_historical SET funding_rate = 0.0 \
         WHERE market_ticker = $1 AND funding_time = $2",
    )
    .bind(TICKER)
    .bind(FT_LATEST)
    .execute(&pool)
    .await;
    assert!(
        upd.unwrap_err().to_string().contains("append-only"),
        "a content UPDATE is refused by the append-only trigger"
    );

    // A raw DELETE is refused.
    let del = sqlx::query(
        "DELETE FROM funding_rates_historical \
         WHERE market_ticker = $1 AND funding_time = $2",
    )
    .bind(TICKER)
    .bind(FT_LATEST)
    .execute(&pool)
    .await;
    assert!(
        del.unwrap_err().to_string().contains("append-only"),
        "a DELETE is refused by the append-only trigger"
    );

    // The finalized row survived both refused mutations unchanged.
    let rate = repo
        .realized_rate(TICKER, FT_LATEST)
        .await
        .expect("realized_rate after refused mutations")
        .expect("row still present");
    assert!((rate - RATE_LATEST).abs() < 1e-15);
}
