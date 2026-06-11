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

    // Wall time enters here (binary edge) and at the cadence driver only.
    let start_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let start = fortuna_core::clock::UtcTimestamp::from_epoch_millis(start_ms)
        .context("wall clock start")?;

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

    // NOTE (ledgered): the dead-man pinger arms the external monitor on
    // its FIRST ping — it activates here, in the real daemon, only.
    // Wiring it is part of the ops tail with Slack routing; until then
    // the operator must not expect dead-man coverage from this binary.

    let mut cadence = RealCadence {
        clock: runner.clock.clone(),
    };
    let mut poller = PgHaltPoller::new(pool);
    let loop_cfg = LoopConfig {
        tick_interval_ms: dcfg.daemon.tick_interval_ms,
        halt_poll_ms: dcfg.daemon.halt_poll_ms,
    };
    let mut scrape = DegradeScrape::new(default_degrade_thresholds());

    let snapshot_for_segments = snapshot.clone();
    let (stats, shutdown) = drive(
        &mut runner,
        &mut cadence,
        &mut poller,
        &loop_cfg,
        60, // segment = 60 wakes (~30s at the 500ms poll): metrics refresh cadence
        &mut stop_rx,
        move |r, _seg| {
            let registry = registry_from(r);
            if let Ok(mut snap) = snapshot_for_segments.try_write() {
                snap.generated_at = fortuna_core::clock::Clock::now(r.clock.as_ref()).to_iso8601();
                snap.metrics_text = registry.render_prometheus();
                snap.boards = r.boards_json();
            }
        },
        &mut scrape,
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
