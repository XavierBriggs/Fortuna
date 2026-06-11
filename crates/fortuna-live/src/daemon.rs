//! The T4.1 composition (library half): assemble the Sim-venue daemon
//! from validated config + a Postgres pool, and drive it in run-loop
//! SEGMENTS so the metrics snapshot refreshes between segments without
//! the loop knowing about HTTP.
//!
//! What composes here (and is smoke-tested deterministically):
//! boot-validated config -> PgIntentJournal (recovery fold = the
//! journal-side boot reconciliation) + PgAuditSink (I5 fail-synchronous)
//! -> SimRunner over the [sim] bracket world with mech_structural ->
//! run_loop (halt poll via HaltsRepo, ticks on the injected clock) ->
//! stop signal -> SimRunner::shutdown (cancel working orders + final
//! audit row).
//!
//! Degrade alerts ROUTE to Slack via route_alerts (audit row always;
//! send failure counted, never silent); main builds the router from the
//! validated env via build_slack_router over the reqwest transport.
//!
//! Belief persistence (persist_beliefs) + the drain path exist and are
//! tested; the daemon main persists them once a belief-producing
//! strategy is composed (mech_structural holds none today).
//!
//! The dead-man heartbeat runs as an independent spawned task in main
//! (deadman_tick, mock-tested; real pings only in the binary).
//!
//! HONESTLY NOT HERE YET (ledgered in GAPS; claims must match code):
//! the synthesis strategy in main (compose::calibration_for_scope +
//! persist_beliefs are tested, not yet fed into a daemon-booted
//! SynthesisStrategy — edge-source design-blocked), and the scheduled
//! daily/weekly loops.

use crate::audit_bridge::PgAuditSink;
use crate::boot::{BootError, DaemonToml};
use crate::compose::DegradeScrape;
use crate::run_loop::{run_loop, CadenceDriver, HaltPoller, LoopConfig, LoopStats};
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_core::market::{MarketId, VenueId};
use fortuna_core::money::Cents;
use fortuna_ledger::{HaltsRepo, PgIntentJournal};
use fortuna_ops::alerts::DegradeThresholds;
use fortuna_ops::FortunaConfig;
use fortuna_runner::mech_structural::{MechStructural, MechStructuralConfig};
use fortuna_runner::{RunnerConfig, RunnerError, SimRunner, Strategy};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, SettlementMeta};
use sqlx::PgPool;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error(transparent)]
    Boot(#[from] BootError),
    #[error(transparent)]
    Runner(#[from] RunnerError),
    #[error("composition error: {reason}")]
    Compose { reason: String },
}

fn sim_market(id: &str) -> Result<Market, DaemonError> {
    Ok(Market {
        id: MarketId::new(id).map_err(|e| DaemonError::Compose {
            reason: format!("[sim] market id {id:?}: {e}"),
        })?,
        venue: VenueId::new("sim").map_err(|e| DaemonError::Compose {
            reason: e.to_string(),
        })?,
        title: format!("sim bracket {id}"),
        category: "weather".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "sim".into(),
            resolution_source: "sim".into(),
            expected_lag_hours: 2,
        },
        volume_contracts: None,
        payout_per_contract: Cents::new(100),
    })
}

