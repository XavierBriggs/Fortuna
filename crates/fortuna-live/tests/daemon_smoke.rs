//! T4.1 hard requirement 10: the daemon-composition DST smoke — boot ->
//! ticks -> stop-signal -> graceful shutdown, deterministic under
//! SimClock, against the COMMITTED example config (operators copy it; if
//! it cannot boot the daemon, it is broken). The stop channel fired here
//! is the SAME channel main's SIGTERM handler fires: the smoke asserts
//! the signal path end-to-end minus the OS delivery (operator amendment:
//! SIGTERM -> cancel working orders + final audit row).

use fortuna_cognition::cycle::TriageDecision;
use fortuna_cognition::mind::{Mind, MindOutput, StubMind};
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

/// The inert mind every smoke passes (no beliefs => no synthesis proposals);
/// the synthesis arm is exercised only by the S5a live-trading test below.
fn stub_mind() -> Arc<dyn Mind> {
    Arc::new(StubMind::scripted(Vec::new()))
}

/// A mind that BELIEVES `p` for `event` (the S5a live test's scripted
/// cognition; mirrors the synthesis_loop believing-mind fixture).
fn believing_mind(event: &str, p: f64) -> Arc<dyn Mind> {
    let out: MindOutput = serde_json::from_value(serde_json::json!({
        "beliefs": [{
            "event_id": event,
            "p": p,
            "p_raw": p,
            "horizon": "2026-06-20T18:00:00.000Z",
            "evidence": [{"source": "stub", "ref": "sig-1"}]
        }],
        "proposals": [],
        "journal": null
    }))
    .unwrap();
    Arc::new(StubMind::scripted(vec![out]))
}

