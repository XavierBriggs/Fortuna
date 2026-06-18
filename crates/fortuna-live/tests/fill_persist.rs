//! TDD (A2): fills produced by a paper/sim run drain through the daemon and
//! persist to the `fills` table with the strategy id; a second persist of the
//! same fills (simulating a restart replay) is a safe no-op (idempotent via
//! `ON CONFLICT (fill_id) DO NOTHING`).
//!
//! Written RED first against the not-yet-implemented `drain_applied_fills`
//! on `ActiveRunner` and the fill-persist step in `drive()`; will fail
//! until A2 lands.

use fortuna_cognition::cycle::TriageDecision;
use fortuna_cognition::mind::StubMind;
use fortuna_core::clock::SimClock;
use fortuna_core::market::MarketId;
use fortuna_core::money::Cents;
use fortuna_ledger::PgIntentJournal;
use fortuna_live::boot::DaemonToml;
use fortuna_live::compose::DegradeScrape;
use fortuna_live::daemon::{compose_runner, default_degrade_thresholds, drive};
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

/// After the daemon drives a segment that produces fills, the `fills` table
/// contains those fills with the correct strategy and `producer = NULL`.
/// A second drive over the same fills (replay simulation) leaves the count
/// unchanged (idempotent via ON CONFLICT (fill_id) DO NOTHING).
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn fills_persist_with_strategy_and_idempotent_on_replay(pool: PgPool) {
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

    let (tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut cadence = StopAtCadence {
        clock: runner.clock.clone(),
        sleeps: 0,
        fire_at: 4,
        tx: Some(tx),
    };
    let mut poller = NeverHalted;
    let loop_cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };
    let mut scrape = DegradeScrape::new(default_degrade_thresholds());
    let mut daily = fortuna_live::daemon::DailyScheduler::new();
    let mut runner = fortuna_live::daemon::ActiveRunner::Sim(runner);

    drive(
        &mut runner,
        &mut cadence,
        &mut poller,
        &loop_cfg,
        4,
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
        Some(pool.clone()), // A2: fills_pool — persist paper fills
        None,               // A6 (F4): no recording persist in this smoke
    )
    .await
    .expect("daemon drive");

    // At least one fill row must exist (mech_structural captures the arb).
    let fill_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fills")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        fill_count >= 1,
        "fills table has at least one row; got {fill_count}"
    );

    // Every persisted fill carries a strategy (mech_structural) and NULL producer.
    let bad_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM fills WHERE strategy IS NULL OR producer IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bad_rows, 0, "all fills have strategy set and producer=NULL");

    // Idempotency: FillsRepo::insert uses ON CONFLICT (fill_id) DO NOTHING,
    // so calling it again with the same fill_id returns Ok(false) and the
    // count stays unchanged (exactly what a restart replay would do).
    // Read the fill_id from the DB to construct a minimal Fill for the re-insert.
    let fill_id: String = sqlx::query_scalar("SELECT fill_id FROM fills LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Build a minimal Fill that matches enough fields for the repo insert (the
    // ON CONFLICT key is fill_id — the other fields don't matter for the test).
    // We re-insert with a known fill_id that already exists.
    use fortuna_core::book::Fill;
    use fortuna_core::clock::UtcTimestamp;
    use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, VenueOrderId};
    use fortuna_core::money::Cents;
    let dup_fill = Fill {
        fill_id: fill_id.clone(),
        venue_order_id: VenueOrderId::new("dup-order").unwrap(),
        client_order_id: ClientOrderId::new("dup-coid").unwrap(),
        market: MarketId::new("SIM-BKT-LO").unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        price: Cents::new(25),
        qty: Contracts::new(1),
        fee: Cents::new(0),
        is_maker: false,
        at: UtcTimestamp::parse_iso8601("2026-06-18T12:00:00.000Z").unwrap(),
    };

    let inserted = fortuna_ledger::FillsRepo::new(pool.clone())
        .insert("sim", &dup_fill, None, Some("mech_structural"))
        .await
        .unwrap();
    assert!(
        !inserted,
        "ON CONFLICT DO NOTHING: re-insert of known fill_id returns false"
    );

    let fill_count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fills")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        fill_count_after, fill_count,
        "fill count unchanged after replay (idempotency confirmed)"
    );
}
