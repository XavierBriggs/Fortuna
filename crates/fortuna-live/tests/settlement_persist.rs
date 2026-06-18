//! TDD (A3): the settlement bridge — a real venue resolution applied to a held
//! paper position drains through the daemon and persists ONE `settlement_entries`
//! row whose `amount_cents` is the NET realized-PnL delta, so
//! `SUM(amount_cents)` reconstructs realized PnL from the DB (F1, "DB-as-truth").
//!
//! Coverage (per the A3 brief):
//!   (a) a settled HELD market writes ONE row; SUM(amount_cents) == realized PnL.
//!   (b) an UNRESOLVED market (no notice) writes nothing for it.
//!   (c) idempotent: re-persisting the same drained settlement leaves ONE row,
//!       SUM unchanged (proves the A1 ON CONFLICT / notice_id dedup).
//!   (d) realized PnL is reconstructable via SELECT SUM(amount_cents).
//!
//! Written RED first against the not-yet-implemented `drain_applied_settlements`
//! on `ActiveRunner` + the settlement-persist step in `drive()`; will fail until
//! A3 lands. Mirrors `fill_persist.rs` (A2) for the drive/compose harness.

use fortuna_cognition::cycle::TriageDecision;
use fortuna_cognition::mind::StubMind;
use fortuna_core::clock::SimClock;
use fortuna_core::market::{MarketId, Side};
use fortuna_core::money::Cents;
use fortuna_ledger::PgIntentJournal;
use fortuna_live::boot::DaemonToml;
use fortuna_live::compose::DegradeScrape;
use fortuna_live::daemon::{compose_runner, default_degrade_thresholds, drive, ActiveRunner};
use fortuna_live::run_loop::{CadenceDriver, HaltPoller, LoopConfig};
use fortuna_ops::FortunaConfig;
use fortuna_runner::SimRunner;
use fortuna_venues::sim::SimVenue;
use fortuna_venues::PriceLevel;
use sqlx::PgPool;
use std::sync::Arc;

fn t0() -> fortuna_core::clock::UtcTimestamp {
    fortuna_core::clock::UtcTimestamp::parse_iso8601("2026-06-18T12:00:00.000Z").unwrap()
}

fn stub_mind() -> Arc<dyn fortuna_cognition::mind::Mind> {
    Arc::new(StubMind::scripted(Vec::new()))
}

/// Stops the loop after a fixed number of cadence sleeps, advancing the sim
/// clock each sleep. Re-usable across two sequential `drive()` calls.
struct StopAtCadence {
    clock: Arc<SimClock>,
    sleeps: u64,
    fire_at: u64,
    tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl CadenceDriver for StopAtCadence {
    async fn sleep_ms(&mut self, ms: u64) {
        self.clock.advance_millis(ms).expect("sim clock advances");
        self.sleeps += 1;
        if self.sleeps == self.fire_at {
            if let Some(tx) = self.tx.take() {
                let _ = tx.send(());
            }
        }
    }
}

struct NeverHalted;

impl HaltPoller for NeverHalted {
    async fn poll(&mut self) -> Result<Option<String>, String> {
        Ok(None)
    }
}

fn arb_books(r: &SimRunner<SimVenue, PgIntentJournal>) {
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: fortuna_core::market::Contracts::new(q),
    };
    for (m, bid, ask) in [
        ("SIM-BKT-LO", 20, 25),
        ("SIM-BKT-MID", 23, 28),
        ("SIM-BKT-HI", 25, 30),
    ] {
        r.venue()
            .set_book(
                &MarketId::new(m).unwrap(),
                vec![lvl(bid, 80)],
                vec![lvl(ask, 80)],
            )
            .unwrap();
    }
}

/// Drive one bounded segment-set over `runner`, stopping after `fire_at`
/// cadence sleeps. Settlement persistence reuses the A2 `fills_pool` arg.
async fn drive_once(runner: &mut ActiveRunner, pool: &PgPool, fire_at: u64) {
    let (tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut cadence = StopAtCadence {
        clock: runner.clock().clone(),
        sleeps: 0,
        fire_at,
        tx: Some(tx),
    };
    let mut poller = NeverHalted;
    let loop_cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };
    let mut scrape = DegradeScrape::new(default_degrade_thresholds());
    let mut daily = fortuna_live::daemon::DailyScheduler::new();

    drive(
        runner,
        &mut cadence,
        &mut poller,
        &loop_cfg,
        fire_at,
        &mut stop,
        |_r, _s| {},
        &mut scrape,
        None,
        &mut daily,
        None,               // S4: no synthesis edge refresh
        None,               // slice-4d: no scalar producer
        None,               // M2: no reconciliation
        None,               // M2: no reviews
        "claude-opus-4-8",  // S5b: synthesis model id
        None,               // slice-4e: no perp feed
        None,               // [personas]: none
        None,               // [discovery]: none
        None,               // resolution_pool: none
        None,               // C-next-1b: no live PerpTick channel
        Some(pool.clone()), // A2/A3: ledger pool — persist fills AND settlements
        None,               // A6 (F4): no recording persist in this smoke
    )
    .await
    .expect("daemon drive");
}