/// A mind that returns a JOURNAL entry (the daily-reconciliation product);
/// no beliefs, no proposals. The reconciliation cycle's one job is the journal.
fn journaling_mind(body: &str) -> Arc<dyn Mind> {
    let out: MindOutput = serde_json::from_value(serde_json::json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": body},
        "cost_cents": 5
    }))
    .unwrap();
    Arc::new(StubMind::scripted(vec![out]))
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

    let mut runner = compose_runner(
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
        None, // slice-4d: no scalar producer in this smoke
        None, // M2: no reconciliation in this smoke
        None, // M2: no reviews in this smoke
        None, // slice-4e: no perp feed in this smoke
        None, // [personas]: none in this smoke
        None, // [discovery]: none in this smoke
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
    // S6b: drive now emits the RICH digest ("FORTUNA daily digest — ...");
    // the assertion (exactly one digest audited) is unchanged.
    let digest_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'alert' AND payload->>'message' LIKE 'FORTUNA daily digest%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        digest_rows, 1,
        "drive() emitted + audited exactly one RICH digest"
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
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        7,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
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
        None, // slice-4d: no scalar producer in this smoke
        None, // M2: no reconciliation in this smoke
        None, // M2: no reviews in this smoke
        None, // slice-4e: no perp feed in this smoke
        None, // [personas]: none in this smoke
        None, // [discovery]: none in this smoke
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
    let runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg_with,
        t0(),
        1,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
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
    let runner2 = compose_runner(
        pool,
        &full,
        &dcfg_without,
        t0(),
        2,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
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

    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        9,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
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
        None, // slice-4d: no scalar producer in this smoke
        None, // M2: no reconciliation in this smoke
        None, // M2: no reviews in this smoke
        None, // slice-4e: no perp feed in this smoke
        None, // [personas]: none in this smoke
        None, // [discovery]: none in this smoke
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
async fn refresh_failure_keeps_last_known_edges_alerts_and_survives(pool: PgPool) {
    // T4.1/S4 (synthesis-edge-source-decision req 2, the failure arm): a FAILING
    // per-segment edge refresh KEEPS the last-known set, ALERTS (audit row), and
    // the loop SURVIVES to a clean shutdown — a transient ledger outage never
    // crashes the daemon nor silently drops the synth arm to empty. The T4.1
    // completion gate proved this by a scratch mutation (verdict finding m2);
    // this is its committed equivalent.
    // NON-VACUOUS BY CONSTRUCTION: the booted edge is SUPERSEDED by an
    // UNCONFIRMED successor before drive (asserted: confirmed_edges() now empty),
    // so a SUCCESSFUL refresh would read ZERO confirmed-current edges — a
    // post-drive count of 1 can ONLY mean the failure path retained last-known.
    // (Mutation-checked in dev: swapping the broken pool for the live `pool`
    // turns it RED — count drops to 0 and no alert is written.)
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    let dcfg = DaemonToml::parse(&format!("{text}\n[synthesis]\nvenue = \"sim\"\n")).unwrap();

    // A confirmed sim edge KX-A -> evt-1: the last-known set.
    fortuna_ledger::EventsRepo::new(pool.clone())
        .create(
            "evt-1",
            "s",
            "c",
            "nws",
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

    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        901,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .expect("composition");
    assert_eq!(runner.synthesis_edge_count(), Some(1), "booted with 1 edge");

    // Supersede the edge (the append-only DB refuses DELETE — I5) with an
    // UNCONFIRMED successor: a SUCCESSFUL refresh now reads 0 confirmed heads.
    fortuna_ledger::EdgesRepo::new(pool.clone())
        .insert_edge(
            "e2",
            "KX-A",
            "sim",
            "evt-1",
            "direct",
            0.5,
            "model:stub",
            None,
            Some("e1"),
            "2026-06-10T12:02:00.000Z",
        )
        .await
        .unwrap();
    let confirmed_now = fortuna_ledger::EdgesRepo::new(pool.clone())
        .confirmed_edges()
        .await
        .unwrap();
    assert!(
        confirmed_now.is_empty(),
        "a successful refresh would now read ZERO confirmed-current edges"
    );

    // A lazily-connected pool to a nonexistent DB: every refresh query fails.
    let broken: PgPool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://xavierbriggs@localhost:5432/zz_refresh_no_such_db")
        .unwrap();

    let (tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut cadence = StopAtCadence {
        clock: runner.clock.clone(),
        sleeps: 0,
        fire_at: 10, // two full 4-wake segments refresh (and fail) before stop
        tx: Some(tx),
    };
    let mut poller = NeverHalted;
    let loop_cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };
    let mut scrape = DegradeScrape::new(default_degrade_thresholds());
    let mut daily = fortuna_live::daemon::DailyScheduler::new();
    let syn = dcfg.synthesis.clone().unwrap();

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
        Some((broken, syn)),
        None, // slice-4d: no scalar producer in this smoke
        None, // M2: no reconciliation in this smoke
        None, // M2: no reviews in this smoke
        None, // slice-4e: no perp feed in this smoke
        None, // [personas]: none in this smoke
        None, // [discovery]: none in this smoke
    )
    .await
    .expect("the loop must SURVIVE a failing refresh");

    assert_eq!(
        runner.synthesis_edge_count(),
        Some(1),
        "failure path retained LAST-KNOWN (a successful refresh would read 0)"
    );
    let alert_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'alert'
         AND payload->>'message' LIKE '%edge-refresh failure%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        alert_rows >= 1,
        "the refresh-failure run is alerted/audited (got {alert_rows})"
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
    let runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg_with,
        t0(),
        1,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
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
    let runner2 = compose_runner(
        pool,
        &full,
        &dcfg_without,
        t0(),
        2,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
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

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn compose_runner_composes_perp_strategies_only_when_configured(pool: PgPool) {
    // slice 4c: a [funding_forecast] section composes the zero-capital funding
    // belief producer, and a [perp_event_basis] section (with a valid ladder)
    // composes the propose-only basis strategy — both ALONGSIDE mech_structural,
    // NEITHER veto-enrolled (so no veto mind is required). Their absence leaves
    // them out (fail closed). NON-VACUOUS: WITH the sections the runner BOOTS and
    // strategy_ids contains both perp ids; WITHOUT them, neither holds.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();

    // WITH both perp sections (a 3-rung less/between/greater ladder): composed,
    // and the runner boots clean (no veto mind needed — neither is enrolled).
    let dcfg_with = DaemonToml::parse(&format!(
        "{text}\n\
         [funding_forecast]\n\
         [perp_event_basis]\n\
         perp_market = \"KXBTCPERP\"\n\
         fee_floor_dollars = 4.0\n\
         min_basis_dollars = 1.0\n\
         edge_premium_cents = 2\n\
         [perp_event_basis.ladder.\"KXBTC-LO\"]\n\
         kind = \"less\"\n\
         cap_dollars = 60000.0\n\
         [perp_event_basis.ladder.\"KXBTC-MID\"]\n\
         kind = \"between\"\n\
         floor_dollars = 60000.0\n\
         cap_dollars = 70000.0\n\
         [perp_event_basis.ladder.\"KXBTC-HI\"]\n\
         kind = \"greater\"\n\
         floor_dollars = 70000.0\n"
    ))
    .unwrap();
    let runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg_with,
        t0(),
        1,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .expect("boots with both perp strategies composed (neither veto-enrolled)");
    let ids: Vec<String> = runner
        .strategy_ids()
        .iter()
        .map(|i| i.to_string())
        .collect();
    assert!(
        ids.iter().any(|i| i == "funding_forecast"),
        "[funding_forecast] present => composed: {ids:?}"
    );
    assert!(
        ids.iter().any(|i| i == "perp_event_basis"),
        "[perp_event_basis] present => composed: {ids:?}"
    );
    assert!(
        ids.iter().any(|i| i == "mech_structural"),
        "mech_structural still composed alongside: {ids:?}"
    );

    // WITHOUT the perp sections: neither composed (fail closed).
    let dcfg_without = DaemonToml::parse(&text).unwrap();
    let runner2 = compose_runner(
        pool,
        &full,
        &dcfg_without,
        t0(),
        2,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .unwrap();
    let ids2: Vec<String> = runner2
        .strategy_ids()
        .iter()
        .map(|i| i.to_string())
        .collect();
    assert!(
        !ids2
            .iter()
            .any(|i| i == "funding_forecast" || i == "perp_event_basis"),
        "no perp sections => neither composed: {ids2:?}"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn synthesis_arm_trades_with_ledger_calibration_and_an_injected_mind(pool: PgPool) {
    // T4.1/S5a (the high-value step): the daemon-composed synthesis arm TRADES
    // when [synthesis].category selects a calibration scope the ledger has
    // FITTED params + resolved history for, AND the injected mind believes. The
    // NON-VACUOUS populated path: REAL ledger calibration (fetched by
    // compose_runner, not hand-built) + a scripted belief produce a sized,
    // gated order. Isolation: the synth edge is ONE sim bracket whose book is
    // the only one set, so mech_structural (which arbs complete SETS) trades
    // nothing -- any SIM-BKT-LO position is the synthesis arm's.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let mut full = FortunaConfig::load_file(example_path).unwrap();
    // The example config has NO synthesis capital envelope NOR per-strategy
    // gate (only mech_structural + mech_extremes) -- without them the synth arm
    // prices but sizes ZERO / is gate-rejected fail-closed. Grant both here; the
    // production gap (the example + operator config need `synthesis_cents` +
    // `[gates.per_strategy.synthesis]` for S5b's live arm) is ledgered in GAPS.
    full.envelopes.insert("synthesis".to_string(), 200_000);
    full.gates.per_strategy.insert(
        "synthesis".to_string(),
        fortuna_gates::StrategyLimits {
            max_exposure_cents: 200_000,
            max_order_notional_cents: 10_000,
            min_net_edge_bps: 100,
        },
    );

    // (1) The FITTED params row (identity-ish platt) for the canonical
    // "synth_events" calibration scope the daemon queries.
    fortuna_ledger::CalibrationParamsRepo::new(pool.clone())
        .insert(
            "01PARAM0000000000000000901",
            "claude-fable-5",
            "synth_events",
            "weather",
            "platt",
            &serde_json::json!({
                "version": 1,
                "method": { "Platt": { "a": 0.0, "b": 1.0 } },
                "extremization_k": 1.0,
                "fitted_on_n": 10
            }),
            1,
            "2026-06-11T00:00:00.000Z",
            "2026-06-11T00:00:00.000Z",
        )
        .await
        .unwrap();
    // (2) Resolved, well-calibrated history (p=0.7 resolving true ~70%) =>
    // quality > 0 => non-zero Kelly sizing.
    fortuna_ledger::EventsRepo::new(pool.clone())
        .create(
            "evt-hist",
            "s",
            "c",
            "nws",
            None,
            "2026-06-12T00:00:00.000Z",
            "weather",
            "2026-06-11T00:00:00.000Z",
        )
        .await
        .unwrap();
    // FULL_AUTONOMY_N resolved beliefs (50) => shrink weight w == 1 => the
    // belief does NOT shrink toward the market prior, so fair stays ~70 > ask
    // 60. `i % 10 < 7` keeps p=0.7 resolving true 70% of the time (well-
    // calibrated => positive quality => non-zero size).
    for i in 0..50 {
        let outcome: i32 = if (i % 10) < 7 { 1 } else { 0 };
        let brier = if outcome == 1 {
            (1.0f64 - 0.7).powi(2)
        } else {
            (0.7f64).powi(2)
        };
        sqlx::query(
            "INSERT INTO beliefs (belief_id, event_id, p, p_raw, horizon, status,
                                  outcome, brier, evidence, provenance, created_at)
             VALUES ($1, 'evt-hist', 0.7, 0.7, '2026-06-12T00:00:00.000Z',
                     'resolved', $2, $3, '[]'::jsonb, '{}'::jsonb, $4)",
        )
        .bind(format!("01BELIEFHIST00000000000{i:02}"))
        .bind(outcome)
        .bind(brier)
        .bind(format!("2026-06-11T00:00:{i:02}.000Z"))
        .execute(&pool)
        .await
        .unwrap();
    }
    // (3) The LIVE event + a confirmed sim edge mapping SIM-BKT-LO -> evt-1.
    fortuna_ledger::EventsRepo::new(pool.clone())
        .create(
            "evt-1",
            "s",
            "c",
            "nws",
            None,
            "2026-06-20T18:00:00.000Z",
            "weather",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();
    fortuna_ledger::EdgesRepo::new(pool.clone())
        .insert_edge(
            "e-synth",
            "SIM-BKT-LO",
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

    // [synthesis] venue=sim + category=weather (the calibration scope).
    let dcfg = DaemonToml::parse(&format!(
        "{text}\n[synthesis]\nvenue = \"sim\"\ncategory = \"weather\"\n"
    ))
    .unwrap();
    assert_eq!(
        dcfg.synthesis.as_ref().unwrap().category.as_deref(),
        Some("weather"),
        "[synthesis].category parses => the calibration scope is selected"
    );
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        50,
        believing_mind("evt-1", 0.70),
        TriageDecision::AlwaysAccept,
    )
    .await
    .expect("composition");
    assert_eq!(
        runner.synthesis_edge_count(),
        Some(1),
        "the confirmed sim edge loaded into the synth arm"
    );

    // The ONLY book is SIM-BKT-LO (ask 60 < fair ~70 => a realistic 10c edge
    // that sizes through the gates; mech_structural cannot arb a one-market
    // set, so it trades nothing — any order here is the synthesis arm's).
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: Contracts::new(q),
    };
    runner
        .venue()
        .set_book(
            &MarketId::new("SIM-BKT-LO").unwrap(),
            vec![lvl(58, 80)],
            vec![lvl(60, 80)],
        )
        .unwrap();

    let report = runner.tick().await.unwrap();
    // The non-vacuous populated path: a PROPOSAL proves the injected mind +
    // the LEDGER calibration priced the edge (the cycle would price nothing
    // without either); a SUBMITTED order proves the measured calibration
    // quality fed non-zero sizing through the gates. (The fill/position is an
    // execution-policy detail proven in fortuna-runner's synthesis_loop.)
    assert!(
        report.proposals >= 1,
        "the synthesis arm priced the edge and proposed (ledger calibration + \
         injected belief): {report:?}"
    );
    assert!(
        report.orders_submitted >= 1,
        "the proposal sized (quality > 0) + gated + submitted an order: {report:?}"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn drive_drains_and_persists_the_synthesis_arms_beliefs(pool: PgPool) {
    // T4.1/S6: the daemon's drive loop DRAINS the synthesis arm's belief drafts
    // and PERSISTS them per segment (the calibration substrate). The synth arm
    // drafts a belief whenever its book event fires + the mind believes
    // (calibration is irrelevant to belief DRAFTING — it gates only pricing).
    // NON-VACUOUS: a belief for evt-1 lands in the ledger that was NOT there
    // before drive — the drain->persist wiring is load-bearing.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();

    // A confirmed sim edge SIM-BKT-LO -> evt-1 (the belief's event).
    fortuna_ledger::EventsRepo::new(pool.clone())
        .create(
            "evt-1",
            "s",
            "c",
            "nws",
            None,
            "2026-06-20T18:00:00.000Z",
            "weather",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();
    fortuna_ledger::EdgesRepo::new(pool.clone())
        .insert_edge(
            "e-synth",
            "SIM-BKT-LO",
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

    let dcfg = DaemonToml::parse(&format!("{text}\n[synthesis]\nvenue = \"sim\"\n")).unwrap();
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        60,
        believing_mind("evt-1", 0.70),
        TriageDecision::AlwaysAccept,
    )
    .await
    .expect("composition");
    // A book for the edge market so the synth arm's cycle runs (and drafts).
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: Contracts::new(q),
    };
    runner
        .venue()
        .set_book(
            &MarketId::new("SIM-BKT-LO").unwrap(),
            vec![lvl(58, 80)],
            vec![lvl(60, 80)],
        )
        .unwrap();

    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM beliefs WHERE event_id = 'evt-1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, 0, "no beliefs before drive");

    let (tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut cadence = StopAtCadence {
        clock: runner.clock.clone(),
        sleeps: 0,
        fire_at: 6, // one full 4-wake segment drains+persists before the stop
        tx: Some(tx),
    };
    let mut poller = NeverHalted;
    let loop_cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };
    let mut scrape = DegradeScrape::new(default_degrade_thresholds());
    let mut daily = fortuna_live::daemon::DailyScheduler::new();
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
        None, // slice-4d: no scalar producer in this smoke
        None, // M2: no reconciliation in this smoke
        None, // M2: no reviews in this smoke
        None, // slice-4e: no perp feed in this smoke
        None, // [personas]: none in this smoke
        None, // [discovery]: none in this smoke
    )
    .await
    .expect("daemon drive");

    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM beliefs WHERE event_id = 'evt-1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        after >= 1,
        "the synth arm's belief drafted during the segment was drained + persisted (got {after})"
    );
}

/// slice-4d+4e (operator directive 2026-06-11): the SCALAR-belief egress wired
/// end-to-end — a RECORDED PerpTick feed fires `funding_forecast`, whose scalar
/// drafts the per-segment drain PERSISTS to `scalar_beliefs` (the table ROTA
/// §9.1 groups by `producer`). This is the gate the operator named: the soak
/// must PRODUCE beliefs; a daemon that ticks but persists 0 is NOT done. It
/// proves the WHOLE chain: a recorded `ticker` frame -> from_ws_ticker (4a) ->
/// PerpTickFeed (4e) -> inject_perp_tick (the 4b seam) -> funding_forecast draft
/// -> drain_pending_scalar_beliefs -> persist_scalar_beliefs (4d).
///
/// NON-VACUOUS + MUTATION-PROOF BY CONSTRUCTION: `scalar_beliefs` is empty
/// before drive; the ONLY writer is the slice-4d drain+persist block, which
/// fires ONLY when `scalar_belief_persist` is Some AND a PerpTick was fed.
/// Break the egress (pass `None` for `scalar_belief_persist`) and the post-drive
/// count stays 0 -> this reds. Drop the `perp_tick_feed` (no PerpTick) and
/// funding_forecast never drafts -> also 0. (Both mutations verified RED in dev.)
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn drive_drains_and_persists_funding_forecast_scalar_beliefs(pool: PgPool) {
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();

    // [funding_forecast] composes the zero-capital scalar producer ALONGSIDE
    // mech_structural (not veto-enrolled, so no veto mind). It needs no other
    // config — the PerpTicks arrive via the feed below, not the sim venue.
    let dcfg = DaemonToml::parse(&format!("{text}\n[funding_forecast]\n")).unwrap();
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        70,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .expect("composition with funding_forecast");

    // The recorded kinetics capture (NEVER fabricated) -> a replayable PerpTick
    // feed. drive() injects one recorded PerpTick at the head of EACH segment
    // (the 4b seam), so funding_forecast drafts a scalar belief per segment.
    let feed = fortuna_live::perp_feed::PerpTickFeed::from_ws_ticker_jsonl(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/kinetics-perps/ws__public_orderbook_ticker.jsonl"
    ))
    .expect("recorded ticker fixture parses");

    let before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scalar_beliefs WHERE producer = 'funding_forecast'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, 0, "no scalar beliefs before drive");

    let (tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut cadence = StopAtCadence {
        clock: runner.clock.clone(),
        sleeps: 0,
        fire_at: 6, // one full 4-wake segment injects + drains + persists first
        tx: Some(tx),
    };
    let mut poller = NeverHalted;
    let loop_cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };
    let mut scrape = DegradeScrape::new(default_degrade_thresholds());
    let mut daily = fortuna_live::daemon::DailyScheduler::new();

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
        None,               // synthesis_refresh: no synth arm in this smoke
        Some(pool.clone()), // slice-4d: the scalar producer IS composed -> persist
        None,               // M2: no reconciliation in this smoke
        None,               // M2: no reviews in this smoke
        Some(feed),         // slice-4e: recorded PerpTicks so the producer fires
        None,               // [personas]: none in this smoke
        None,               // [discovery]: none in this smoke
    )
    .await
    .expect("daemon drive");

    let after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scalar_beliefs WHERE producer = 'funding_forecast'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        after >= 1,
        "funding_forecast's scalar belief, fed by a recorded PerpTick, was drained + persisted (got {after})"
    );

    // The persisted row is a well-formed prob_claims/v1 scalar claim, not an
    // empty husk: unit "rate" (funding_forecast's domain) + the {0.1,0.5,0.9}
    // quantile fan. Asserting SHAPE proves the drain carried the real draft.
    let unit: String = sqlx::query_scalar(
        "SELECT unit FROM scalar_beliefs WHERE producer = 'funding_forecast' \
         ORDER BY created_at LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        unit, "rate",
        "funding_forecast persists a funding-rate claim"
    );
    let qlen: i32 = sqlx::query_scalar(
        "SELECT jsonb_array_length(quantiles) FROM scalar_beliefs \
         WHERE producer = 'funding_forecast' ORDER BY created_at LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        qlen, 3,
        "the persisted quantile fan is the {{0.1,0.5,0.9}} forecast"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn daily_reconciliation_writes_a_journal_and_places_no_orders(pool: PgPool) {
    // T4.1/M2 slice 1 (spec 5.8): the daily reconciliation reads the day's
    // fills + open positions (assembled context) and writes the journal entry;
    // it places NO orders (STRUCTURAL — ReconciliationOutcome carries none).
    // A scripted journaling mind drives it. NON-VACUOUS: a journal row appears
    // for the UTC day that was NOT there before, and orders_submitted is
    // UNCHANGED across the call (the day's tick placed real orders first).
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    let dcfg = DaemonToml::parse(&text).unwrap();
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        70,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .unwrap();
    arb_books(&runner);
    runner.tick().await.unwrap(); // real day activity for the context

    let now = t0();
    let day = "2026-06-11";
    assert!(
        fortuna_ledger::JournalRepo::new(pool.clone())
            .get_day(day)
            .await
            .unwrap()
            .is_none(),
        "no journal before reconciliation"
    );
    let orders_before = runner.counters().orders_submitted;

    let mind = journaling_mind("Day flat; no surprises. Tomorrow: hold.");
    let wrote = fortuna_live::daemon::run_daily_reconciliation(
        &mut runner,
        &pool,
        mind.as_ref(),
        now,
        1000,
    )
    .await
    .expect("reconciliation runs");
    assert!(wrote, "a journal-producing mind writes the journal");

    let row = fortuna_ledger::JournalRepo::new(pool.clone())
        .get_day(day)
        .await
        .unwrap();
    let row = row.expect("journal persisted for the UTC day");
    assert!(
        row.body.get("body").is_some(),
        "the journal row carries the entry body"
    );
    assert_eq!(
        runner.counters().orders_submitted,
        orders_before,
        "reconciliation places NO orders (structural)"
    );
    let recon_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'alert'
         AND payload->>'kind' = 'reconciliation' AND payload->>'message' LIKE 'journal written%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(recon_audits, 1, "the reconciliation cycle is audited once");

    // Idempotent: a second call the same UTC day is a no-op (one journal/day).
    let again = fortuna_live::daemon::run_daily_reconciliation(
        &mut runner,
        &pool,
        mind.as_ref(),
        now,
        1001,
    )
    .await
    .unwrap();
    assert!(
        !again,
        "idempotent: already reconciled today => no second journal"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn daily_reconciliation_gracefully_skips_when_the_mind_writes_no_journal(pool: PgPool) {
    // The default StubMind (no key) emits MindOutput::empty() => no journal =>
    // ReconError::NoJournal => GRACEFUL SKIP: Ok(false), no journal row, a skip
    // audit, and the call NEVER errors (the daily boundary must survive).
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    let dcfg = DaemonToml::parse(&text).unwrap();
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        71,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .unwrap();

    let now = t0();
    let day = "2026-06-11";
    let mind = stub_mind(); // StubMind::scripted(vec![]) => MindOutput::empty()
    let wrote = fortuna_live::daemon::run_daily_reconciliation(
        &mut runner,
        &pool,
        mind.as_ref(),
        now,
        2000,
    )
    .await
    .expect("reconciliation must not crash on a no-journal mind");
    assert!(!wrote, "no journal => graceful skip");
    assert!(
        fortuna_ledger::JournalRepo::new(pool.clone())
            .get_day(day)
            .await
            .unwrap()
            .is_none(),
        "no journal row is written on a skip"
    );
    let skip_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'alert'
         AND payload->>'kind' = 'reconciliation' AND payload->>'message' LIKE 'skipped%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(skip_audits, 1, "the skip is audited (never silent)");
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn drive_runs_daily_reconciliation_at_the_utc_day_boundary(pool: PgPool) {
    // T4.1/M2 slice 2: drive() runs the daily reconciliation on the UTC-day
    // boundary (the first due() fires on boot, alongside the digest). With a
    // journaling reconciliation mind wired in, a journal row lands for the day.
    // NON-VACUOUS: no journal exists before drive, and the FIVE sibling drives
    // pass reconciliation=None and write none — only the wiring writes it.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    let dcfg = DaemonToml::parse(&text).unwrap();
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        80,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .unwrap();
    arb_books(&runner);

    let day = "2026-06-11";
    assert!(
        fortuna_ledger::JournalRepo::new(pool.clone())
            .get_day(day)
            .await
            .unwrap()
            .is_none(),
        "no journal before drive"
    );

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
        None,                                                               // synthesis_refresh
        None, // slice-4d: no scalar producer in this e2e
        Some((pool.clone(), journaling_mind("EOD: flat; tomorrow hold."))), // reconciliation
        None, // M2: no reviews in this e2e
        None, // slice-4e: no perp feed in this e2e
        None, // [personas]: none in this e2e
        None, // [discovery]: none in this e2e
    )
    .await
    .expect("daemon drive");

    let row = fortuna_ledger::JournalRepo::new(pool.clone())
        .get_day(day)
        .await
        .unwrap();
    assert!(
        row.is_some(),
        "drive() ran the daily reconciliation at the boundary and wrote the journal"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn weekly_review_audits_the_deterministic_calibration_and_go_nogo(pool: PgPool) {
    // T4.1/M2 slice B1 (spec 5.8 weekly review): the DETERMINISTIC core — a
    // calibration audit (per scope from resolved_stats) + GO/NO-GO recommendations
    // (RECOMMENDATIONS ONLY, I7) — runs even with a StubMind (no commentary).
    // Seed a synth_events/weather scope (50 resolved beliefs), trade once so the
    // digest carries a strategy row, run the review, and assert it READ the data
    // (the calibration scope carries all 50 samples), produced a GO/NO-GO rec,
    // and audited the cycle. NON-VACUOUS: an empty resolved_stats would give n=0.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();

    fortuna_ledger::CalibrationParamsRepo::new(pool.clone())
        .insert(
            "01PARAM0000000000000000990",
            "claude-fable-5",
            "synth_events",
            "weather",
            "platt",
            &serde_json::json!({
                "version": 1,
                "method": { "Platt": { "a": 0.0, "b": 1.0 } },
                "extremization_k": 1.0,
                "fitted_on_n": 50
            }),
            1,
            "2026-06-11T00:00:00.000Z",
            "2026-06-11T00:00:00.000Z",
        )
        .await
        .unwrap();
    fortuna_ledger::EventsRepo::new(pool.clone())
        .create(
            "evt-hist",
            "s",
            "c",
            "nws",
            None,
            "2026-06-12T00:00:00.000Z",
            "weather",
            "2026-06-11T00:00:00.000Z",
        )
        .await
        .unwrap();
    for i in 0..50 {
        let outcome: i32 = if (i % 10) < 7 { 1 } else { 0 };
        let brier = if outcome == 1 {
            (1.0f64 - 0.7).powi(2)
        } else {
            (0.7f64).powi(2)
        };
        sqlx::query(
            "INSERT INTO beliefs (belief_id, event_id, p, p_raw, horizon, status,
                                  outcome, brier, evidence, provenance, created_at)
             VALUES ($1, 'evt-hist', 0.7, 0.7, '2026-06-12T00:00:00.000Z',
                     'resolved', $2, $3, '[]'::jsonb, '{}'::jsonb, $4)",
        )
        .bind(format!("01BELIEFWK0000000000000{i:02}"))
        .bind(outcome)
        .bind(brier)
        .bind(format!("2026-06-11T00:00:{i:02}.000Z"))
        .execute(&pool)
        .await
        .unwrap();
    }

    let dcfg = DaemonToml::parse(&format!(
        "{text}\n[synthesis]\nvenue = \"sim\"\ncategory = \"weather\"\n"
    ))
    .unwrap();
    let review = dcfg.review.clone().expect("the example ships [review]");
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        90,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .unwrap();
    // Trade once so the digest snapshot carries a strategy row (GO/NO-GO needs a
    // StrategyRecord; an arb tick gives mech_structural activity).
    arb_books(&runner);
    runner.tick().await.unwrap();

    // A week after boot (paper_days > 0). StubMind => no commentary; the
    // deterministic core still produces the calibration audit + recommendations.
    let now = fortuna_core::clock::UtcTimestamp::parse_iso8601("2026-06-18T00:00:00.000Z").unwrap();
    let sm = stub_mind();
    let wr = fortuna_live::daemon::run_weekly_review(
        &mut runner,
        &pool,
        sm.as_ref(),
        &review,
        Some("weather"),
        t0(),
        now,
    )
    .await
    .unwrap();

    assert_eq!(
        wr.calibration.len(),
        1,
        "one calibrated scope (synth_events/weather)"
    );
    assert_eq!(
        wr.calibration[0].n, 50,
        "the calibration audit read all 50 resolved beliefs"
    );
    assert!(
        !wr.recommendations.is_empty(),
        "GO/NO-GO recommendations for the composed strategies (I7: recs only)"
    );
    assert!(
        wr.commentary.is_none(),
        "StubMind => no commentary; the deterministic core stands"
    );
    let audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'alert' AND payload->>'kind' = 'weekly_review'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 1, "the weekly review cycle is audited once");
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn drive_runs_the_weekly_review_at_the_week_boundary(pool: PgPool) {
    // T4.1/M2 slice B2: drive() runs the weekly review on the WEEK boundary (the
    // first WeeklyScheduler.due() fires on boot, like the daily digest). With the
    // review wired, a weekly_review audit lands. NON-VACUOUS: the sibling drives
    // pass reviews=None and audit none — only the wiring fires it. (The review's
    // DATA path is covered by weekly_review_audits_*; this proves the WIRING.)
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    let dcfg = DaemonToml::parse(&format!(
        "{text}\n[synthesis]\nvenue = \"sim\"\ncategory = \"weather\"\n"
    ))
    .unwrap();
    let review = dcfg.review.clone().expect("the example ships [review]");
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        91,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .unwrap();
    arb_books(&runner);

    let before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'alert' AND payload->>'kind' = 'weekly_review'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, 0, "no weekly review before drive");

    let reviews = Some(fortuna_live::daemon::ReviewWiring {
        pool: pool.clone(),
        mind: stub_mind(),
        review,
        synth_category: Some("weather".to_string()),
        start: t0(),
        weekly: fortuna_live::daemon::WeeklyScheduler::new(),
        monthly: fortuna_live::daemon::MonthlyScheduler::new(),
        envelopes: full.envelopes.clone(),
    });

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
        None,    // synthesis_refresh
        None,    // slice-4d: no scalar producer in this smoke
        None,    // reconciliation
        reviews, // M2: weekly review wiring
        None,    // slice-4e: no perp feed in this smoke
        None,    // [personas]: none in this smoke
        None,    // [discovery]: none in this smoke
    )
    .await
    .expect("daemon drive");

    let after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'alert' AND payload->>'kind' = 'weekly_review'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        after, 1,
        "drive() ran the weekly review at the week boundary"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn monthly_review_audits_allocations_cost_and_lesson_demotion(pool: PgPool) {
    // T4.1/M2 slice C1 (spec 5.8 monthly review): a PURE allocation + fee/PnL/
    // cost audit (no mind) + lesson-demotion candidates + the operator checklist
    // (kill-switch test, backup drill) — RECOMMENDATIONS ONLY (I7). Compose +
    // trade once (the digest carries a strategy row), seed an active lesson whose
    // review_at has passed, run the monthly review, and assert: an allocation rec
    // per traded strategy + the overdue lesson is due for demotion + the operator
    // checklist + audited. NON-VACUOUS: the active lesson is read (direct query)
    // and filtered by review_at <= now.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    let dcfg = DaemonToml::parse(&text).unwrap();
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        92,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .unwrap();
    arb_books(&runner);
    runner.tick().await.unwrap();

    sqlx::query(
        "INSERT INTO lessons (lesson_id, body, provenance, status, review_at, created_at)
         VALUES ('01LESSONMONTHLY000000000AB', 'fade longshots harder', '{}'::jsonb, 'active',
                 '2026-06-01T00:00:00.000Z', '2026-05-01T00:00:00.000Z')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let now = fortuna_core::clock::UtcTimestamp::parse_iso8601("2026-07-01T00:00:00.000Z").unwrap();
    let mr = fortuna_live::daemon::run_monthly_review(&mut runner, &pool, &full.envelopes, now)
        .await
        .unwrap();

    assert!(
        !mr.allocations.is_empty(),
        "an allocation recommendation per traded strategy (recs only, I7)"
    );
    assert_eq!(
        mr.lessons_due_demotion.len(),
        1,
        "the active lesson whose review_at passed is due for demotion"
    );
    assert!(
        !mr.operator_checklist.is_empty(),
        "the operator checklist (kill-switch test, backup drill)"
    );
    let audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'alert' AND payload->>'kind' = 'monthly_review'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 1, "the monthly review cycle is audited once");
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn drive_runs_the_monthly_review_at_the_month_boundary(pool: PgPool) {
    // T4.1/M2 slice C2: drive() runs the monthly review on the MONTH boundary
    // (the first MonthlyScheduler.due() fires on boot, like the daily/weekly).
    // With the review wired, a monthly_review audit lands. NON-VACUOUS: the
    // sibling drives pass reviews=None and audit none — only the wiring fires it.
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    let dcfg = DaemonToml::parse(&format!(
        "{text}\n[synthesis]\nvenue = \"sim\"\ncategory = \"weather\"\n"
    ))
    .unwrap();
    let review = dcfg.review.clone().expect("the example ships [review]");
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        93,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .unwrap();
    arb_books(&runner);

    let before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'alert' AND payload->>'kind' = 'monthly_review'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, 0, "no monthly review before drive");

    let reviews = Some(fortuna_live::daemon::ReviewWiring {
        pool: pool.clone(),
        mind: stub_mind(),
        review,
        synth_category: Some("weather".to_string()),
        start: t0(),
        weekly: fortuna_live::daemon::WeeklyScheduler::new(),
        monthly: fortuna_live::daemon::MonthlyScheduler::new(),
        envelopes: full.envelopes.clone(),
    });

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
        None,    // synthesis_refresh
        None,    // slice-4d: no scalar producer in this smoke
        None,    // reconciliation
        reviews, // M2: weekly + monthly review wiring
        None,    // slice-4e: no perp feed in this smoke
        None,    // [personas]: none in this smoke
        None,    // [discovery]: none in this smoke
    )
    .await
    .expect("daemon drive");

    let after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'alert' AND payload->>'kind' = 'monthly_review'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        after, 1,
        "drive() ran the monthly review at the month boundary"
    );
}

/// Track A persona-live-wiring: drive() runs the OPT-IN persona step end-to-end —
/// read a signal -> run_due_personas (scripted StubMind) -> persist one
/// domain_analyses row -> fan out to binary beliefs citing that artifact. Mirrors
/// crates/fortuna-ledger/tests/persona_e2e.rs (the persona pipeline, registration +
/// StubMind findings shape) but exercises it through the LIVE drive() seam (one
/// StopAtCadence segment, like the scalar/synthesis/review drive-tests).
///
/// MUTATION PROOF (verified, not just claimed): passing `personas: None` instead of
/// `Some(wiring)` makes drive() skip the step entirely, so domain_analyses stays at
/// 0 rows and the `after == 1` assertion goes RED — proving the wiring is
/// load-bearing, not incidental. (I confirmed this by temporarily flipping the arg
/// to None, observing the RED, and restoring it to Some.) The sibling drive-tests
/// above all pass `personas: None` and persist zero persona rows, which is the same
/// proof standing.
///
/// Region derivation: the meteorologist's region_key template is
/// `weather:{station}:tmax:{date}`, so the signal PAYLOAD carries `station` +
/// `date` fields; run_due_personas' fill_region_key renders them into
/// `weather:KNYC:tmax:2026-06-12`, a date-bearing region belief_horizon parses
/// (-> 2026-06-12T23:59:59.999Z). The signal `kind` is `aeolus.forecast` (one of
/// the persona's reads_signal_kinds) and `received_at` is the SimClock start (well
/// within the 48h window), so recent_by_kind finds it and the trigger fires.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn drive_persists_persona_analysis_and_beliefs_when_wired(pool: PgPool) {
    use fortuna_cognition::context::content_hash_of;
    use fortuna_cognition::discovery::DiscoveryBudget;
    use fortuna_cognition::persona::{PersonaDef, RegistryHead};
    use fortuna_cognition::persona_orchestrator::{PersonaSchedule, PersonaScheduleState};
    use fortuna_core::market::StrategyId;
    use fortuna_ledger::PersonasRepo;
    use serde_json::json;

    // ---- Boot the daemon from the committed example config. ----
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let dcfg = DaemonToml::parse(&text).expect("example parses");
    let full = FortunaConfig::load_file(example_path).expect("example full-config parses");
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        77,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .expect("composition");
    arb_books(&runner);

    // ---- 1. Register the SHIPPED meteorologist persona, hash-bound (status=active).
    //         Arg list copied from persona_e2e.rs:42-59; method_hash = the loader's
    //         content_hash_of(persona.md) so validate_against (in the wiring) binds.
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/personas/meteorologist"
    );
    let md = std::fs::read_to_string(format!("{dir}/persona.md")).unwrap();
    let schema = std::fs::read_to_string(format!("{dir}/schema.json")).unwrap();
    let def = PersonaDef::parse(&md, &schema).unwrap();
    let method_hash = content_hash_of(&md);
    let personas_repo = PersonasRepo::new(pool.clone());
    personas_repo
        .insert(
            "p-1",
            &def.meta.id,
            def.meta.version,
            &def.meta.domain,
            &json!(def.meta.domain_tags),
            &json!(def.meta.reads_signal_kinds),
            &def.meta.tier,
            &method_hash,
            &def.meta.output_schema_version,
            "active",
            None,
            "2026-06-13T00:00:00.000Z",
            "2026-06-13T00:00:00.000Z",
        )
        .await
        .unwrap();
    // The loader's fail-closed check (the same one the wiring runs in main).
    let head = personas_repo.head(&def.meta.id).await.unwrap().unwrap();
    def.validate_against(Some(&RegistryHead {
        version: head.version,
        method_hash: head.method_hash.clone(),
        status: head.status.clone(),
    }))
    .expect("the shipped file's hash matches the registered row");

    // ---- 2. Insert a signal recent_by_kind will find. kind ∈ reads_signal_kinds;
    //         received_at = SimClock start (within the 48h window); payload carries
    //         the region template fields (station + date) so fill_region_key yields
    //         a date-bearing region belief_horizon can parse.
    let signal_payload = json!({
        "station": "KNYC",
        "date": "2026-06-12",
        "mu": 64.3,
        "sigma": 3.1
    });
    let received_at = t0().to_iso8601(); // 2026-06-11T12:00:00.000Z — within 48h of now
    fortuna_ledger::SignalsRepo::new(pool.clone())
        .insert(
            "sig-aeolus-knyc",
            "aeolus",
            "aeolus.forecast", // one of the meteorologist's reads_signal_kinds
            &received_at,
            &content_hash_of("aeolus-knyc-tmax-2026-06-12"),
            &signal_payload,
        )
        .await
        .unwrap();

    // ---- 3. A scripted StubMind whose journal.body IS the findings JSON (so
    //         produced_artifact() is true). Findings shape copied from
    //         persona_e2e.rs:80-89 (3 thresholds -> 3 binary beliefs).
    let findings = json!({
        "thresholds": [{"ge": 60, "p": 0.92}, {"ge": 65, "p": 0.41}, {"ge": 70, "p": 0.08}],
        "sigma_trend": "tightening",
        "confidence": "high",
        "regime": "stagnant upper ridge",
        "key_risk": "onshore flow backdoor front near 21Z"
    });
    let scripted: MindOutput = serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": findings.to_string()},
        "cost_cents": 1
    }))
    .unwrap();
    let persona_mind: Arc<dyn Mind> = Arc::new(StubMind::scripted(vec![scripted]));

    // ---- 4. Build the PersonasWiring with ONE loaded schedule (the shipped files),
    //         cadences=[] so it triggers on the in-window signal (not a cadence).
    let wiring = fortuna_live::daemon::PersonasWiring {
        pool: pool.clone(),
        schedules: vec![PersonaSchedule {
            def,
            cadences: Vec::new(),
        }],
        state: PersonaScheduleState::new(0),
        budget: DiscoveryBudget::new(500),
        mind: persona_mind,
        strategy: StrategyId::new("domain-analysis").unwrap(), // TEST code: unwrap fine
        window_hours: 48,
        max_signals: 200,
    };

    // No persona rows before the drive.
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_analyses")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, 0, "no domain analyses before drive");

    // ---- 5. Run ONE drive() segment with personas: Some(wiring); None for the
    //         other opt-in params (StopAtCadence one-segment harness, as the
    //         scalar/synthesis/review drive-tests).
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
        None, // synthesis_refresh
        None, // slice-4d: no scalar producer
        None, // reconciliation
        None, // reviews
        None, // slice-4e: no perp feed
        // MUTATION PROOF: flip this to `None` and domain_analyses stays 0 => RED.
        Some(wiring), // [personas]: the wiring under test
        None,         // [discovery]: none in this persona e2e
    )
    .await
    .expect("daemon drive");

    // ---- 6. Assert: exactly one domain_analyses row for the meteorologist. ----
    let analyses: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM domain_analyses WHERE persona_id = 'meteorologist'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        analyses, 1,
        "drive() persisted exactly one persona analysis when wired"
    );

    // The analysis_id the wiring minted (01PAN-prefixed), to cross-check beliefs.
    let analysis_id: String = sqlx::query_scalar(
        "SELECT analysis_id FROM domain_analyses WHERE persona_id = 'meteorologist'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // ---- and beliefs fanned out, each citing the persisted artifact by provenance.
    let beliefs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM beliefs WHERE provenance->>'persona_id' = 'meteorologist'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    // The scripted findings carry 3 thresholds, so map_persona_analysis fans out
    // EXACTLY 3 binary beliefs (cf. persona_e2e.rs which asserts drafts.len()==3).
    assert_eq!(
        beliefs, 3,
        "the 3-threshold analysis fanned out to exactly 3 beliefs (got {beliefs})"
    );
    let matching_analysis: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM beliefs \
         WHERE provenance->>'persona_id' = 'meteorologist' \
           AND provenance->>'analysis_id' = $1",
    )
    .bind(&analysis_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        matching_analysis, beliefs,
        "every persona belief cites the persisted analysis_id (replay anchor holds)"
    );
}

