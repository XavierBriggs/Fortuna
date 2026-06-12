//! T4.1 hard requirement 10: the daemon-composition DST smoke — boot ->
//! ticks -> stop-signal -> graceful shutdown, deterministic under
//! SimClock, against the COMMITTED example config (operators copy it; if
//! it cannot boot the daemon, it is broken). The stop channel fired here
//! is the SAME channel main's SIGTERM handler fires: the smoke asserts
//! the signal path end-to-end minus the OS delivery (operator amendment:
//! SIGTERM -> cancel working orders + final audit row).

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

    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 42, stub_mind())
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
        None, // M2: no reconciliation in this smoke
        None, // M2: no reviews in this smoke
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
    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 7, stub_mind())
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
        None, // M2: no reconciliation in this smoke
        None, // M2: no reviews in this smoke
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
    let runner = compose_runner(pool.clone(), &full, &dcfg_with, t0(), 1, stub_mind())
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
    let runner2 = compose_runner(pool, &full, &dcfg_without, t0(), 2, stub_mind())
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

    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 9, stub_mind())
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
        None, // M2: no reconciliation in this smoke
        None, // M2: no reviews in this smoke
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

    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 901, stub_mind())
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
        None, // M2: no reconciliation in this smoke
        None, // M2: no reviews in this smoke
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
    let runner = compose_runner(pool.clone(), &full, &dcfg_with, t0(), 1, stub_mind())
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
    let runner2 = compose_runner(pool, &full, &dcfg_without, t0(), 2, stub_mind())
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
        None, // M2: no reconciliation in this smoke
        None, // M2: no reviews in this smoke
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
    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 70, stub_mind())
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
    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 71, stub_mind())
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
    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 80, stub_mind())
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
        Some((pool.clone(), journaling_mind("EOD: flat; tomorrow hold."))), // reconciliation
        None, // M2: no reviews in this e2e
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
    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 90, stub_mind())
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
    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 91, stub_mind())
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
        None,    // reconciliation
        reviews, // M2: weekly review wiring
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
    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 92, stub_mind())
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
    let mut runner = compose_runner(pool.clone(), &full, &dcfg, t0(), 93, stub_mind())
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
        None,    // reconciliation
        reviews, // M2: weekly + monthly review wiring
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
