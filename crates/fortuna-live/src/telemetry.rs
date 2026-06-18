//! Phase-C named metric families derived from persisted DB state (Task A7).
//!
//! `phase_c_db_metrics(pool)` is the single public entry point: it runs five
//! grouped COUNT/SUM queries (one per family) and populates a MetricsRegistry.
//! Pure read — no money-path effect, no halt influence, never panics.
//! Called by the daemon's segment closure (via `block_on`) after `registry_from`,
//! then merged into the telemetry view via `MetricsRegistry::merge`.
//!
//! Uses `sqlx::query` (the non-macro, runtime-only form) so no `.sqlx/` offline
//! files are required for these queries — they are validated at runtime, not at
//! compile time.

use fortuna_ops::metrics::MetricsRegistry;
use sqlx::{PgPool, Row};

/// Build a MetricsRegistry from the five Phase-C persisted-state families.
///
/// Errors are propagated so the caller can degrade gracefully: the telemetry
/// view is non-critical; a DB error must never crash the daemon.
pub async fn phase_c_db_metrics(pool: &PgPool) -> Result<MetricsRegistry, sqlx::Error> {
    let mut m = MetricsRegistry::new();

    // 1. fortuna_fills_total{venue}
    m.describe_counter("fortuna_fills_total", "Total fills persisted, by venue");
    let rows = sqlx::query(
        "SELECT COALESCE(venue,'(none)') AS venue, COUNT(*)::BIGINT AS cnt \
         FROM fills GROUP BY venue",
    )
    .fetch_all(pool)
    .await?;
    for row in &rows {
        let venue: String = row.try_get("venue")?;
        let cnt: i64 = row.try_get("cnt")?;
        // Positive count from a COUNT(*) is always non-negative; safe to ignore
        // the MetricsError (would only fire for negative, which COUNT never returns).
        let _ = m.inc_counter("fortuna_fills_total", &[("venue", &venue)], cnt);
    }

    // 2. fortuna_settlements_total{venue}
    m.describe_counter(
        "fortuna_settlements_total",
        "Total settlement entries persisted, by venue",
    );
    let rows = sqlx::query(
        "SELECT COALESCE(venue,'(none)') AS venue, COUNT(*)::BIGINT AS cnt \
         FROM settlement_entries GROUP BY venue",
    )
    .fetch_all(pool)
    .await?;
    for row in &rows {
        let venue: String = row.try_get("venue")?;
        let cnt: i64 = row.try_get("cnt")?;
        let _ = m.inc_counter("fortuna_settlements_total", &[("venue", &venue)], cnt);
    }

    // 3. fortuna_realized_pnl_cents{strategy} — GAUGE (can be negative)
    m.describe_gauge(
        "fortuna_realized_pnl_cents",
        "Realized PnL after fees (cents), by strategy",
    );
    let rows = sqlx::query(
        "SELECT COALESCE(strategy,'(none)') AS strategy, \
                COALESCE(SUM(pnl_after_fees_cents),0)::BIGINT AS total \
         FROM trade_scores GROUP BY strategy",
    )
    .fetch_all(pool)
    .await?;
    for row in &rows {
        let strategy: String = row.try_get("strategy")?;
        let total: i64 = row.try_get("total")?;
        m.set_gauge(
            "fortuna_realized_pnl_cents",
            &[("strategy", &strategy)],
            total,
        );
    }

    // 4. fortuna_trade_scores_total{strategy}
    m.describe_counter(
        "fortuna_trade_scores_total",
        "Total trade score records, by strategy",
    );
    let rows = sqlx::query(
        "SELECT COALESCE(strategy,'(none)') AS strategy, COUNT(*)::BIGINT AS cnt \
         FROM trade_scores GROUP BY strategy",
    )
    .fetch_all(pool)
    .await?;
    for row in &rows {
        let strategy: String = row.try_get("strategy")?;
        let cnt: i64 = row.try_get("cnt")?;
        let _ = m.inc_counter(
            "fortuna_trade_scores_total",
            &[("strategy", &strategy)],
            cnt,
        );
    }

    // 5. fortuna_belief_scores_total{producer}
    m.describe_counter(
        "fortuna_belief_scores_total",
        "Total belief scores, by producer",
    );
    let rows = sqlx::query(
        "SELECT sb.producer, COUNT(*)::BIGINT AS cnt \
         FROM belief_scores bs \
         JOIN scalar_beliefs sb ON sb.belief_id = bs.belief_id \
         GROUP BY sb.producer",
    )
    .fetch_all(pool)
    .await?;
    for row in &rows {
        let producer: String = row.try_get("producer")?;
        let cnt: i64 = row.try_get("cnt")?;
        let _ = m.inc_counter(
            "fortuna_belief_scores_total",
            &[("producer", &producer)],
            cnt,
        );
    }

    Ok(m)
}