/// COMMIT 1 (spec 5.12, world-forward): drive() runs the OPT-IN discovery step
/// end-to-end — read a fresh signal -> world_forward_discovery (scripted StubMind)
/// -> persist each candidate as a `watch:` event -> fan the SCOREABLE candidates
/// out to binary beliefs. Mirrors the persona drive-e2e harness (one StopAtCadence
/// segment) and the WatchlistBatch JSON shape from
/// crates/fortuna-cognition/tests/discovery.rs (the `watchlist_body()` helper).
///
/// The unscoreable rule (the doctrine under test): the scripted batch carries TWO
/// candidates — one whose `resolution_source = "nws"` (an enabled registry row, so
/// SCOREABLE) and one whose source is outside the registry (UNSCOREABLE). The mind
/// attaches a belief only to the scoreable one. So exactly ONE belief survives, and
/// both candidates persist as events (the unscoreable one is recorded but carries no
/// belief — "no beliefs nobody can grade").
///
/// MUTATION PROOF (verified, not just claimed): passing `discovery: None` instead of
/// `Some(wiring)` makes drive() skip the step entirely, so zero `watch:` events and
/// zero beliefs persist and the `>= 1` assertions go RED — proving the wiring is
/// load-bearing. (I confirmed this by temporarily flipping the arg to None,
/// observing the RED, and restoring it to Some.) The sibling drive-tests above all
/// pass `discovery: None` and persist zero watch events, the same proof standing.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn discovery_world_forward_persists_watchlist_events_and_beliefs(pool: PgPool) {
    use fortuna_cognition::context::content_hash_of;
    use fortuna_cognition::discovery::DiscoveryBudget;
    use fortuna_cognition::signals::{SourceEntry, SourceRegistry, TrustTier};
    use fortuna_core::market::StrategyId;
    use serde_json::json;

    // ---- Boot the daemon from the committed example config (operators copy it). ----
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let dcfg = DaemonToml::parse(&text).expect("example parses");
    let full = FortunaConfig::load_file(example_path).expect("example full-config parses");
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        88,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .expect("composition");
    arb_books(&runner);

    // ---- 1. Seed a SCOREABLE source ("nws") in the registry so the candidate that
    //         declares it is scoreable (and gets a belief). trust_tier 8 mirrors the
    //         discovery library test's registry().
    let now_iso = t0().to_iso8601();
    fortuna_ledger::SourceRegistryRepo::new(pool.clone())
        .upsert("nws", 8, &["weather".to_string()], true, &now_iso)
        .await
        .unwrap();

    // ---- 2. Insert a signal of a kind in [discovery].signal_kinds, within the 48h
    //         window. world_forward_discovery assembles it into a <context-item>; the
    //         signal is untrusted DATA (the scripted mind ignores it, but the read +
    //         assemble path is exercised exactly as in production).
    let signal_payload = json!({"headline": "record heat forecast for NYC", "station": "KNYC"});
    fortuna_ledger::SignalsRepo::new(pool.clone())
        .insert(
            "sig-disc-1",
            "aeolus",
            "aeolus.forecast", // in the discovery signal_kinds below
            &now_iso,
            &content_hash_of("disc-heat-2026-06-11"),
            &signal_payload,
        )
        .await
        .unwrap();

    // ---- 3. Script the StubMind: a WatchlistBatch with TWO candidates (one
    //         scoreable via "nws", one unscoreable via a source outside the registry)
    //         and a belief on the SCOREABLE candidate only. JSON shape copied from
    //         crates/fortuna-cognition/tests/discovery.rs::watchlist_body().
    let scripted: MindOutput = serde_json::from_value(json!({
        "beliefs": [{
            "event_id": "watch:heat-dome-2026-06",
            "p": 0.3,
            "p_raw": 0.3,
            "horizon": "2026-06-25T00:00:00.000Z",
            "evidence": [{"source": "nws", "ref": "sig-disc-1"}]
        }],
        "proposals": [],
        "journal": {"body": json!({
            "candidates": [
                {
                    "event_hint": "heat-dome-2026-06",
                    "statement": "A heat dome produces 3+ consecutive 95F days in NYC in June 2026",
                    "resolution_criteria": "NWS Central Park daily climate reports",
                    "resolution_source": "nws",
                    "horizon": "2026-06-25T00:00:00.000Z",
                    "category": "weather"
                },
                {
                    "event_hint": "alien-disclosure",
                    "statement": "A government discloses alien contact in 2026",
                    "resolution_criteria": "vibes",
                    "resolution_source": "my-cool-blog",
                    "horizon": "2026-12-31T00:00:00.000Z",
                    "category": "politics"
                }
            ]
        }).to_string()},
        "cost_cents": 1
    }))
    .unwrap();
    let discovery_mind: Arc<dyn Mind> = Arc::new(StubMind::scripted(vec![scripted]));

    // ---- 4. Build the DiscoveryWiring. The registry is loaded from the seeded
    //         table (the same load_all path main.rs uses), so "nws" is present +
    //         enabled. budget 500 (room to spend); strategy "world-forward".
    let rows = fortuna_ledger::SourceRegistryRepo::new(pool.clone())
        .load_all()
        .await
        .unwrap();
    let mut registry = SourceRegistry::new();
    for r in rows {
        registry.upsert(SourceEntry {
            source_id: r.source_id,
            trust_tier: TrustTier::new(u8::try_from(r.trust_tier).unwrap()).unwrap(),
            domain_tags: r.domain_tags,
            enabled: r.enabled,
        });
    }
    let wiring = fortuna_live::daemon::DiscoveryWiring {
        pool: pool.clone(),
        mind: discovery_mind,
        budget: DiscoveryBudget::new(500),
        registry,
        strategy: StrategyId::new("world-forward").unwrap(), // TEST code: unwrap fine
        signal_kinds: vec!["aeolus.forecast".to_string()],
        window_hours: 48,
        max_signals: 200,
        // COMMIT 2 market-back fields: an EMPTY catalog makes the market-back step
        // inert (this test exercises ONLY the world-forward arm), so the prefilter
        // and id bases are unused here.
        prefilter: fortuna_cognition::discovery::PrefilterConfig {
            category_allowlist: vec![],
            min_volume_contracts: 0,
            min_category_quality: 0.0,
            category_quality: std::collections::BTreeMap::new(),
        },
        catalog: vec![],
        event_id_base: 0,
        edge_id_base: 0,
    };

    // No watch events before the drive.
    let before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE event_id LIKE 'watch:%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before, 0, "no watch events before drive");

    // ---- 5. Run ONE drive() segment with discovery: Some(wiring); None for the
    //         other opt-in params (StopAtCadence one-segment harness).
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
        None, // synthesis_refresh
        None, // slice-4d: no scalar producer
        None, // reconciliation
        None, // reviews
        None, // slice-4e: no perp feed
        None, // [personas]: none in this discovery e2e
        // MUTATION PROOF: flip this to `None` and watch events + beliefs stay 0 => RED.
        Some(wiring), // [discovery]: the wiring under test
    )
    .await
    .expect("daemon drive");

    // ---- 6. Assert: BOTH candidates persisted as `watch:` events (the unscoreable
    //         one is recorded too — only its BELIEFS are refused). >= 1 is the
    //         load-bearing claim; exactly 2 is the unscoreable-rule detail.
    let watch_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE event_id LIKE 'watch:%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        watch_events >= 1,
        "drive() persisted at least one watch: event when wired (got {watch_events})"
    );
    assert_eq!(
        watch_events, 2,
        "both candidates (scoreable + unscoreable) persist as events (got {watch_events})"
    );

    // The SCOREABLE candidate's belief fanned out (and only it — the unscoreable
    // candidate's belief is refused: no beliefs nobody can grade).
    let watch_beliefs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM beliefs WHERE event_id LIKE 'watch:%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        watch_beliefs >= 1,
        "drive() fanned out at least one belief on a watch: event when wired (got {watch_beliefs})"
    );
    assert_eq!(
        watch_beliefs, 1,
        "only the SCOREABLE candidate carries a belief (the unscoreable one is refused)"
    );
    // The surviving belief is the scoreable candidate's, by event_id.
    let scoreable_belief: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM beliefs WHERE event_id = 'watch:heat-dome-2026-06'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        scoreable_belief, 1,
        "the belief rides the scoreable watch event (watch:heat-dome-2026-06)"
    );
}