/// Assemble the Sim composition over Postgres journal + audit. The
/// returned runner has already completed the journal-side boot
/// reconciliation (OrderManager::recover inside the constructor).
pub async fn compose_runner(
    pool: PgPool,
    full: &FortunaConfig,
    dcfg: &DaemonToml,
    start: UtcTimestamp,
    audit_seed: u64,
) -> Result<SimRunner<PgIntentJournal>, DaemonError> {
    let sim = dcfg.sim.as_ref().ok_or_else(|| DaemonError::Compose {
        reason: "venue sim without [sim] section (validate_bootable should have refused)"
            .to_string(),
    })?;

    let mut markets = Vec::new();
    let mut sets: Vec<Vec<MarketId>> = Vec::new();
    for set in &sim.bracket_sets {
        let mut ids = Vec::new();
        for name in set {
            let m = sim_market(name)?;
            ids.push(m.id.clone());
            markets.push(m);
        }
        sets.push(ids);
    }

    let fees: FeeSchedule = full
        .fees
        .get("kalshi")
        .cloned()
        .ok_or_else(|| DaemonError::Compose {
            reason: "[fees.kalshi] missing (the sim world prices with it)".to_string(),
        })
        .and_then(|v| {
            v.try_into().map_err(|e| DaemonError::Compose {
                reason: format!("[fees.kalshi] does not parse as a schedule: {e}"),
            })
        })?;
    let fee_model = ScheduleFeeModel::new(vec![fees]).map_err(|e| DaemonError::Compose {
        reason: format!("fee schedule rejected: {e}"),
    })?;

    let strategies: Vec<Box<dyn Strategy>> = vec![Box::new(
        MechStructural::new(MechStructuralConfig {
            bracket_sets: sets,
            min_edge_cents_per_set: 2,
            max_unhedged_notional: Cents::new(5_000),
            max_leg_open_ms: 60_000,
            min_completion_edge_bps: 100,
        })
        .map_err(|e| DaemonError::Compose {
            reason: format!("mech_structural rejected its config: {e}"),
        })?,
    )];

    let envelopes = full
        .envelopes
        .iter()
        .map(|(k, v)| (k.clone(), Cents::new(*v)))
        .collect();

    let config = RunnerConfig {
        seed: audit_seed,
        gate_config: full.gates.clone(),
        exec_policy: fortuna_exec::ExecPolicy::default(),
        envelopes,
        max_daily_loss: Cents::new(full.gates.global.max_daily_loss_cents),
        fee_model,
        markets,
        starting_cash: Cents::new(1_000_000),
        faults: FaultConfig::none(audit_seed),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 50,
        kelly_fraction: full.sizing.kelly_fraction,
        veto_mind: None,
        veto_strategies: Vec::new(),
    };

    let journal_clock: Arc<dyn Clock> = Arc::new(fortuna_core::clock::SimClock::new(start));
    let journal = PgIntentJournal::new(pool.clone(), "sim", journal_clock.clone());
    let sink = PgAuditSink::spawn(pool, journal_clock, audit_seed);
    Ok(SimRunner::new_with_journal(config, strategies, Box::new(sink), start, journal).await?)
}

/// HaltsRepo-backed poller for the run loop (the durable halt store the
/// operator CLI writes; the daemon only ever APPLIES halts — re-arms are
/// CLI-only out-of-band, I2).
pub struct PgHaltPoller {
    repo: HaltsRepo,
}

impl PgHaltPoller {
    pub fn new(pool: PgPool) -> PgHaltPoller {
        PgHaltPoller {
            repo: HaltsRepo::new(pool),
        }
    }
}

