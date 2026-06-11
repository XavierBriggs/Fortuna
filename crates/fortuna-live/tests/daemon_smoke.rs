//! T4.1 hard requirement 10: the daemon-composition DST smoke — boot ->
//! ticks -> stop-signal -> graceful shutdown, deterministic under
//! SimClock, against the COMMITTED example config (operators copy it; if
//! it cannot boot the daemon, it is broken). The stop channel fired here
//! is the SAME channel main's SIGTERM handler fires: the smoke asserts
//! the signal path end-to-end minus the OS delivery (operator amendment:
//! SIGTERM -> cancel working orders + final audit row).

use fortuna_core::clock::SimClock;
use fortuna_core::market::{Contracts, MarketId};
use fortuna_core::money::Cents;
use fortuna_ledger::PgIntentJournal;
use fortuna_live::boot::DaemonToml;
use fortuna_live::compose::DegradeScrape;
use fortuna_live::daemon::{compose_runner, default_degrade_thresholds, drive};
use fortuna_live::run_loop::{CadenceDriver, HaltPoller, LoopConfig};
use fortuna_ops::FortunaConfig;
use fortuna_runner::SimRunner;
use fortuna_venues::PriceLevel;
use sqlx::PgPool;
use std::sync::Arc;

fn t0() -> fortuna_core::clock::UtcTimestamp {
    fortuna_core::clock::UtcTimestamp::parse_iso8601("2026-06-11T12:00:00.000Z").unwrap()
}

/// Sim cadence that FIRES THE STOP CHANNEL at a chosen wake — the
/// deterministic stand-in for SIGTERM arriving mid-run.
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

fn arb_books(r: &SimRunner<PgIntentJournal>) {
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: Contracts::new(q),
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

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn daemon_smoke_boot_ticks_signal_shutdown(pool: PgPool) {
    // Boot from the COMMITTED example config — both halves.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let dcfg = DaemonToml::parse(&text).expect("example parses");
    dcfg.validate_bootable().expect("example boots sim");
    let full = FortunaConfig::load_file(example_path).expect("example full-config parses");

    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 42)
        .await
        .expect("composition");
    arb_books(&runner);

    // SIGTERM stand-in fires at wake 6 (3 simulated seconds in): some
    // ticks happen first, then the stop wins the next select.
    let (tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut cadence = StopAtCadence {
        clock: runner.clock.clone(),
        sleeps: 0,
        fire_at: 6,
        tx: Some(tx),
    };
    let mut poller = NeverHalted;
    let loop_cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };
    let mut scrape = DegradeScrape::new(default_degrade_thresholds());

    let (stats, shutdown) = drive(
        &mut runner,
        &mut cadence,
        &mut poller,
        &loop_cfg,
        4, // small segments: exercises the segment re-entry path too
        &mut stop,
        |_r, _s| {},
        &mut scrape,
        None,
    )
    .await
    .expect("daemon drive");

    assert!(
        stats.ticks >= 2,
        "the daemon traded before the signal: {stats:?}"
    );
    assert_eq!(
        shutdown.unknown + shutdown.unacked,
        0,
        "clean sim shutdown leaves nothing ambiguous: {shutdown:?}"
    );

    // The signal path's contract artifacts are IN POSTGRES: the trade
    // trail and exactly one final audit row.
    let intents: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT intent_id) FROM intent_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(intents >= 3, "the arb's legs journaled (got {intents})");
    let final_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit WHERE kind = 'daemon_shutdown'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(final_rows, 1, "exactly one final audit row");
}