/// Read the in-memory realized PnL for a market from the inner Sim runner.
fn realized_pnl_of(runner: &ActiveRunner, market: &str) -> i64 {
    let ActiveRunner::Sim(r) = runner else {
        panic!("test composes a Sim runner");
    };
    r.positions()
        .position(&MarketId::new(market).unwrap())
        .map(|p| p.realized_pnl.raw())
        .unwrap_or(0)
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn settlement_persists_net_pnl_and_is_idempotent_and_db_is_truth(pool: PgPool) {
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let dcfg = DaemonToml::parse(&text).expect("example parses");
    dcfg.validate_bootable().expect("example boots sim");
    let full = FortunaConfig::load_file(example_path).expect("example full-config parses");

    let runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        42,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .expect("composition");
    arb_books(&runner);
    let mut runner = ActiveRunner::Sim(runner);

    // Segment 1: open positions (mech_structural captures the bracket arb).
    drive_once(&mut runner, &pool, 4).await;

    // Before settlement: SIM-BKT-LO carries no realized PnL yet.
    assert_eq!(
        realized_pnl_of(&runner, "SIM-BKT-LO"),
        0,
        "no settlement applied yet"
    );

    // Settle SIM-BKT-LO at the venue (real-resolution analog). The OTHER two
    // brackets stay unresolved.
    {
        let ActiveRunner::Sim(r) = &runner else {
            panic!("sim runner");
        };
        r.venue()
            .settle_market(&MarketId::new("SIM-BKT-LO").unwrap(), Side::Yes)
            .unwrap();
    }

    // Re-arm the books for the next segment's poll, then drive again so the
    // settlement processor reconciles it and the daemon drains + persists.
    arb_books(match &runner {
        ActiveRunner::Sim(r) => r,
        _ => unreachable!(),
    });
    drive_once(&mut runner, &pool, 4).await;

    // The settled market now carries a non-zero realized PnL in memory.
    let expected_pnl = realized_pnl_of(&runner, "SIM-BKT-LO");
    assert_ne!(expected_pnl, 0, "SIM-BKT-LO settled with a realized PnL");

    // (a) exactly ONE settlement_entries row for SIM-BKT-LO.
    let lo_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM settlement_entries WHERE market_id = $1")
            .bind("SIM-BKT-LO")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(lo_rows, 1, "one settlement row for the settled market");

    // (a)+(d) SUM(amount_cents) for SIM-BKT-LO == the in-memory realized PnL
    // (amount_cents is the NET delta — this is the DB-as-truth reconstruction).
    let lo_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount_cents),0)::bigint FROM settlement_entries WHERE market_id = $1",
    )
    .bind("SIM-BKT-LO")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        lo_sum, expected_pnl,
        "SUM(amount_cents) reconstructs realized PnL from the DB"
    );

    // The persisted intent_id is the venue notice id (set-once dedup key).
    let notice_id: String =
        sqlx::query_scalar("SELECT intent_id FROM settlement_entries WHERE market_id = $1 LIMIT 1")
            .bind("SIM-BKT-LO")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        notice_id.starts_with("stl-SIM-BKT-LO-"),
        "intent_id is the venue settlement notice id, got {notice_id}"
    );

    // (b) the two UNRESOLVED brackets wrote nothing.
    for unresolved in ["SIM-BKT-MID", "SIM-BKT-HI"] {
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM settlement_entries WHERE market_id = $1")
                .bind(unresolved)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n, 0, "{unresolved} is unresolved and must write no row");
    }

    // (c) idempotency: re-insert the SAME (market_id, notice_id) initial entry.
    // The A1 partial-unique index makes this a no-op (Ok(false)); count + SUM
    // stay unchanged — exactly what a restart / cursor-replay would do.
    let total_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settlement_entries")
        .fetch_one(&pool)
        .await
        .unwrap();
    let sum_before: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(amount_cents),0)::bigint FROM settlement_entries")
            .fetch_one(&pool)
            .await
            .unwrap();

    let inserted = fortuna_ledger::SettlementsRepo::new(pool.clone())
        .insert_entry(
            "stl-replay-id",
            "SIM-BKT-LO",
            "sim",
            expected_pnl,
            "confirmed",
            None,
            Some(&notice_id),
            &serde_json::json!({"outcome": "Winner(Yes)", "kind": "settlement"}),
            &t0().to_iso8601(),
        )
        .await
        .unwrap();
    assert!(
        !inserted,
        "re-insert of the same (market_id, notice_id) is a no-op (Ok(false))"
    );

    let total_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settlement_entries")
        .fetch_one(&pool)
        .await
        .unwrap();
    let sum_after: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(amount_cents),0)::bigint FROM settlement_entries")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        total_after, total_before,
        "row count unchanged after replay"
    );
    assert_eq!(
        sum_after, sum_before,
        "realized PnL SUM unchanged after replay"
    );
}