impl HaltPoller for PgHaltPoller {
    async fn poll(&mut self) -> Result<Option<String>, String> {
        match self.repo.active().await {
            Ok(active) => Ok(active
                .first()
                .map(|(scope, reason)| format!("{scope:?}: {reason}"))),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Drive the composed runner in SEGMENTS until the stop signal fires,
/// then run the graceful-shutdown contract. Between segments the caller
/// refreshes whatever needs refreshing (the metrics snapshot in main).
/// Returns accumulated stats + the shutdown report.
#[allow(clippy::too_many_arguments)]
pub async fn drive<C: CadenceDriver, P: HaltPoller>(
    runner: &mut SimRunner<PgIntentJournal>,
    cadence: &mut C,
    poller: &mut P,
    loop_cfg: &LoopConfig,
    wakes_per_segment: u64,
    stop: &mut tokio::sync::oneshot::Receiver<()>,
    mut between_segments: impl FnMut(&SimRunner<PgIntentJournal>, &LoopStats),
    scrape: &mut DegradeScrape,
    slack: Option<&fortuna_ops::SlackRouter>,
) -> Result<(LoopStats, fortuna_runner::ShutdownReport), DaemonError> {
    let mut total = LoopStats::default();
    loop {
        let stats = run_loop(
            runner,
            cadence,
            poller,
            loop_cfg,
            Some(wakes_per_segment),
            stop,
        )
        .await?;
        total.ticks += stats.ticks;
        total.halt_polls += stats.halt_polls;
        total.poll_failures += stats.poll_failures;
        total.halts_applied += stats.halts_applied;

        // Degrade scrape per segment: alerts route to Slack (when
        // configured) and ALWAYS land as audit rows (spec 8). A Slack
        // send failure is audited, never silent.
        let counters = runner.counters();
        let mut alerts = scrape.scrape(counters.budget_breaches, counters.cognition_failures);
        // Halt-poll FAILURE alert (T4.1 req 5 pin: poll failures alert):
        // a segment that could not reach the halt store at all is an Ops
        // alert — trading continues on last-known halt state, but the
        // operator must know the halt rail went blind.
        if stats.poll_failures > 0 {
            alerts.push((
                fortuna_ops::MessageKind::Ops,
                format!(
                    "halt-state poll failed {} time(s) this segment — halt rail blind, \
                     trading on last-known state",
                    stats.poll_failures
                ),
            ));
        }
        let _send_failures = route_alerts(slack, runner, &alerts).await;

        between_segments(runner, &stats);

        // A fired/closed stop channel ends the daemon; the loop returning
        // early (fewer wakes than the segment asked for) means stop won.
        if stats.halt_polls < wakes_per_segment {
            break;
        }
        if stop.try_recv().is_ok() {
            break;
        }
    }
    let report = runner.shutdown().await?;
    Ok((total, report))
}

/// Default thresholds for the degrade scrape (failure bursts at 5+ per
/// segment alert; every budget breach alerts — matching the f-batch
/// alert rule).
pub fn default_degrade_thresholds() -> DegradeThresholds {
    DegradeThresholds {
        failure_alert_threshold: 5,
    }
}

/// Build a fresh Prometheus registry from the runner's metric export
/// (the same conversion the observability chain test proved; counters
/// re-described each refresh because the registry is rebuilt whole).
/// `inc_counter` on a fresh registry cannot fail; a sample the registry
/// refuses is skipped rather than panicking (metrics are diagnostics,
/// never a crash source).
pub fn registry_from<J: fortuna_exec::IntentJournal + Send>(
    runner: &SimRunner<J>,
) -> fortuna_ops::metrics::MetricsRegistry {
    let mut m = fortuna_ops::metrics::MetricsRegistry::new();
    for sample in runner.metrics_export() {
        let labels: Vec<(&str, &str)> = sample
            .labels
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        if sample.counter {
            m.describe_counter(sample.name, sample.help);
            let _ = m.inc_counter(sample.name, &labels, sample.value);
        } else {
            m.describe_gauge(sample.name, sample.help);
            m.set_gauge(sample.name, &labels, sample.value);
        }
    }
    m
}

/// Persist drained belief drafts to the ledger (req 6): the event is
/// created IF ABSENT (the beliefs FK requires it — `event_id` comes from
/// the draft, which the synthesis cycle derives from its edge config),
/// then the belief row. Returns the count persisted.
///
/// Idempotency on the event uses a checked EXISTS query, NOT error-string
/// sniffing (gate finding: string-matching DB errors is brittle). The
/// belief row in the append-only `beliefs` table IS the persistence
/// record — there is no separate audit row, by design (the belief ledger
/// is itself the auditable substrate; see GAPS req-6 note). Belief ids
/// are unique sortable TEXT PKs from a caller-monotonic base (NOT full
/// ULIDs — the daemon does not thread the runner's IdGen; uniqueness +
/// sort order is all the PK needs; ledgered).
///
/// A persistence error is NOT swallowed — a daemon that cannot record
/// its beliefs has lost its calibration substrate and the caller decides
/// (the daemon logs + continues; the belief was already counted in
/// metrics, and the NEXT drain retries nothing — drafts are point-in-time).
pub async fn persist_beliefs(
    pool: &PgPool,
    drafts: &[(
        fortuna_core::market::StrategyId,
        fortuna_cognition::beliefs::BeliefDraft,
    )],
    now_iso: &str,
    id_base: u64,
) -> Result<usize, DaemonError> {
    use fortuna_ledger::{BeliefsRepo, EventsRepo};
    let events = EventsRepo::new(pool.clone());
    let beliefs = BeliefsRepo::new(pool.clone());
    let mut n = 0usize;
    for (i, (strategy, draft)) in drafts.iter().enumerate() {
        // Create the event only if absent (checked existence, not
        // error-string sniffing). The daemon records a minimal row so
        // the FK holds; the discovery loop enriches it later (spec 5.12).
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM events WHERE event_id = $1)")
                .bind(&draft.event_id)
                .fetch_one(pool)
                .await
                .map_err(|e| DaemonError::Compose {
                    reason: format!("event existence check for {}: {e}", draft.event_id),
                })?;
        if !exists {
            events
                .create(
                    &draft.event_id,
                    &format!("belief event for {strategy}"),
                    "synthesis-derived",
                    "synthesis",
                    Some(&draft.horizon.to_iso8601()),
                    &draft.horizon.to_iso8601(),
                    "synthesis",
                    now_iso,
                )
                .await
                .map_err(|e| DaemonError::Compose {
                    reason: format!("event create for {}: {e}", draft.event_id),
                })?;
        }
        // Unique sortable TEXT PK from a caller-MONOTONIC base — never a
        // wall-clock guess; not a full ULID (ledgered, req-6 note).
        let belief_id = format!("01BLF{:021}", id_base + i as u64);
        beliefs
            .insert(
                &belief_id,
                now_iso,
                &draft.event_id,
                draft.p,
                draft.p_raw,
                &draft.horizon.to_iso8601(),
                &draft.evidence,
                &draft.provenance,
                None,
            )
            .await
            .map_err(|e| DaemonError::Compose {
                reason: format!("belief insert for {}: {e}", draft.event_id),
            })?;
        n += 1;
    }
    Ok(n)
}

/// Route degrade-scrape alerts to Slack and AUDIT every one (spec 8:
/// every outbound message is also an audit row; a routed alert that
/// FAILS to send is itself audited — never silently dropped). Without a
/// router (no Slack token / unconfigured) the alerts still land as audit
/// rows: the operator sees them in the trail, just not in Slack. A send
/// failure is counted in the return so the caller can escalate via the
/// dead-man path (spec: Slack delivery failure escalates) — wiring that
/// escalation is the ledgered next step.
pub async fn route_alerts<J: fortuna_exec::IntentJournal + Send>(
    router: Option<&fortuna_ops::SlackRouter>,
    runner: &mut SimRunner<J>,
    alerts: &[(fortuna_ops::MessageKind, String)],
) -> usize {
    let mut send_failures = 0usize;
    for (kind, message) in alerts {
        match router {
            None => {
                runner.apply_external_alert(&format!("{kind:?}"), message);
            }
            Some(r) => match r.send(*kind, message).await {
                Ok(sent) => {
                    runner.apply_external_alert(
                        &format!("{kind:?}"),
                        &format!(
                            "{message} [slack ts {}]",
                            sent.response_ts.unwrap_or_default()
                        ),
                    );
                }
                Err(e) => {
                    send_failures += 1;
                    runner.apply_external_alert(
                        &format!("{kind:?}"),
                        &format!("{message} [SLACK SEND FAILED: {e}]"),
                    );
                }
            },
        }
    }
    send_failures
}

/// Build the Slack router from the validated env (req 3 sliver): the
/// bot token + the per-channel ids (FORTUNA_SLACK_CHANNEL_<NAME>, already
/// validated present) over the supplied transport. `None` ONLY when no
/// bot token is configured (Slack disabled — alerts still audit); any
/// OTHER construction failure (a channel id the config names but env
/// lacks) is a LOUD error, never a silent None — a half-configured Slack
/// must not look like "Slack off".
pub fn build_slack_router(
    slack_cfg: &fortuna_ops::SlackConfig,
    bot_token: Option<&str>,
    channel_ids: std::collections::BTreeMap<String, String>,
    transport: Box<dyn fortuna_ops::SlackTransport>,
) -> Result<Option<fortuna_ops::SlackRouter>, DaemonError> {
    let token = match bot_token {
        None => return Ok(None),
        Some(t) => t.to_string(),
    };
    let router =
        fortuna_ops::SlackRouter::new(slack_cfg, channel_ids, token, transport).map_err(|e| {
            DaemonError::Compose {
                reason: format!("slack router construction: {e}"),
            }
        })?;
    Ok(Some(router))
}

/// One dead-man heartbeat tick (T4.1 req): if a ping is DUE at `now`,
/// send it; record success, or hand the typed error to `on_failure` for
/// escalation (the daemon audits + Ops-alerts it — a dead-man ping that
/// cannot reach the monitor is exactly what the operator must learn). The
/// pinger NEVER touches the real URL in tests: the test supplies a mock
/// PingTransport. Returns true iff a ping was attempted this tick.
pub async fn deadman_tick(
    pinger: &mut fortuna_ops::DeadmanPinger,
    now: fortuna_core::clock::UtcTimestamp,
    mut on_failure: impl FnMut(String),
) -> bool {
    if !pinger.due(now) {
        return false;
    }
    match pinger.ping().await {
        Ok(()) => pinger.record_ping(now),
        Err(e) => on_failure(e.to_string()),
    }
    true
}