/// COMMIT 2 (spec 5.12 market-back; the amendment's gate): the FULL early-arrival
/// chain end-to-end — catalog -> deterministic prefilter -> mind normalizes a
/// survivor into a NEW canonical event -> a LOW-STAKES edge (Direct mapping +
/// deterministic_score 1.0) is AUTO-CONFIRMED (`confirmed_by = 'discovery:auto'`,
/// spec 5.12:252) -> the SAME-SEGMENT synthesis edge-refresh picks it up -> the
/// synthesis arm believes on that minted event and DRAFTS a belief that drains +
/// persists. This stitches the COMMIT-1 world-forward e2e harness (one StopAtCadence
/// segment) to `drive_drains_and_persists_the_synthesis_arms_beliefs` (the synthesis
/// drain) and `per_segment_refresh_picks_up_a_newly_confirmed_edge` (the ledger is
/// the boundary, re-read per segment) — only here the edge is minted by discovery
/// inside the SAME run, not pre-seeded.
///
/// DETERMINISTIC EVENT ID: the market-back block mints the first NEW event as
/// `format!("01EVT{:021}", event_id_base)` where `event_id_base` is seeded (in the
/// wiring) from the drive-start epoch. Under SimClock the drive-start is `t0()` =
/// 2026-06-11T12:00:00.000Z = epoch_ms 1781179200000, so the minted id is the
/// constant below — which the synthesis `believing_mind` targets, closing the loop
/// on a RUNTIME-minted id (no fallback needed).
///
/// MUTATION PROOF (verified, not just claimed): passing `discovery: None` makes
/// drive() skip the market-back step entirely, so NO event + NO edge is minted, the
/// synthesis arm has no edge to believe against, and ALL THREE counts (events, the
/// auto-confirmed edge, the synthesis belief on the minted event) go to 0 => RED. (I
/// confirmed this by temporarily flipping the arg to None, observing the RED across
/// all three assertions, and restoring it to Some.)
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn discovery_market_back_auto_confirms_and_synthesis_drafts_a_belief(pool: PgPool) {
    use fortuna_cognition::discovery::{DiscoveryBudget, MarketView, PrefilterConfig};
    use fortuna_cognition::signals::SourceRegistry;
    use fortuna_core::clock::UtcTimestamp;
    use fortuna_core::market::StrategyId;
    use serde_json::json;
    use std::collections::BTreeMap;

    // The deterministic minted event id (see the doc comment): 01EVT + the
    // zero-padded(21) t0() epoch_millis (1781179200000). The market-back block
    // mints the first NEW event at this id; the synthesis believing_mind targets it.
    const MINTED_EVENT_ID: &str = "01EVT000000001781179200000";
    let t0_epoch_ms: u64 = t0().epoch_millis().max(0) as u64;
    let horizon = "2026-06-20T18:00:00.000Z";

    // ---- Boot WITH [synthesis] scoped to sim (so the synthesis arm composes), but
    //      NO pre-seeded event/edge — the discovery block creates them. ----
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let dcfg = DaemonToml::parse(&format!("{text}\n[synthesis]\nvenue = \"sim\"\n")).unwrap();
    let full = FortunaConfig::load_file(example_path).expect("example full-config parses");
    // The SYNTHESIS mind believes on the minted event id (the chain's far end).
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        t0(),
        99,
        believing_mind(MINTED_EVENT_ID, 0.70),
        TriageDecision::AlwaysAccept,
    )
    .await
    .expect("composition");
    assert_eq!(
        runner.synthesis_edge_count(),
        Some(0),
        "booted with no confirmed edges (the discovery block mints the edge)"
    );
    // A book for SIM-BKT-LO (the catalog market) so the synth arm's cycle runs once
    // the edge is confirmed; the other sim markets get books too (arb harness).
    arb_books(&runner);

    // ---- 1. The catalog: ONE "weather" MarketView for SIM-BKT-LO (a real sim
    //         market with a book), volume above the floor, resolution_source "nws"
    //         and close_at == the normalization horizon so the deterministic edge
    //         check scores 1.0 => high_stakes == false => AUTO-CONFIRM. ----
    let catalog = vec![MarketView {
        market_id: "SIM-BKT-LO".to_string(),
        venue: "sim".to_string(),
        title: "SIM weather bracket LO".to_string(),
        category: "weather".to_string(),
        volume_contracts: Some(5_000),
        resolution_source: "nws".to_string(),
        close_at: Some(UtcTimestamp::parse_iso8601(horizon).unwrap()),
    }];

    // ---- 2. Prefilter allowing "weather" with a calibration record that clears the
    //         floor (the per-category quality map must list "weather" — an absent
    //         category scores 0.0 and would be excluded). ----
    let prefilter = PrefilterConfig {
        category_allowlist: vec!["weather".to_string()],
        min_volume_contracts: 100,
        min_category_quality: 0.1,
        category_quality: BTreeMap::from([("weather".to_string(), 0.9)]),
    };

    // ---- 3. The discovery mind: a NormalizationBatch with ONE entry for
    //         SIM-BKT-LO — matches_event_id null (NEW event), Direct mapping,
    //         resolution_source "nws" + horizon == close_at => deterministic 1.0 =>
    //         high_stakes false => auto-confirm. JSON shape copied from
    //         crates/fortuna-cognition/tests/discovery.rs::normalization_body(). ----
    let scripted: MindOutput = serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": json!({
            "normalizations": [
                {
                    "market_id": "SIM-BKT-LO",
                    "matches_event_id": null,
                    "statement": "NYC high temp >= 90F on 2026-06-20",
                    "resolution_criteria": "NWS Central Park daily climate report",
                    "resolution_source": "nws",
                    "horizon": horizon,
                    "category": "weather",
                    "mapping": "direct",
                    "confidence": 0.85
                }
            ]
        }).to_string()},
        "cost_cents": 1
    }))
    .unwrap();
    let discovery_mind: Arc<dyn Mind> = Arc::new(StubMind::scripted(vec![scripted]));

    // ---- 4. The DiscoveryWiring. signal_kinds = [] is fine (market-back's driver is
    //         the catalog, not signals). event_id_base/edge_id_base seeded from the
    //         drive-start epoch (== t0() under SimClock), so the first minted event
    //         is MINTED_EVENT_ID. registry empty (market-back does not use it). ----
    let wiring = fortuna_live::daemon::DiscoveryWiring {
        pool: pool.clone(),
        mind: discovery_mind,
        budget: DiscoveryBudget::new(500),
        registry: SourceRegistry::new(),
        strategy: StrategyId::new("market-back").unwrap(), // TEST code: unwrap fine
        signal_kinds: vec![],
        window_hours: 48,
        max_signals: 200,
        prefilter,
        catalog,
        event_id_base: t0_epoch_ms,
        edge_id_base: t0_epoch_ms,
    };

    // Nothing before the drive: no canonical events, no edges, no beliefs.
    let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(events_before, 0, "no events before drive");

    // ---- 5. Run drive() with discovery: Some(wiring) AND synthesis_refresh: Some.
    //         fire_at 6 / segment 4 == the synthesis-drain template: segment 1's
    //         post-block mints+confirms the edge, segment 2's ticks draft the belief
    //         and its post-block drains+persists it (then the stop breaks the loop). --
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
        synthesis_refresh, // the synthesis arm prices the auto-confirmed edge
        None,              // slice-4d: no scalar producer
        None,              // reconciliation
        None,              // reviews
        None,              // slice-4e: no perp feed
        None,              // [personas]: none in this discovery e2e
        // MUTATION PROOF: flip this to `None` and the event + edge + synthesis belief
        // all stay 0 => RED (no discovery means no edge to believe against).
        Some(wiring),
    )
    .await
    .expect("daemon drive");

    // ---- 6. Assert the full chain. ----
    // (a) >= 1 canonical event minted by discovery (the NEW-event draft persisted).
    let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        events_after >= 1,
        "discovery minted at least one canonical event (got {events_after})"
    );
    let minted_event: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE event_id = $1")
        .bind(MINTED_EVENT_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        minted_event, 1,
        "the minted event is at the deterministic id {MINTED_EVENT_ID}"
    );

    // (b) >= 1 market_event_edges row AUTO-CONFIRMED by discovery (the spec 5.12:252
    //     low-stakes auto-confirm). The edge points SIM-BKT-LO -> the minted event.
    let auto_edges: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM market_event_edges WHERE confirmed_by = 'discovery:auto'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        auto_edges >= 1,
        "discovery auto-confirmed at least one low-stakes edge (got {auto_edges})"
    );
    let minted_edge: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM market_event_edges \
         WHERE market_id = 'SIM-BKT-LO' AND event_id = $1 \
           AND confirmed_by = 'discovery:auto' AND mapping_type = 'direct'",
    )
    .bind(MINTED_EVENT_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        minted_edge, 1,
        "the auto-confirmed edge is SIM-BKT-LO -> {MINTED_EVENT_ID} (direct)"
    );

    // (c) >= 1 belief from the synthesis arm on the discovered event — the gate: the
    //     auto-confirmed edge was refreshed into the synth arm SAME-SEGMENT and the
    //     believing_mind drafted a belief on it that drained + persisted.
    let synthesis_belief: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM beliefs WHERE event_id = $1")
            .bind(MINTED_EVENT_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        synthesis_belief >= 1,
        "the synthesis arm believed on the discovered event (got {synthesis_belief}) — \
         the catalog->event->auto-confirmed-edge->synthesis-belief chain is load-bearing"
    );
}
