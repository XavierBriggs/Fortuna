//! The daemon run loop (T4.1 requirement 5): wakes on the HALT-POLL
//! cadence (<=500ms, the ASSUMPTIONS pin), ticks the runner when the
//! tick interval has elapsed on the RUNNER'S OWN CLOCK, and counts
//! everything honestly. The cadence driver is parameterized: the binary
//! sleeps on tokio wall time (`RealCadence`); tests advance the SimClock
//! — one timeline, deterministic either way (the composition never reads
//! wall time itself).
//!
//! Poll-failure posture: counted in `LoopStats` and never silent — but
//! NOT fatal and NOT a local halt (the pin says alert; `drive` routes an
//! Ops alert on the failure transition — see daemon.rs). Trading
//! continues on the last-known halt state.
//!
//! Scope note (honest): loop termination here is `max_wakes`; the
//! binary's SIGTERM handler interrupts the loop at the EDGE and then
//! calls `SimRunner::shutdown` (the proven contract) — that wiring lands
//! with the composition main, not in this module.

use fortuna_core::clock::Clock;
use fortuna_exec::IntentJournal;
use fortuna_runner::{RunnerError, SimRunner};

#[derive(Debug, Clone)]
pub struct LoopConfig {
    pub tick_interval_ms: u64,
    pub halt_poll_ms: u64,
}

/// Honest loop accounting; the composition exports these as metrics.
#[derive(Debug, Default)]
pub struct LoopStats {
    pub ticks: u64,
    pub halt_polls: u64,
    pub poll_failures: u64,
    pub halts_applied: u64,
}

/// How the loop waits between wakes. The daemon uses wall time; tests
/// advance the simulation clock. Static dispatch only (no dyn): the
/// composition picks its driver at compile time.
pub trait CadenceDriver {
    fn sleep_ms(&mut self, ms: u64) -> impl std::future::Future<Output = ()> + Send;
}

/// Wall-clock cadence for the real daemon: sleeps wall time, then
/// ADVANCES the composition's SimClock by the slept amount — wall time
/// enters the system ONLY here, at the edge; everything inside still
/// reads the injected clock (the kickoff's "RealClock at the edges,
/// SimClock semantics preserved").
pub struct RealCadence {
    pub clock: std::sync::Arc<fortuna_core::clock::SimClock>,
}

impl CadenceDriver for RealCadence {
    async fn sleep_ms(&mut self, ms: u64) {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        // Advance failure means the sim timestamp would overflow — at
        // which point refusing to advance (and thus to tick) is the
        // conservative behavior; the halt poll keeps running.
        let _ = self.clock.advance_millis(ms);
    }
}

/// The durable halt-state source (HaltsRepo in the composition; scripted
/// in tests). `Ok(Some(reason))` = an active halt the loop must apply.
pub trait HaltPoller {
    fn poll(&mut self) -> impl std::future::Future<Output = Result<Option<String>, String>> + Send;
}

/// Drive the composed runner: poll halts every wake, tick when due.
/// `max_wakes` bounds the loop (tests and the DST smoke); the daemon
/// passes a large bound per run-segment and re-enters.
///
/// `last_halt` is the dedup state for the standing-halt apply+audit — it
/// is OWNED BY THE CALLER and threaded across segments (a persistent
/// halt re-applied every 500ms poll, OR re-applied once per ~30s segment
/// because the dedup reset on re-entry, would flood the I5 audit table;
/// gate finding 2026-06-11 — the per-segment reset was the second-gate
/// scope bug). The gates stay halted regardless of whether we re-audit.
#[allow(clippy::too_many_arguments)]
pub async fn run_loop<J, C, P>(
    runner: &mut SimRunner<J>,
    cadence: &mut C,
    poller: &mut P,
    cfg: &LoopConfig,
    max_wakes: Option<u64>,
    stop: &mut tokio::sync::oneshot::Receiver<()>,
    last_halt: &mut Option<String>,
) -> Result<LoopStats, RunnerError>
where
    J: IntentJournal + Send,
    C: CadenceDriver,
    P: HaltPoller,
{
    let mut stats = LoopStats::default();
    let mut last_tick_ms = runner.clock.now().epoch_millis();
    let mut wakes: u64 = 0;
    loop {
        if let Some(max) = max_wakes {
            if wakes >= max {
                break;
            }
        }
        // The stop signal (SIGTERM in main; a fired oneshot in the
        // smoke) wins the race against the next wake: the loop exits and
        // the CALLER runs SimRunner::shutdown — one path, signal or not.
        tokio::select! {
            biased;
            _ = &mut *stop => break,
            _ = cadence.sleep_ms(cfg.halt_poll_ms) => {}
        }
        wakes += 1;

        stats.halt_polls += 1;
        match poller.poll().await {
            Ok(Some(reason)) => {
                // Dedup on identity (caller-owned across segments): apply
                // +audit only when the standing halt first appears or its
                // reason changes.
                if last_halt.as_deref() != Some(reason.as_str()) {
                    runner.apply_external_halt(&reason);
                    stats.halts_applied += 1;
                    *last_halt = Some(reason);
                }
            }
            Ok(None) => {
                // The halt cleared out-of-band (operator re-arm); a later
                // halt with the same reason is a NEW event to audit.
                *last_halt = None;
            }
            Err(_store) => {
                // The halt store is unreachable: count it AND alert via
                // the run loop's poll-failure signal (drive routes it);
                // last-known halt state governs until the store answers.
                stats.poll_failures += 1;
            }
        }

        let now_ms = runner.clock.now().epoch_millis();
        if now_ms.saturating_sub(last_tick_ms) >= cfg.tick_interval_ms as i64 {
            runner.tick().await?;
            stats.ticks += 1;
            last_tick_ms = now_ms;
        }
    }
    Ok(stats)
}
