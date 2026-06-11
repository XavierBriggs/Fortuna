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
use fortuna_core::clock::{Clock, RealClock};
use fortuna_live::boot::{validate_env, DaemonToml};
use fortuna_live::compose::DegradeScrape;
use fortuna_live::daemon::{
    compose_runner, default_degrade_thresholds, drive, registry_from, PgHaltPoller,
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

    let mut runner = compose_runner(pool.clone(), &full, &dcfg, start, start_ms as u64)
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
    let dash_state = snapshot.clone();
    tokio::spawn(async move {
        if let Err(e) = serve_dashboard(listener, dash_state).await {
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

    let snapshot_for_segments = snapshot.clone();
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
            let metrics_text = registry_from(r).render_prometheus();
            let boards = r.boards_json();
            let views = fortuna_live::views::views_from(r, &generated_at);
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
    )
    .await
    .context("daemon loop")?;

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
