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
use fortuna_cognition::mind::{ModelTier, ReqwestMindTransport};
use fortuna_core::clock::{Clock, RealClock, SimClock};
use fortuna_live::boot::{validate_env, DaemonToml};
use fortuna_live::compose::DegradeScrape;
use fortuna_live::daemon::{
    build_kalshi_demo_transport, compose_kalshi_runner_with_transport, compose_runner,
    default_degrade_thresholds, drive, mind_from_env, triage_from_env, ActiveRunner, PgHaltPoller,
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
    // Spec 5.9 cognition tiering: ONE registry maps each role's tier → model; the
    // daemon builds each role's mind on its tier's model. The daily reconciliation
    // runs on the MID tier (Sonnet) — a SEPARATE mind, NOT the synthesis (Opus)
    // clone it used to be. Each tier gets its OWN transport (the API key reaches
    // only the transport, never config/logs); an absent key drops every tier to
    // StubMind together. All tiers share the `[cognition]` budget rails (per_cycle
    // + daily) and stay propose-only (I6 intact).
    let model_registry = dcfg.cognition.model_registry();
    let synthesis_transport = match validated.anthropic_api_key.as_ref() {
        Some(_) => Some(
            ReqwestMindTransport::from_env(std::time::Duration::from_secs(SYNTH_MIND_TIMEOUT_SECS))
                .context("anthropic synthesis transport")?,
        ),
        None => None,
    };
    let synthesis_mind = mind_from_env(
        &dcfg.cognition,
        model_registry.model(ModelTier::Synthesis),
        synthesis_transport,
        Arc::new(RealClock),
    );
    let mid_transport = match validated.anthropic_api_key.as_ref() {
        Some(_) => Some(
            ReqwestMindTransport::from_env(std::time::Duration::from_secs(SYNTH_MIND_TIMEOUT_SECS))
                .context("anthropic reconciliation (mid-tier) transport")?,
        ),
        None => None,
    };
    let reconciliation_mind = mind_from_env(
        &dcfg.cognition,
        model_registry.model(ModelTier::Mid),
        mid_transport,
        Arc::new(RealClock),
    );
    // The synthesis arm's TRIAGE tier (spec 5.9): the cheap Haiku gate that runs
    // BEFORE the frontier mind. Its OWN transport (the key reaches only the
    // transport); no key => AlwaysAccept (the recall-safe rule stub, byte-unchanged).
    let triage_transport = match validated.anthropic_api_key.as_ref() {
        Some(_) => Some(
            ReqwestMindTransport::from_env(std::time::Duration::from_secs(SYNTH_MIND_TIMEOUT_SECS))
                .context("anthropic triage (cheap-tier) transport")?,
        ),
        None => None,
    };
    let triage = triage_from_env(
        &dcfg.cognition,
        model_registry.model(ModelTier::Triage),
        triage_transport,
        Arc::new(RealClock),
    );
    if validated.anthropic_api_key.is_some() {
        eprintln!(
            "fortuna-live: cognition tiers = synthesis {} / reconciliation {} / triage {} (AnthropicMind, live)",
            dcfg.cognition.synthesis_model, dcfg.cognition.mid_model, dcfg.cognition.triage_model
        );
    } else {
        eprintln!("fortuna-live: cognition minds = StubMind (no ANTHROPIC_API_KEY; inert)");
    }
    // Demo-flip Phase 2: route on the booted venue. venue="kalshi" (gated to
    // stage="paper" by the boot check above) composes the KalshiVenue demo
    // runner — reading the demo CREDENTIALS from `env` (Secret-wrapped PEM,
    // never logged) — and wraps it ActiveRunner::Kalshi; every other (sim) venue
    // composes the Sim runner as before, wrapped ActiveRunner::Sim. The
    // ActiveRunner enum lets ONE drive() + ONE between-segments closure drive
    // either; the Sim arm is the unchanged sim path.
    // `weather_source` is the F7 live Kalshi day-set source: `Some` ONLY on the
    // kalshi venue (built from the SAME signed transport the runner trades
    // through — no second key read), `None` on sim ⇒ the F7 weather plug-in is
    // INERT off the kalshi demo. It is threaded into the discovery wiring below.
    let (mut active_runner, weather_source): (
        ActiveRunner,
        Option<Arc<dyn fortuna_venues::kalshi::WeatherMarketSource>>,
    ) = match dcfg.daemon.venue.as_str() {
        "kalshi" => {
            // The paper runner's clock: the SAME Arc<SimClock> RealCadence
            // advances by wall-elapsed ms (so the demo tracks real time). The
            // Sim path builds its clock inside compose_runner from `start`; for
            // Kalshi we build it here and thread it into the venue/transport.
            let clock = Arc::new(SimClock::new(start));
            // Build ONE signed demo transport and SHARE it: the runner trades
            // through it; the read-only F7 weather day-set source discovers
            // through it. The PEM is read once, never duplicated.
            let transport_clock: Arc<dyn Clock> = clock.clone();
            let transport = build_kalshi_demo_transport(&env, transport_clock)
                .context("kalshi demo transport")?;
            let weather: Option<Arc<dyn fortuna_venues::kalshi::WeatherMarketSource>> =
                Some(Arc::new(fortuna_venues::kalshi::KalshiWeatherSource::new(
                    transport.clone(),
                )));
            let runner = compose_kalshi_runner_with_transport(
                pool.clone(),
                &full,
                &dcfg,
                start,
                start_ms as u64,
                clock,
                synthesis_mind.clone(),
                triage,
                transport,
            )
            .await
            .context("kalshi composition")?;
            eprintln!(
                "fortuna-live: composed (venue=kalshi, stage=paper, series from [kalshi], \
                 demo creds from env, journal+audit in Postgres; F7 weather day-set source ON)"
            );
            (ActiveRunner::Kalshi(runner), weather)
        }
        _ => {
            let runner = compose_runner(
                pool.clone(),
                &full,
                &dcfg,
                start,
                start_ms as u64,
                synthesis_mind.clone(),
                triage,
            )
            .await
            .context("composition")?;
            eprintln!(
                "fortuna-live: composed (venue=sim, markets from [sim], journal+audit in Postgres)"
            );
            (ActiveRunner::Sim(runner), None)
        }
    };

    // The dashboard stage string follows the booted venue/stage (sim => "sim";
    // the Kalshi demo => "paper").
    let dashboard_stage = if dcfg.daemon.venue == "kalshi" {
        "paper".to_string()
    } else {
        "sim".to_string()
    };

    // Metrics endpoint (GET-only; bind from config — localhost default).
    let snapshot = Arc::new(RwLock::new(DashboardSnapshot {
        generated_at: start.to_iso8601(),
        stage: dashboard_stage,
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
        clock: active_runner.clock().clone(),
    };
    // S4: when [synthesis] is configured, hand drive() the pool + its filters
    // so the loop re-loads the confirmed edge set per segment (req 2). Built
    // BEFORE `pool` moves into the halt poller. Absent [synthesis] => None =>
    // the loop never reloads (a mechanically-only daemon).
    let synthesis_refresh = dcfg.synthesis.clone().map(|syn| (pool.clone(), syn));
    // slice-4d: hand drive() a pool for SCALAR-belief persistence exactly when a
    // scalar producer is composed. funding_forecast is the only scalar producer
    // today (perp_event_basis trades but drafts no scalar belief — trait default
    // is empty); the perp_event_basis arm is included so a future scalar
    // producer behind it persists with no further wiring, and draining an empty
    // buffer is a cheap no-op. Absent both => None => fail closed (no persist).
    // Built BEFORE `pool` moves into the halt poller below.
    let scalar_belief_persist =
        if dcfg.funding_forecast.is_some() || dcfg.perp_event_basis.is_some() {
            Some(pool.clone())
        } else {
            None
        };
    // slice-4e: the Sim-soak PerpTick feed. When [funding_forecast] carries a
    // `ticker_feed_jsonl`, load the RECORDED kinetics ticker frames so the perp
    // producers FIRE each segment (the Sim loop only sources BookSnapshots);
    // absent => None (the producers compose but stay idle until a real feed).
    let perp_tick_feed = match dcfg
        .funding_forecast
        .as_ref()
        .and_then(|f| f.ticker_feed_jsonl.as_deref())
    {
        Some(path) => Some(
            fortuna_live::perp_feed::PerpTickFeed::from_ws_ticker_jsonl(path)
                .context("load slice-4e Sim-soak perp tick feed")?,
        ),
        None => None,
    };
    // T4.1/M2 (spec 5.8) + 3-tier cognition (spec 5.9): the daily reconciliation
    // runs on the MID tier — the SEPARATE `reconciliation_mind` (mid_model/Sonnet)
    // built above, NOT a clone of the synthesis Opus mind. A stub mind (no key)
    // self-skips. Built BEFORE `pool` moves into the halt poller below.
    // (merge: keep the Mid-tier reconciliation from 3-tier cognition; track-a's
    // persona/discovery wiring below is additive and retained.)
    let reconciliation = Some((pool.clone(), reconciliation_mind));
    // OPT-IN [personas] wiring (default-off). PRESENT + enabled => load each
    // configured persona FAIL-CLOSED: read persona.md + schema.json, parse, fetch
    // the registry HEAD, and validate_against it (a file whose hash != the active
    // registry row — or an inactive/version-mismatched head — REFUSES to boot, so
    // a tampered method never runs, design §6). The persona STRATEGY id is built
    // ONCE here (no fallible id construction on the loop path). The persona mind is
    // the SAME synthesis mind (one build; a stub mind proposes nothing). Absent /
    // `enabled = false` => None => the persona step never runs (byte-identical
    // daemon). Built BEFORE `pool` + `synthesis_mind` move below.
    let persona_strategy = fortuna_core::market::StrategyId::new("domain-analysis")
        .map_err(|e| anyhow::anyhow!("building persona strategy id: {e}"))?;
    let personas_wiring = match dcfg.personas.as_ref() {
        Some(sec) if sec.enabled => {
            let mut schedules = Vec::new();
            for entry in &sec.personas {
                let md = std::fs::read_to_string(format!("{}/persona.md", entry.dir))
                    .with_context(|| format!("reading persona.md for {:?}", entry.id))?;
                let schema = std::fs::read_to_string(format!("{}/schema.json", entry.dir))
                    .with_context(|| format!("reading schema.json for {:?}", entry.id))?;
                let def = fortuna_cognition::persona::PersonaDef::parse(&md, &schema)
                    .with_context(|| format!("parsing persona {:?}", entry.id))?;
                let head = fortuna_ledger::PersonasRepo::new(pool.clone())
                    .head(&entry.id)
                    .await
                    .with_context(|| format!("registry head for persona {:?}", entry.id))?;
                let registry_head =
                    head.as_ref()
                        .map(|r| fortuna_cognition::persona::RegistryHead {
                            version: r.version,
                            method_hash: r.method_hash.clone(),
                            status: r.status.clone(),
                        });
                // FAIL-CLOSED: refuse a missing/inactive/hash/version mismatch.
                def.validate_against(registry_head.as_ref())
                    .with_context(|| {
                        format!("persona {:?} failed registry validation", entry.id)
                    })?;
                schedules.push(fortuna_cognition::persona_orchestrator::PersonaSchedule {
                    def,
                    cadences: entry.cadences.clone(),
                });
            }
            eprintln!(
                "fortuna-live: persona analysis ACTIVE ({} persona(s); strategy=domain-analysis)",
                schedules.len()
            );
            Some(fortuna_live::daemon::PersonasWiring {
                pool: pool.clone(),
                schedules,
                state: fortuna_cognition::persona_orchestrator::PersonaScheduleState::new(
                    sec.debounce_ms,
                ),
                budget: fortuna_cognition::discovery::DiscoveryBudget::new(
                    sec.budget_cents_per_day,
                ),
                mind: synthesis_mind.clone(),
                strategy: persona_strategy,
                window_hours: sec.window_hours,
                max_signals: sec.max_signals,
            })
        }
        _ => None,
    };
    // OPT-IN [discovery] WORLD-FORWARD wiring (default-off; spec 5.12, COMMIT 1).
    // PRESENT + enabled => load the curated source registry ONCE (the unscoreable
    // rule keys on it: a candidate whose resolution source is absent/disabled is
    // excluded from watchlist counts + beliefs), hold it in the wiring. The
    // discovery STRATEGY id is built ONCE here (no fallible id construction on the
    // loop path). The discovery mind is the SAME synthesis mind (one build; a stub
    // mind synthesizes nothing). Absent / `enabled = false` => None => the
    // world-forward step never runs (byte-identical daemon). Built BEFORE `pool` +
    // `synthesis_mind` move below.
    let discovery_strategy = fortuna_core::market::StrategyId::new("world-forward")
        .map_err(|e| anyhow::anyhow!("building discovery strategy id: {e}"))?;
    let discovery_wiring = match dcfg.discovery.as_ref() {
        Some(sec) if sec.enabled => {
            let rows = fortuna_ledger::SourceRegistryRepo::new(pool.clone())
                .load_all()
                .await
                .context("loading source registry for discovery")?;
            let source_count = rows.len();
            let mut registry = fortuna_cognition::signals::SourceRegistry::new();
            for r in rows {
                // trust_tier is i32 on the ledger row, 0..=10 by the DB CHECK; map
                // to the bounded TrustTier newtype (fail-closed on an out-of-range
                // tier — a row the funnel cannot trust must not boot quietly).
                let tier = u8::try_from(r.trust_tier)
                    .ok()
                    .and_then(|t| fortuna_cognition::signals::TrustTier::new(t).ok())
                    .with_context(|| {
                        format!(
                            "source {:?} has an out-of-range trust_tier {}",
                            r.source_id, r.trust_tier
                        )
                    })?;
                registry.upsert(fortuna_cognition::signals::SourceEntry {
                    source_id: r.source_id,
                    trust_tier: tier,
                    domain_tags: r.domain_tags,
                    enabled: r.enabled,
                });
            }
            let f7_weather = if weather_source.is_some() {
                "ON (kalshi day-set)"
            } else {
                "off (sim — no live book)"
            };
            eprintln!(
                "fortuna-live: discovery ACTIVE ({source_count} registry source(s); strategy=world-forward; market-back catalog INERT until T4.2; F7 weather {f7_weather})"
            );
            Some(fortuna_live::daemon::DiscoveryWiring {
                pool: pool.clone(),
                mind: synthesis_mind.clone(),
                budget: fortuna_cognition::discovery::DiscoveryBudget::new(
                    sec.budget_cents_per_day,
                ),
                registry,
                strategy: discovery_strategy,
                signal_kinds: sec.signal_kinds.clone(),
                window_hours: sec.window_hours,
                max_signals: sec.max_signals,
                // MARKET-BACK (COMMIT 2). The prefilter knobs come from config; the
                // per-category calibration-quality map is the T2.8 resolved record —
                // not yet wired here, so it starts EMPTY (a category absent from the
                // map scores 0.0, i.e. fails any positive min_category_quality). The
                // catalog is EMPTY (the live Kalshi catalog is not wired until T4.2;
                // GAPS), so the market-back step is INERT in prod even when enabled.
                // The id bases seed from the drive-start epoch (collision-free across
                // runs), exactly like the belief id base.
                prefilter: fortuna_cognition::discovery::PrefilterConfig {
                    category_allowlist: sec.category_allowlist.clone(),
                    min_volume_contracts: sec.min_volume_contracts,
                    min_category_quality: sec.min_category_quality,
                    category_quality: BTreeMap::new(),
                },
                catalog: Vec::new(),
                event_id_base: start_ms.max(0) as u64,
                edge_id_base: start_ms.max(0) as u64,
                // F7 live weather day-set source: Some on kalshi, None on sim
                // (built in the venue match above from the shared transport).
                weather_source,
            })
        }
        _ => None,
    };
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
    // slice-3b-v2 follow-on: clone the pool for the FUNDING-RATES poller BEFORE the
    // halt poller. GATED on `[perp_event_basis_v2]` (the v2 arm scores funding
    // beliefs, so its presence opts the daemon into FILLING the funding store the
    // resolve/score loop reads). Absent => None => no clone, no poller spawn.
    let funding_poll_pool = if dcfg.perp_event_basis_v2.is_some() {
        Some(pool.clone())
    } else {
        None
    };
    // Clone (not move) so `pool` survives for the daily belief-resolution arg below
    // (merge: main's resolver arg at the drive() call needs `pool` after this).
    let mut poller = PgHaltPoller::new(pool.clone());
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
                active_runner.clock().clone(),
            )
            .await
            .context("ingestion wiring")?;
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            let tick = std::time::Duration::from_millis(sec.tick_ms);
            let clk = active_runner.clock().clone();
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

    // slice-3b-v2 follow-on: spawn the FUNDING-RATES poller alongside the trading
    // loop, GATED on `[perp_event_basis_v2]` (default OFF => byte-unchanged daemon).
    // It fills `funding_rates_historical` (the public, UNAUTHENTICATED Kinetics GET
    // — no credential read on this path) so the per-segment resolve/score loop has
    // realized rates to score against. It is INDEPENDENT of the deterministic
    // trading cycle: the spawn does NOT block boot, and it STOPS with the daemon via
    // a `watch` cancel — the paired `watch::Sender` is dropped after `drive()`
    // returns (a dropped/sent sender ends `run_funding_poller`'s `cancel.changed()`
    // wait). `KineticsPublicFetch::production()` is host-PINNED (no payload-derived
    // URL); `on_report` logs each poll's outcome (the structured Slack routing is a
    // later ops slice — the daemon's existing alert path is segment-bound, so the
    // poller logs to stderr here, mirroring the ingestion task's eprintln summary).
    let (funding_poll_cancel, funding_poll_handle) = match funding_poll_pool {
        Some(fpool) => {
            // Build the host-pinned public fetch (no credential). A construction
            // failure (reqwest client build) is a LOUD boot error, not a silent
            // skip — the operator opted in via [perp_event_basis_v2].
            let fetch = fortuna_live::funding_poller::KineticsPublicFetch::production()
                .context("building the funding-rates public fetch")?;
            let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(());
            eprintln!(
                "fortuna-live: funding-rates poller ACTIVE (public Kinetics GET, no creds; \
                 fills funding_rates_historical for the v2 belief scoring)"
            );
            let handle = tokio::spawn(async move {
                let repo = fortuna_ledger::FundingRatesHistoricalRepo::new(fpool);
                // The poller's own wall-time clock (it polls past real 8h funding
                // boundaries); RealClock is the binary's one wall source.
                let clock = RealClock;
                fortuna_live::funding_poller::run_funding_poller(
                    &fetch,
                    &repo,
                    &clock,
                    cancel_rx,
                    |report: &fortuna_live::funding_poller::FundingPollReport| {
                        if report.fetch_failed {
                            eprintln!(
                                "fortuna-live: funding poll FETCH FAILED: {}",
                                report.fetch_alert.as_deref().unwrap_or("(no detail)")
                            );
                        } else {
                            eprintln!(
                                "fortuna-live: funding poll — fetched={} inserted={} \
                                 skipped_dup={} quarantined={}",
                                report.fetched,
                                report.inserted,
                                report.skipped_dup,
                                report.quarantined,
                            );
                        }
                    },
                )
                .await;
            });
            (Some(cancel_tx), Some(handle))
        }
        None => (None, None),
    };

    let snapshot_for_segments = snapshot.clone();
    // OBS-2c: a read handle to the live ingestion telemetry, merged into the ROTA
    // snapshot each segment so the V1/V2/V3 ingestion boards render LIVE daemon
    // data. Inert/degraded when ingestion is off (merge_ingest_views gates on an
    // empty telemetry — the daemon snapshot is byte-unchanged in that case).
    let ingest_telemetry_for_segments = ingest_telemetry.clone();
    let (stats, shutdown) = drive(
        &mut active_runner,
        &mut cadence,
        &mut poller,
        &loop_cfg,
        60, // segment = 60 wakes (~30s at the 500ms poll): metrics refresh cadence
        &mut stop_rx,
        move |r: &ActiveRunner, _seg| {
            // Build everything BEFORE taking the write lock (R8: minimise
            // time the snapshot is held). T4.3 ROTA slice 2: the daemon
            // shapes the per-view JSON the rota handlers serve verbatim
            // (R2 — fortuna-ops never depends on the runner). demo-flip Phase 2:
            // the runner is an ActiveRunner; for the Sim arm `boards_json` +
            // `rota_views` are the SAME full views as before (BYTE-IDENTICAL),
            // and for the Kalshi arm they are the venue-agnostic subset (no
            // SimVenue-only board reads). The telemetry pane + ingest merge below
            // are venue-agnostic and apply identically to both arms.
            let generated_at = fortuna_core::clock::Clock::now(r.clock().as_ref()).to_iso8601();
            let registry = r.metrics_registry();
            let metrics_text = registry.render_prometheus();
            let boards = r.boards_json();
            let mut views = r.rota_views(&generated_at);
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
        scalar_belief_persist,
        reconciliation,
        reviews,
        perp_tick_feed,
        personas_wiring,
        discovery_wiring,
        // Daily belief resolution: settled weather + funding beliefs graded once
        // per UTC day against the NWS-CLI / realized-funding ledger (off the money
        // path). The one ledger pool; the resolvers self-skip a day with nothing due.
        Some(pool.clone()),
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

    // slice-3b-v2 follow-on: stop the funding-rates poller with the daemon. Sending
    // on (then dropping) the `watch::Sender` ends `run_funding_poller`'s
    // `cancel.changed()` wait; the join awaits its clean exit. `None` => the poller
    // was never spawned (no [perp_event_basis_v2]).
    if let Some(cancel_tx) = funding_poll_cancel {
        let _ = cancel_tx.send(());
        drop(cancel_tx);
    }
    if let Some(h) = funding_poll_handle {
        if let Err(e) = h.await {
            eprintln!("fortuna-live: funding-rates poller task join error: {e}");
        } else {
            eprintln!("fortuna-live: funding-rates poller stopped");
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
