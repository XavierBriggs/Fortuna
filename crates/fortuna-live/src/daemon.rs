//! The T4.1 composition (library half): assemble the Sim-venue daemon
//! from validated config + a Postgres pool, and drive it in run-loop
//! SEGMENTS so the metrics snapshot refreshes between segments without
//! the loop knowing about HTTP.
//!
//! What composes here (and is smoke-tested deterministically):
//! boot-validated config -> PgIntentJournal (recovery fold = the
//! journal-side boot reconciliation) + PgAuditSink (I5 fail-synchronous)
//! -> SimRunner over the [sim] bracket world with mech_structural + the
//! opt-in [synthesis] arm (S3b) ->
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
//! The daily-boundary scheduler (DailyScheduler) fires once per UTC day,
//! emitting a terse daily digest to #fortuna-digest.
//!
//! The OPT-IN synthesis arm (S3b): a [synthesis] config section composes a
//! SynthesisStrategy from the confirmed-tier edge load (compose::
//! synthesis_edges) ALONGSIDE mech_structural. Its mind is a StubMind
//! PLACEHOLDER and calibration is None until S5 binds the real mind +
//! measured calibration, so the arm is INERT (prices no edge, makes no
//! trade) until then — do NOT start the soak before S5.
//!
//! HONESTLY NOT HERE YET (ledgered in GAPS; claims must match code): the
//! real-mind binding (S5: StubMind -> AnthropicMind via mind_from_env +
//! CostBudget) + belief drain/persist into the booted synthesis strategy
//! (S6); the RICH daily digest (full DigestInputs) + daily reconciliation
//! re-run + weekly/monthly cognition reviews (need belief/review data, S5/S6).

