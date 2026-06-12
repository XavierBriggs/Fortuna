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
    books(r, 80);
}

/// `ask_depth` = 1 leaves a RESTING remainder per leg (working orders at
/// stop) — the SIGTERM-contract vector (cancel working orders on signal).
fn books(r: &SimRunner<PgIntentJournal>, ask_depth: i64) {
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
                vec![lvl(ask, ask_depth)],
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
    let mut daily = fortuna_live::daemon::DailyScheduler::new();

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
        &mut daily,
        None, // S4: no per-segment edge refresh in this smoke
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

    // audit-tail-fix gate #3c: drive() emits AND AUDITS the daily digest on
    // the UTC-day boundary (the first due() fires on boot). route_alerts writes
    // the audit row even with no Slack router (spec 8: every outbound message
    // is also an audit row), so the digest is durably in the trail exactly once.
    let digest_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'alert' AND payload->>'message' LIKE 'FORTUNA digest%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        digest_rows, 1,
        "drive() emitted + audited exactly one digest"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn signal_with_working_orders_cancels_them_and_audits(pool: PgPool) {
    // Gate finding (2026-06-11): the SIGTERM contract is "cancel WORKING
    // orders + final audit row" — the happy smoke had none at stop.
    // Thin asks leave resting remainders working at signal time; the
    // stop channel is the exact channel main's SIGTERM handler fires.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let dcfg = DaemonToml::parse(&text).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 7)
        .await
        .unwrap();
    books(&runner, 1); // depth 1 => partial fill, remainder rests working

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

    let (_stats, shutdown) = drive(
        &mut runner,
        &mut cadence,
        &mut poller,
        &loop_cfg,
        8,
        &mut stop,
        |_r, _s| {},
        &mut scrape,
        None,
        &mut daily,
        None, // S4: no per-segment edge refresh in this smoke
    )
    .await
    .expect("daemon drive");

    assert!(
        shutdown.cancelled >= 1,
        "the signal cancelled working orders (got {shutdown:?})"
    );
    let cancel_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM intent_events WHERE event->>'kind' IN ('cancel_requested', 'cancelled')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        cancel_events >= 1,
        "cancels journaled on signal ({cancel_events})"
    );
    let final_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit WHERE kind = 'daemon_shutdown'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(final_rows, 1, "exactly one final audit row on signal");
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn compose_runner_composes_synthesis_only_when_configured(pool: PgPool) {
    // T4.1/S3b: a [synthesis] section composes the synthesis strategy ALONGSIDE
    // mech (strategy_ids contains "synthesis"); its absence leaves the daemon
    // mechanically-only (fail closed). Asserts the OPT-IN wiring end-to-end:
    // compose_runner -> synthesis_edges -> SynthesisStrategy -> strategies.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    // Seed a confirmed sim edge so synthesis_edges loads a real edge.
    let events = fortuna_ledger::EventsRepo::new(pool.clone());
    events
        .create(
            "evt-1",
            "s",
            "c",
            "src",
            None,
            "2026-06-20T18:00:00.000Z",
            "weather",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();
    fortuna_ledger::EdgesRepo::new(pool.clone())
        .insert_edge(
            "e1",
            "KX-A",
            "sim",
            "evt-1",
            "direct",
            0.9,
            "model:stub",
            Some("op"),
            None,
            "2026-06-10T12:01:00.000Z",
        )
        .await
        .unwrap();

    // WITH [synthesis]: synthesis composed alongside mech.
    let dcfg_with = DaemonToml::parse(&format!("{text}\n[synthesis]\nvenue = \"sim\"\n")).unwrap();
    let runner = compose_runner(pool.clone(), &full, &dcfg_with, t0(), 1)
        .await
        .unwrap();
    let ids: Vec<String> = runner
        .strategy_ids()
        .iter()
        .map(|i| i.to_string())
        .collect();
    assert!(
        ids.iter().any(|i| i == "synthesis"),
        "[synthesis] present => synthesis composed: {ids:?}"
    );
    assert!(
        ids.iter().any(|i| i == "mech_structural"),
        "mech still composed: {ids:?}"
    );

    // WITHOUT [synthesis]: mechanically-only (fail closed).
    let dcfg_without = DaemonToml::parse(&text).unwrap();
    let runner2 = compose_runner(pool, &full, &dcfg_without, t0(), 2)
        .await
        .unwrap();
    let ids2: Vec<String> = runner2
        .strategy_ids()
        .iter()
        .map(|i| i.to_string())
        .collect();
    assert!(
        !ids2.iter().any(|i| i == "synthesis"),
        "no [synthesis] => mechanically-only: {ids2:?}"
    );
    assert!(
        ids2.iter().any(|i| i == "mech_structural"),
        "mech composed: {ids2:?}"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn per_segment_refresh_picks_up_a_newly_confirmed_edge(pool: PgPool) {
    // T4.1/S4 (synthesis-edge-source-decision req 2): the daemon re-loads the
    // confirmed-tier edge set ONCE PER SEGMENT from the ledger — the ledger is
    // the boundary between the discovery loop (which WRITES edges) and the
    // trading daemon (which re-READS them). A daemon that booted with zero
    // confirmed edges (synth arm empty, fail-closed) picks up an edge that is
    // confirmed WHILE IT RUNS, within one segment and with no restart.
    // NON-VACUOUS: the live count moves 0 -> 1 because a REAL confirmed edge
    // was inserted between boot and the segment refresh; a stubbed/again-empty
    // refresh would leave it at 0 and fail the 0 -> 1 assertion.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    // Boot WITH [synthesis] scoped to the sim venue, but with NO edges yet.
    let dcfg = DaemonToml::parse(&format!("{text}\n[synthesis]\nvenue = \"sim\"\n")).unwrap();

    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 9)
        .await
        .expect("composition");
    assert_eq!(
        runner.synthesis_edge_count(),
        Some(0),
        "booted with no confirmed edges => the synth arm is empty (fail closed)"
    );
    arb_books(&runner);

    // The edge becomes CONFIRMED AFTER boot — the running daemon must pick it
    // up on its next segment refresh (not at the next restart).
    let events = fortuna_ledger::EventsRepo::new(pool.clone());
    events
        .create(
            "evt-1",
            "s",
            "c",
            "src",
            None,
            "2026-06-20T18:00:00.000Z",
            "weather",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();
    fortuna_ledger::EdgesRepo::new(pool.clone())
        .insert_edge(
            "e1",
            "KX-A",
            "sim",
            "evt-1",
            "direct",
            0.9,
            "model:stub",
            Some("op"),
            None,
            "2026-06-10T12:01:00.000Z",
        )
        .await
        .unwrap();

    let (tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut cadence = StopAtCadence {
        clock: runner.clock.clone(),
        sleeps: 0,
        fire_at: 6, // one full 4-wake segment refreshes before the stop fires
        tx: Some(tx),
    };
    let mut poller = NeverHalted;
    let loop_cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };
    let mut scrape = DegradeScrape::new(default_degrade_thresholds());
    let mut daily = fortuna_live::daemon::DailyScheduler::new();
    // The S4 wiring main builds: the pool + the SAME [synthesis] filters the
    // composition used, so the per-segment reload is scoped identically.
    let synthesis_refresh = dcfg.synthesis.clone().map(|syn| (pool.clone(), syn));

    let (_stats, _shutdown) = drive(
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
        synthesis_refresh,
    )
    .await
    .expect("daemon drive");

    assert_eq!(
        runner.synthesis_edge_count(),
        Some(1),
        "the per-segment refresh loaded the edge confirmed mid-run (0 -> 1)"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn compose_runner_composes_mech_extremes_with_veto_only_when_configured(pool: PgPool) {
    // T4.1/mech_extremes+veto: a [mech_extremes] section composes the
    // favorite-longshot fade strategy (spec Section 6) ALONGSIDE
    // mech_structural, ENROLLED in the reduce-only model veto (the strategy
    // ships WITH its veto). Its absence leaves it out (fail closed).
    // NON-VACUOUS: WITH [mech_extremes] the runner BOOTS — which a broken
    // wiring could not, because a veto-enrolled strategy with no veto mind
    // FAILS to boot (runner.rs) — and strategy_ids contains "mech_extremes";
    // WITHOUT it, neither holds.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();

    // WITH [mech_extremes] (empty table => conservative defaults): composed +
    // veto-enrolled, and the runner boots clean (the stub veto mind is wired).
    let dcfg_with = DaemonToml::parse(&format!("{text}\n[mech_extremes]\n")).unwrap();
    let runner = compose_runner(pool.clone(), &full, &dcfg_with, t0(), 1)
        .await
        .expect("boots with mech_extremes veto-enrolled + a stub veto mind");
    let ids: Vec<String> = runner
        .strategy_ids()
        .iter()
        .map(|i| i.to_string())
        .collect();
    assert!(
        ids.iter().any(|i| i == "mech_extremes"),
        "[mech_extremes] present => composed: {ids:?}"
    );
    assert!(
        ids.iter().any(|i| i == "mech_structural"),
        "mech_structural still composed alongside: {ids:?}"
    );

    // WITHOUT [mech_extremes]: not composed (fail closed).
    let dcfg_without = DaemonToml::parse(&text).unwrap();
    let runner2 = compose_runner(pool, &full, &dcfg_without, t0(), 2)
        .await
        .unwrap();
    let ids2: Vec<String> = runner2
        .strategy_ids()
        .iter()
        .map(|i| i.to_string())
        .collect();
    assert!(
        !ids2.iter().any(|i| i == "mech_extremes"),
        "no [mech_extremes] => not composed: {ids2:?}"
    );
}