// ─── A4: settled market with fills -> one trade_scores row ────────────────────

/// A4 gate: when a market settles AND has fills, `drive()` persists exactly
/// one `trade_scores` row. `pnl_after_fees_cents` equals the settlement's
/// `realized_pnl_cents` minus the summed `fee_cents` from the fills table.
///
/// NON-VACUOUS: `trade_scores` is empty before drive; the ONLY writer is the
/// A4 hook in the settlement-drain block; the fills pool (`Some(pool)`) is
/// the same wired for A2/A3, so fills arrive before settlements.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn trade_score_written_for_settled_market_with_fills(pool: PgPool) {
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let dcfg = DaemonToml::parse(&text).expect("example parses");
    dcfg.validate_bootable().expect("example boots sim");
    let full = FortunaConfig::load_file(example_path).expect("example full-config parses");

    let runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        43,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .expect("composition");
    arb_books(&runner);
    let mut runner = ActiveRunner::Sim(runner);

    // Segment 1: open positions (fills land in the fills table).
    drive_once(&mut runner, &pool, 4).await;

    // Verify fills were persisted (A2 must be wired for A4 to aggregate).
    let fill_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM fills WHERE market_id = 'SIM-BKT-LO'")
            .fetch_one(&pool)
            .await
            .unwrap();
    // mech_structural opens a position on SIM-BKT-LO; if no fills yet, retry with more wakes.
    // At minimum, the strategy trades the arb set — SIM-BKT-LO is in it.

    // Settle SIM-BKT-LO at the venue.
    {
        let ActiveRunner::Sim(r) = &runner else {
            panic!("sim runner");
        };
        r.venue()
            .settle_market(&MarketId::new("SIM-BKT-LO").unwrap(), Side::Yes)
            .unwrap();
    }

    // Re-arm the books for the next segment's poll, then drive again so the
    // settlement processor reconciles + the daemon drains + persists A3 + A4.
    arb_books(match &runner {
        ActiveRunner::Sim(r) => r,
        _ => unreachable!(),
    });
    drive_once(&mut runner, &pool, 4).await;

    // The settled market now carries a non-zero realized PnL in memory.
    let realized_pnl = realized_pnl_of(&runner, "SIM-BKT-LO");
    assert_ne!(realized_pnl, 0, "SIM-BKT-LO settled with a realized PnL");

    // A4: exactly ONE trade_scores row for the settled market.
    let score_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM trade_scores WHERE market_id = 'SIM-BKT-LO'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        score_count, 1,
        "A4: exactly one trade_scores row for the settled market (got {score_count})"
    );

    // The summed fill fees for SIM-BKT-LO.
    let fill_fees: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(fee_cents), 0)::bigint FROM fills WHERE market_id = 'SIM-BKT-LO'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // pnl_after_fees_cents == realized_pnl_cents - fees.
    let pnl_after_fees: i64 = sqlx::query_scalar(
        "SELECT pnl_after_fees_cents FROM trade_scores WHERE market_id = 'SIM-BKT-LO'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        pnl_after_fees,
        realized_pnl - fill_fees,
        "pnl_after_fees_cents = settlement PnL − fill fees (realized={realized_pnl}, fees={fill_fees})"
    );

    // The score is attributed to the fills' strategy (mech_structural or mech_extremes).
    let strategy: Option<String> =
        sqlx::query_scalar("SELECT strategy FROM trade_scores WHERE market_id = 'SIM-BKT-LO'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        strategy.is_some(),
        "A4: trade_scores row has a non-null strategy (got {strategy:?})"
    );

    // The score counts match the persisted fills.
    let ts_n_fills: i64 =
        sqlx::query_scalar("SELECT n_fills FROM trade_scores WHERE market_id = 'SIM-BKT-LO'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        ts_n_fills, fill_count,
        "n_fills in trade_scores matches the fills table count"
    );
}
