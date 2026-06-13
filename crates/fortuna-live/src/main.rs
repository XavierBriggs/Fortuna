//! fortuna-live — the T4.1 daemon binary. The ONLY place in the crate
//! that touches the real world: process env (loaded EXPLICITLY from
//! .env per the kickoff corollary — an inherited DATABASE_URL alone is
//! never trusted; the other secrets gate boot), wall-clock start time,
//! OS signals, and sockets. Everything below main composes through the
//! library halves, which are deterministic and test-injected.
//!
//! SIGTERM CONTRACT (operator-BINDING, BUILD_PLAN T4.1): SIGTERM and
//! SIGINT both feed ONE stop channel; the run loop exits on it and the
//! daemon runs `SimRunner::shutdown` — cancel working orders + the final
//! audit row — before the process ends. The smoke asserts the identical
//! path by firing the same channel.

use anyhow::{bail, Context, Result};
use fortuna_cognition::mind::ReqwestMindTransport;
use fortuna_core::clock::{Clock, RealClock};
use fortuna_live::boot::{validate_env, DaemonToml};
use fortuna_live::compose::DegradeScrape;
use fortuna_live::daemon::{
    compose_runner, default_degrade_thresholds, drive, mind_from_env, registry_from, PgHaltPoller,
    SYNTH_MIND_TIMEOUT_SECS,
};
use fortuna_live::run_loop::{LoopConfig, RealCadence};
use fortuna_ops::dashboard::{serve_dashboard, DashboardSnapshot};
use fortuna_ops::FortunaConfig;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/fortuna.toml".to_string());

    // Explicit .env load (absent file is fine — systemd EnvironmentFile
    // case; validate_env still gates every required var).
    let _ = dotenvy::from_path(".env");

    let config_text = std::fs::read_to_string(&config_path).with_context(|| {
        format!("reading config at {config_path} (copy config/fortuna.example.toml)")
    })?;
    let dcfg = DaemonToml::parse(&config_text).context("config rejected")?;
    dcfg.validate_bootable().context("boot check failed")?;
    let full = FortunaConfig::load_file(&config_path).context("full config rejected")?;

    let env: BTreeMap<String, String> = std::env::vars().collect();
    let validated = validate_env(&env).context(
        "environment rejected (set -a && source .env && set +a, or a systemd EnvironmentFile)",
    )?;
    if validated.anthropic_api_key.is_none() && !dcfg.cognition.allow_stub_mind {
        bail!(
            "ANTHROPIC_API_KEY is absent and [cognition] allow_stub_mind = false: \
             booting would silently run the stub mind. Set the key or opt into the \
             stub explicitly."
        );
    }

    // Wall time enters here (binary edge) and at the cadence driver only,
    // ALWAYS through RealClock — the single Clock impl permitted to read
    // the wall (clock.rs), never a raw SystemTime::now() (CLAUDE.md: a
    // wall read outside a Clock impl is a defect even at the edge).
    let start = RealClock.now();
    let start_ms = start.epoch_millis();

    let pool = fortuna_ledger::connect(validated.database_url.expose())
        .await
        .context("postgres connect + migrate")?;

    // S5b: build the synthesis mind from the environment. ANTHROPIC_API_KEY
    // present (validated; the boot gate above refused no-key + !allow_stub) =>
    // the Claude-backed AnthropicMind over the real reqwest transport; absent =>
    // the inert StubMind. The key reaches ONLY the transport (read from env by
    // from_env), never config or logs. The mind's cost-budget day boundary runs
    // on RealClock — the real-time daemon's SimClock tracks wall time, so they
    // align. The S5a config gap is CLOSED (304f746): the example config carries
    // the `synthesis_cents` envelope + `[gates.per_strategy.synthesis]`, so a
    // keyed mind + the opt-in [synthesis] section trades; the default StubMind
    // (no key) proposes nothing.
    let synthesis_transport = match validated.anthropic_api_key.as_ref() {
        Some(_) => Some(
            ReqwestMindTransport::from_env(std::time::Duration::from_secs(SYNTH_MIND_TIMEOUT_SECS))
                .context("anthropic synthesis transport")?,
        ),
        None => None,
    };
    let synthesis_mind = mind_from_env(&dcfg.cognition, synthesis_transport, Arc::new(RealClock));
    if validated.anthropic_api_key.is_some() {
        eprintln!("fortuna-live: synthesis mind = AnthropicMind (live; model from [cognition])");
    } else {
        eprintln!("fortuna-live: synthesis mind = StubMind (no ANTHROPIC_API_KEY; inert)");
    }
    let mut runner = compose_runner(
        pool.clone(),
        &full,
        &dcfg,
        start,
        start_ms as u64,
        synthesis_mind.clone(),
    )
    .await
    .context("composition")?;
    eprintln!("fortuna-live: composed (venue=sim, markets from [sim], journal+audit in Postgres)");

    // Metrics endpoint (GET-only; bind from config — localhost default).
    let snapshot = Arc::new(RwLock::new(DashboardSnapshot {
        generated_at: start.to_iso8601(),
        stage: "sim".to_string(),
        metrics_text: String::new(),
        boards: serde_json::json!({}),
        views: serde_json::json!({}),
    }));
    let listener = tokio::net::TcpListener::bind(&dcfg.daemon.metrics_bind)
        .await
        .with_context(|| format!("binding metrics endpoint {}", dcfg.daemon.metrics_bind))?;
    eprintln!(
        "fortuna-live: metrics at http://{}",
        dcfg.daemon.metrics_bind
    );
    // R5: a DEDICATED, isolated read pool for ROTA's audit tail — NEVER the
    // writer's. Audit-append failure is a global halt ("no audit, no trading"),
    // so dashboard load must be unable to queue against the audit writer's
    // connections. A connect failure degrades the audit panel to empty (the
    // operator keeps the snapshot views); it never crashes the daemon.
    let rota_pool = fortuna_ledger::connect_readonly_pool(validated.database_url.expose())
        .await
        .ok();
    if rota_pool.is_none() {
        eprintln!("fortuna-live: ROTA read pool unavailable — audit tail degrades to empty");
    }
    let dash_state = snapshot.clone();
    tokio::spawn(async move {
        // ROTA mounts alongside the legacy boards off the same snapshot
        // (T4.3). perishable_dir = the recorder's output base ("data/perishable",
        // matching fortuna-recorder's default --out-dir) so the /streams panel
        // shows recorder liveness; reviews_dir = the verifier's gate-record dir
        // ("docs/reviews") for the /build gate-verdict badge on the LOCAL
        // operator console. Either absent dir degrades to "unknown"/empty,
        // never a 500.
        let rota = fortuna_ops::rota::RotaState {
            snapshot: dash_state,
            pool: rota_pool,
            perishable_dir: Some(Arc::new(std::path::PathBuf::from("data/perishable"))),
            reviews_dir: Some(Arc::new(std::path::PathBuf::from("docs/reviews"))),
        };
        if let Err(e) = serve_dashboard(listener, rota).await {
            eprintln!("fortuna-live: metrics endpoint died: {e}");
        }
    });

    // SIGTERM == SIGINT == graceful shutdown: one channel, one path.
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("fortuna-live: SIGTERM handler failed to install: {e}");
                    return;
                }
            };
        tokio::select! {
            _ = term.recv() => eprintln!("fortuna-live: SIGTERM"),
            _ = tokio::signal::ctrl_c() => eprintln!("fortuna-live: SIGINT"),
        }
        let _ = stop_tx.send(());
    });

    // Dead-man heartbeat: an INDEPENDENT spawned task (it must keep
    // beating even if a trading segment stalls). It arms the external
    // monitor on its first ping — so it runs ONLY in this real binary,
    // NEVER in tests (daemon::deadman_tick is tested with a mock
    // transport). A ping failure is logged here; the monitor itself is
    // the escalation of record (silence => it pages the operator).
    {
        let mut pinger = fortuna_ops::DeadmanPinger::new(
            validated.deadman_url.expose().to_string(),
            &full.deadman,
            Box::new(fortuna_ops::ReqwestPing::new().context("deadman transport")?),
        )
        .context("deadman pinger")?;
        let interval = std::time::Duration::from_secs(full.deadman.ping_interval_secs.max(1));
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                // RealClock is the daemon's one wall-time source; the
                // dead-man deliberately reads the WALL (not the runner's
                // SimClock) so its heartbeat is real-time even in sim soak.
                let now = RealClock.now();
                fortuna_live::daemon::deadman_tick(&mut pinger, now, |e| {
                    eprintln!("fortuna-live: dead-man ping FAILED: {e}");
                })
                .await;
            }
        });
        eprintln!("fortuna-live: dead-man heartbeat armed (pings the monitor every interval)");
    }

    let mut cadence = RealCadence {
        clock: runner.clock.clone(),
    };
    // S4: when [synthesis] is configured, hand drive() the pool + its filters
    // so the loop re-loads the confirmed edge set per segment (req 2). Built
    // BEFORE `pool` moves into the halt poller. Absent [synthesis] => None =>
    // the loop never reloads (a mechanically-only daemon).
    let synthesis_refresh = dcfg.synthesis.clone().map(|syn| (pool.clone(), syn));
    // T4.1/M2 (spec 5.8): the daily reconciliation reuses the SAME synthesis
    // mind (one build, not a second) — a stub mind self-skips. Built BEFORE
    // `pool` moves into the halt poller below.
    let reconciliation = Some((pool.clone(), synthesis_mind.clone()));
    // T4.1/M2 slice B2: the opt-in [review] weekly-review wiring, reusing the
    // synthesis mind + [synthesis].category + the boot time (paper_days). Absent
    // [review] => None => no weekly review fires. Built BEFORE `pool` moves.
    let reviews = dcfg
        .review
        .clone()
        .map(|review| fortuna_live::daemon::ReviewWiring {
            pool: pool.clone(),
            mind: synthesis_mind,
            review,
            synth_category: dcfg.synthesis.as_ref().and_then(|s| s.category.clone()),
            start,
            weekly: fortuna_live::daemon::WeeklyScheduler::new(),
            monthly: fortuna_live::daemon::MonthlyScheduler::new(),
            envelopes: full.envelopes.clone(),
        });
    // D10: clone the pool for the ingestion loop BEFORE it moves into the halt
    // poller (only when [ingestion] is enabled — otherwise no clone, no loop).
    let ingest_pool = if dcfg.ingestion.as_ref().is_some_and(|s| s.enabled) {
        Some(pool.clone())
    } else {
        None
    };
    let mut poller = PgHaltPoller::new(pool);
    let loop_cfg = LoopConfig {
        tick_interval_ms: dcfg.daemon.tick_interval_ms,
        halt_poll_ms: dcfg.daemon.halt_poll_ms,
    };
    let mut scrape = DegradeScrape::new(default_degrade_thresholds());
    let mut daily = fortuna_live::daemon::DailyScheduler::new();

    // Slack router from the validated env over the real reqwest transport.
    // The bot token validated present => Some(router); a channel id the
    // config names but env lacks is a LOUD boot error (validate_env
    // already required all five, so this only fails on a config/env
    // mismatch). Routed alerts each write an audit row (spec 8).
    let slack_router = fortuna_live::daemon::build_slack_router(
        &full.slack,
        Some(validated.slack_bot_token.expose()),
        validated.slack_channels.clone(),
        Box::new(fortuna_ops::ReqwestTransport::new().context("slack transport")?),
    )
    .context("slack router")?;
    if slack_router.is_some() {
        eprintln!("fortuna-live: Slack routing active (alerts -> #fortuna-alerts/#fortuna-ops)");
    }

    // D10: spawn the news-aggregation ingestion loop alongside the trading loop,
    // behind [ingestion].enabled (default OFF => the daemon is byte-unchanged).
    // The Layer-1 validator runs LIVE on the ingest path here; this loop is
    // independent of the deterministic trading cycle and stops with the daemon.
    // OBS-2b: the published telemetry snapshot ("one writer, many readers"). The
    // loop is the writer; a reader clone goes to the ROTA/metrics handlers once
    // track B wires it into RotaState (the live ingestion board). Created
    // unconditionally (an empty Arc is inert when ingestion is OFF — the daemon
    // stays byte-unchanged); only the spawned loop ever writes it.
    let ingest_telemetry = fortuna_live::ingestion::new_telemetry_handle();
    let (ingest_stop, ingest_handle) = match (dcfg.ingestion.as_ref(), ingest_pool) {
        (Some(sec), Some(ipool)) if sec.enabled => {
            let wiring = fortuna_live::ingestion::build_ingestion_wiring(
                &config_text,
                sec,
                ipool,
                runner.clock.clone(),
            )
            .await
            .context("ingestion wiring")?;
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            let tick = std::time::Duration::from_millis(sec.tick_ms);
            let clk = runner.clock.clone();
            let telemetry_writer = ingest_telemetry.clone();
            eprintln!("fortuna-live: news-aggregation ingestion ACTIVE (validator live on the ingest path)");
            (
                Some(tx),
                Some(tokio::spawn(fortuna_live::ingestion::run_ingestion_loop(
                    wiring,
                    clk,
                    tick,
                    rx,
                    telemetry_writer,
                ))),
            )
        }
        _ => (None, None),
    };

    let snapshot_for_segments = snapshot.clone();
    // OBS-2c: a read handle to the live ingestion telemetry, merged into the ROTA
    // snapshot each segment so the V1/V2/V3 ingestion boards render LIVE daemon
    // data. Inert/degraded when ingestion is off (merge_ingest_views gates on an
    // empty telemetry — the daemon snapshot is byte-unchanged in that case).
    let ingest_telemetry_for_segments = ingest_telemetry.clone();
    let (stats, shutdown) = drive(
        &mut runner,
        &mut cadence,
        &mut poller,
        &loop_cfg,
        60, // segment = 60 wakes (~30s at the 500ms poll): metrics refresh cadence
        &mut stop_rx,
        move |r, _seg| {
            // Build everything BEFORE taking the write lock (R8: minimise
            // time the snapshot is held). T4.3 ROTA slice 2: the daemon
            // shapes the per-view JSON the rota handlers serve verbatim
            // (R2 — fortuna-ops never depends on the runner).
            let generated_at = fortuna_core::clock::Clock::now(r.clock.as_ref()).to_iso8601();
            let registry = registry_from(r);
            let metrics_text = registry.render_prometheus();
            let boards = r.boards_json();
            let mut views = fortuna_live::views::views_from(r, &generated_at);
            // Mission item 6: the telemetry pane — the same MetricsRegistry the
            // /metrics exposition is rendered from, shaped into a ROTA board (R2: the
            // daemon shapes; fortuna-ops serves it via read_view, never parsing text).
            views["telemetry"] = registry.telemetry_board(&generated_at);
            // OBS-2c: non-blocking read of the published telemetry (the closure is
            // sync; the ingestion loop holds the write lock only momentarily). On
            // contention or pre-first-tick the ingest boards stay degraded this
            // segment — identical to the snapshot try_write contract below.
            if let Ok(tel) = ingest_telemetry_for_segments.try_read() {
                fortuna_live::views::merge_ingest_views(&mut views, &tel, &generated_at);
            }
            if let Ok(mut snap) = snapshot_for_segments.try_write() {
                snap.generated_at = generated_at;
                snap.metrics_text = metrics_text;
                snap.boards = boards;
                snap.views = views;
            }
        },
        &mut scrape,
        slack_router.as_ref(),
        &mut daily,
        synthesis_refresh,
        reconciliation,
        reviews,
    )
    .await
    .context("daemon loop")?;

    // D10: stop the ingestion loop with the daemon, then drain its last stats.
    if let Some(tx) = ingest_stop {
        let _ = tx.send(());
    }
    if let Some(h) = ingest_handle {
        match h.await {
            Ok(s) => eprintln!(
                "fortuna-live: ingestion stopped — persisted={} duplicates={} dropped={} \
                 alerts={} persist_failures={}",
                s.persisted, s.duplicates, s.dropped, s.alerts, s.persist_failures
            ),
            Err(e) => eprintln!("fortuna-live: ingestion task join error: {e}"),
        }
        // OBS-2b: read the final published snapshot (proves the publish ran; the
        // same handle ROTA reads live). Empty generated_at => the loop never
        // ticked, so nothing to report.
        let snap = ingest_telemetry.read().await;
        if !snap.generated_at.is_empty() {
            eprintln!(
                "fortuna-live: ingestion funnel — fetched={} accepted={} normalized={} \
                 persisted={} sources={}",
                snap.funnel.fetched,
                snap.funnel.validated_accepted,
                snap.funnel.normalized,
                snap.funnel.persisted,
                snap.sources.len(),
            );
        }
    }

    eprintln!(
        "fortuna-live: clean shutdown — ticks={} polls={} poll_failures={} \
         halts_applied={} cancelled={} unacked={}",
        stats.ticks,
        stats.halt_polls,
        stats.poll_failures,
        stats.halts_applied,
        shutdown.cancelled,
        shutdown.unacked
    );
    Ok(())
}
