//! Task A7 Part 1b test: phase_c_db_metrics emits the five named families
//! with correct labels and values from seeded ledger data.
//! Non-vacuous: all assertions use specific seeded values.

use fortuna_live::telemetry::phase_c_db_metrics;
use sqlx::PgPool;

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn phase_c_db_metrics_emits_all_five_families(pool: PgPool) {
    // ── Seed: one fill (venue="kalshi") ──────────────────────────────────
    sqlx::query(
        "INSERT INTO fills \
         (fill_id, venue, venue_order_id, client_order_id, market_id, \
          side, action, price_cents, qty, fee_cents, is_maker, at, producer, strategy) \
         VALUES \
         ('f-a7-1', 'kalshi', 'v-1', 'c-1', 'm-1', \
          'yes', 'buy', 50, 10, 1, true, '2026-06-18T00:00:00.000Z', \
          'aeolus_weather', 'mech_extremes')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed: one settlement_entry (venue="kalshi") ───────────────────────
    sqlx::query(
        "INSERT INTO settlement_entries \
         (settlement_id, market_id, venue, amount_cents, status, detail, at) \
         VALUES \
         ('s-a7-1', 'm-1', 'kalshi', 100, 'posted', '{}', '2026-06-18T01:00:00.000Z')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed: one trade_score (strategy="mech_extremes", pnl=250) ────────
    sqlx::query(
        "INSERT INTO trade_scores \
         (trade_score_id, market_id, venue, strategy, producer, \
          realized_pnl_cents, fees_cents, pnl_after_fees_cents, n_fills, maker_fills, \
          settled_at, scored_at) \
         VALUES \
         ('ts-a7-1', 'm-1', 'kalshi', 'mech_extremes', 'aeolus_weather', \
          260, 10, 250, 1, 1, '2026-06-18T01:00:00.000Z', '2026-06-18T02:00:00.000Z')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed: one scalar_belief + one belief_score (producer="aeolus_weather") ──
    sqlx::query(
        "INSERT INTO scalar_beliefs \
         (belief_id, producer, event_key, quantiles, unit, horizon, provenance, created_at) \
         VALUES \
         ('sb-a7-1', 'aeolus_weather', 'KBOS:2026-06-20T18:00:00.000Z', \
          '{\"0.1\":60.0,\"0.5\":65.0,\"0.9\":70.0}', 'degF', \
          '2026-06-20T18:00:00.000Z', '{\"model_id\":\"aeolus\"}', \
          '2026-06-18T00:00:00.000Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO belief_scores \
         (score_id, belief_id, rule_id, score, scored_at) \
         VALUES \
         ('bs-a7-1', 'sb-a7-1', 'crps_pinball', 2.5, '2026-06-18T03:00:00.000Z')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Run ──────────────────────────────────────────────────────────────
    let reg = phase_c_db_metrics(&pool)
        .await
        .expect("phase_c_db_metrics ok");
    let snap = reg.snapshot();

    // fortuna_fills_total{venue="kalshi"} == 1
    assert_eq!(
        snap.get("fortuna_fills_total{venue=\"kalshi\"}").copied(),
        Some(1),
        "fills total for kalshi venue"
    );

    // fortuna_settlements_total{venue="kalshi"} == 1
    assert_eq!(
        snap.get("fortuna_settlements_total{venue=\"kalshi\"}")
            .copied(),
        Some(1),
        "settlements total for kalshi venue"
    );

    // fortuna_realized_pnl_cents{strategy="mech_extremes"} == 250
    assert_eq!(
        snap.get("fortuna_realized_pnl_cents{strategy=\"mech_extremes\"}")
            .copied(),
        Some(250),
        "realized pnl for mech_extremes strategy"
    );

    // fortuna_trade_scores_total{strategy="mech_extremes"} == 1
    assert_eq!(
        snap.get("fortuna_trade_scores_total{strategy=\"mech_extremes\"}")
            .copied(),
        Some(1),
        "trade scores count for mech_extremes"
    );

    // fortuna_belief_scores_total{producer="aeolus_weather"} == 1
    assert_eq!(
        snap.get("fortuna_belief_scores_total{producer=\"aeolus_weather\"}")
            .copied(),
        Some(1),
        "belief scores count for aeolus_weather producer"
    );
}

/// Empty DB → all five families are described (no series but no panic).
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn phase_c_db_metrics_empty_db_returns_empty_families(pool: PgPool) {
    let reg = phase_c_db_metrics(&pool).await.expect("empty db ok");
    let snap = reg.snapshot();
    // No fills → no fortuna_fills_total series (but described, just no rows)
    assert!(
        !snap.keys().any(|k| k.starts_with("fortuna_fills_total{")),
        "empty DB: no fills series emitted"
    );
}