use crate::audit_bridge::PgAuditSink;
use crate::boot::{BootError, DaemonToml};
use crate::compose::DegradeScrape;
use crate::run_loop::{run_loop, CadenceDriver, HaltPoller, LoopConfig, LoopStats};
use fortuna_cognition::cycle::{ComparatorConfig, TriageDecision};
use fortuna_cognition::events::EdgeTier;
use fortuna_cognition::mind::StubMind;
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_core::market::{MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_ledger::{HaltsRepo, PgIntentJournal};
use fortuna_ops::alerts::DegradeThresholds;
use fortuna_ops::FortunaConfig;
use fortuna_runner::mech_structural::{MechStructural, MechStructuralConfig};
use fortuna_runner::synthesis::{SynthesisConfig, SynthesisStrategy};
use fortuna_runner::{RunnerConfig, RunnerError, SimRunner, Stage, Strategy};
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

    let mut strategies: Vec<Box<dyn Strategy>> = vec![Box::new(
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

    // T4.1/S3b: the OPT-IN synthesis arm. A `[synthesis]` section composes the
    // synthesis strategy ALONGSIDE the mechanical ones (I1: it rides the same
    // gate/exec path); its absence leaves the daemon mechanically-only (fail
    // closed). The edge set is the confirmed-tier load filtered by the config
    // (synthesis-edge-source-decision.md req 1+4); an empty set is VALID. The
    // mind is a StubMind PLACEHOLDER and calibration is None until S5 binds the
    // real mind + measured calibration, so the arm is INERT (structurally
    // prices no edge, makes no trade) until then — do NOT start the soak first.
    if let Some(syn) = &dcfg.synthesis {
        let edges = crate::compose::synthesis_edges(&pool, syn)
            .await
            .map_err(|e| DaemonError::Compose {
                reason: format!("synthesis edge load: {e}"),
            })?;
        let synth = SynthesisStrategy::new(
            SynthesisConfig {
                id: StrategyId::new("synthesis").map_err(|e| DaemonError::Compose {
                    reason: format!("synthesis strategy id: {e}"),
                })?,
                edges,
                comparator: ComparatorConfig {
                    min_edge_cents: 5,
                    required_tier: EdgeTier::Confirmed,
                },
                triage: TriageDecision::AlwaysAccept,
                shadow_quota: 0,
                calibration: None,
                stage: Stage::Sim,
            },
            Arc::new(StubMind::scripted(Vec::new())),
        );
        strategies.push(Box::new(synth));
    }

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
    daily: &mut DailyScheduler,
) -> Result<(LoopStats, fortuna_runner::ShutdownReport), DaemonError> {
    let mut total = LoopStats::default();
    // Dedup state OWNED ACROSS SEGMENTS (gate finding 2026-06-11: keeping
    // these inside run_loop reset them every ~30s segment, so one standing
    // halt / sustained poll-outage still flooded once per segment). The
    // halt re-applies only on identity change; the poll-failure alert
    // fires only on the TRANSITION into the failing state.
    let mut last_halt: Option<String> = None;
    let mut poll_failing = false;
    let mut total_send_failures = 0usize;
    loop {
        let stats = run_loop(
            runner,
            cadence,
            poller,
            loop_cfg,
            Some(wakes_per_segment),
            stop,
            &mut last_halt,
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
        // Halt-poll FAILURE alert (T4.1 req 5 pin), deduped on the
        // failing->ok TRANSITION so a sustained outage alerts ONCE, not
        // once per segment: the operator learns the halt rail went blind,
        // and learns again only after it recovers and fails anew.
        if stats.poll_failures > 0 && !poll_failing {
            poll_failing = true;
            alerts.push((
                fortuna_ops::MessageKind::Ops,
                "halt-state poll FAILING — halt rail blind, trading on last-known state"
                    .to_string(),
            ));
        } else if stats.poll_failures == 0 {
            poll_failing = false;
        }
        total_send_failures += route_alerts(slack, runner, &alerts).await;

        // Daily boundary (req 5 tail): on each new UTC day, emit the
        // end-of-day digest to #fortuna-digest + an audit row. The daily
        // RECONCILIATION re-run and the weekly/monthly cognition reviews
        // are the remaining req-5 surface (ledgered).
        let now = Clock::now(runner.clock.as_ref());
        if daily.due(now) {
            let digest = terse_daily_digest(runner, now);
            total_send_failures +=
                route_alerts(slack, runner, &[(fortuna_ops::MessageKind::Digest, digest)]).await;
        }

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
    // Surface the total Slack send-failure count on the final shutdown
    // audit's surrounding log path (never silently discarded): a daemon
    // that could not deliver N alerts to Slack is itself an ops signal.
    if total_send_failures > 0 {
        runner.apply_external_alert(
            "Ops",
            &format!("{total_send_failures} Slack alert send(s) failed over this run"),
        );
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
/// send it; record success, or hand the typed error to the CALLER-SUPPLIED
/// `on_failure` closure (the escalation policy lives at the call site —
/// main currently logs to stderr; the external monitor's own silence-page
/// is the escalation of record). A failed ping does NOT record, so the
/// next tick retries rather than backing off silent. The pinger NEVER
/// touches the real URL in tests: the test supplies a mock PingTransport.
/// Returns true iff a ping was attempted this tick.
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

/// UTC-day-boundary scheduler (T4.1 req 5 tail): the clock-injected core
/// that the daily reconciliation + digest key off. `due` is true on the
/// first call and whenever the UTC calendar day has changed since the
/// last fire (day = epoch-ms floored to 86_400_000), recording the new
/// day. Deterministic under the injected clock — no wall-time read.
#[derive(Debug, Default)]
pub struct DailyScheduler {
    last_day: Option<i64>,
}

impl DailyScheduler {
    pub fn new() -> DailyScheduler {
        DailyScheduler { last_day: None }
    }

    /// True iff `now` falls on a UTC day not yet fired (records it).
    pub fn due(&mut self, now: fortuna_core::clock::UtcTimestamp) -> bool {
        let day = now.epoch_millis().div_euclid(86_400_000);
        if self.last_day == Some(day) {
            return false;
        }
        self.last_day = Some(day);
        true
    }
}

/// A terse digest line (req 5): the date (00:00 UTC boundary), stage, and the
/// runner's headline counters. HONESTY (audit-tail-fix gate #3b): the counters
/// are CUMULATIVE SINCE BOOT — `RunCounters` accrue for the runner's lifetime,
/// not per UTC day — so the line says exactly that and never implies a single
/// day's activity. True per-UTC-day deltas (snapshot-at-boundary) are part of
/// the RICH DigestInputs composition (per-strategy rows, veto accounting) which,
/// with the weekly/monthly cognition reviews, is the remaining req-5 surface —
/// ledgered; it needs belief/review data that flows only once synthesis is in
/// the daemon (edge-source design-blocked).
pub fn terse_daily_digest<J: fortuna_exec::IntentJournal + Send>(
    runner: &SimRunner<J>,
    now: fortuna_core::clock::UtcTimestamp,
) -> String {
    let iso = now.to_iso8601();
    let date = iso.get(..10).unwrap_or(&iso);
    let c = runner.counters();
    format!(
        "FORTUNA digest {date} (sim, cumulative since boot): ticks={} orders={} \
         fills={} gate_rejections={} settlement_notices={} cognition_failures={}",
        c.ticks,
        c.orders_submitted,
        c.fills_applied,
        c.gate_rejections,
        c.settlement_notices,
        c.cognition_failures
    )
}
