//! T4.1 hard requirement 5 (kickoff): the daemon drives ticks from a
//! real-time scheduler at the EDGE while the composition stays
//! deterministic under SimClock — the cadence driver is parameterized.
//! Halt-state poll runs at <=500ms (ASSUMPTIONS pin) and poll failures
//! are counted loudly (the Slack alert rides the req-3 degrade wiring).
//! Written red-first against a run_loop that did not exist.

use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_ledger::PgIntentJournal;
use fortuna_live::audit_bridge::PgAuditSink;
use fortuna_live::run_loop::{run_loop, CadenceDriver, HaltPoller, LoopConfig};
use fortuna_runner::{AuditSink, RunnerError, SimRunner};
use fortuna_venues::sim::SimVenue;
use sqlx::PgPool;
use std::sync::Arc;

mod common;
use common::{runner_config, set_arb_books, strategy, t0};

/// Sim cadence: "sleeping" advances the runner's own SimClock — the test
/// timeline is the composition's timeline, deterministically.
struct SimCadence {
    clock: Arc<SimClock>,
}

impl CadenceDriver for SimCadence {
    async fn sleep_ms(&mut self, ms: u64) {
        self.clock.advance_millis(ms).expect("sim clock advances");
    }
}

/// Scripted halt poller: counts calls; yields a halt or an error when told.
/// `standing_from_call` returns the SAME halt on every poll at/after it
/// (a persistent operator halt — the re-audit-flood vector).
#[derive(Default)]
struct ScriptedPoller {
    calls: u64,
    halt_at_call: Option<u64>,
    standing_from_call: Option<u64>,
    fail_at_call: Option<u64>,
}

impl HaltPoller for ScriptedPoller {
    async fn poll(&mut self) -> Result<Option<String>, String> {
        self.calls += 1;
        if Some(self.calls) == self.fail_at_call {
            return Err("halt store unreachable (scripted)".to_string());
        }
        if let Some(from) = self.standing_from_call {
            if self.calls >= from {
                return Ok(Some("operator halt (standing)".to_string()));
            }
        }
        if Some(self.calls) == self.halt_at_call {
            return Ok(Some("operator halt (scripted)".to_string()));
        }
        Ok(None)
    }
}

async fn compose(pool: &PgPool) -> SimRunner<SimVenue, PgIntentJournal> {
    let clock: Arc<dyn Clock> = Arc::new(SimClock::new(t0()));
    let journal = PgIntentJournal::new(pool.clone(), "sim", clock.clone());
    let sink = PgAuditSink::spawn(pool.clone(), clock, 7);
    SimRunner::new_with_journal(
        runner_config(42),
        vec![strategy()],
        Box::new(sink),
        t0(),
        journal,
    )
    .await
    .unwrap()
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn loop_ticks_at_cadence_and_polls_halts_at_500ms(pool: PgPool) {
    let mut r = compose(&pool).await;
    set_arb_books(&r);
    let mut cadence = SimCadence {
        clock: r.clock.clone(),
    };
    let mut poller = ScriptedPoller::default();
    let cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };

    // Ten loop wakes at the 500ms poll cadence = 5 simulated seconds:
    // five ticks, ten polls, zero failures.
    let (_tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut last_halt = None;
    let stats = run_loop(
        &mut r,
        &mut cadence,
        &mut poller,
        &cfg,
        Some(10),
        &mut stop,
        &mut last_halt,
    )
    .await
    .unwrap();
    assert_eq!(stats.halt_polls, 10, "{stats:?}");
    assert_eq!(stats.ticks, 5, "tick fires every second wake: {stats:?}");
    assert_eq!(stats.poll_failures, 0);
    assert_eq!(stats.halts_applied, 0);
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn polled_halt_applies_to_the_gates_and_audits(pool: PgPool) {
    let mut r = compose(&pool).await;
    set_arb_books(&r);
    let mut cadence = SimCadence {
        clock: r.clock.clone(),
    };
    let mut poller = ScriptedPoller {
        halt_at_call: Some(3),
        ..ScriptedPoller::default()
    };
    let cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };

    let (_tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut last_halt = None;
    let stats = run_loop(
        &mut r,
        &mut cadence,
        &mut poller,
        &cfg,
        Some(8),
        &mut stop,
        &mut last_halt,
    )
    .await
    .unwrap();
    assert_eq!(stats.halts_applied, 1, "{stats:?}");

    // The halt is on the GATES (ticks after it submit nothing) and on
    // the AUDIT record.
    let tick = r.tick().await.unwrap();
    assert!(tick.halted, "polled operator halt governs the gates");
    let halt_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'halt' AND payload->>'source' = 'halt_poll'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(halt_rows, 1, "the polled halt is audited");
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn a_running_daemon_never_auto_clears_a_halt_on_rearm_only_a_restart_does(pool: PgPool) {
    // R12 halt-rearm finding (2026-06-12), adjudicated option (a): I2's "no
    // automatic resumption" is enforced by keeping the RUNNING daemon halted
    // once it halts. An operator re-arm makes the durable halt fold show no
    // active halt — i.e. the poller starts returning Ok(None) — but that does
    // NOT clear the in-memory gate halt. Only a deliberate RESTART, whose boot
    // fold reads the set->rearm sequence, resumes trading. The closest sibling
    // (polled_halt_applies_to_the_gates_and_audits) proves the APPLY path and
    // covers this only incidentally; this is the EXPLICIT regression pin for
    // the option-(a) clear path the R12 drill surfaced. It guards against a
    // future "helpful" refactor that auto-clears on Ok(None) (option (b),
    // REJECTED). Mutation-proven non-vacuous: wiring any clear into run_loop's
    // Ok(None) arm flips both `tick.halted` and the clear-audit count, RED.
    let mut r = compose(&pool).await;
    set_arb_books(&r); // absent the halt there IS arb to trade — the halt is what stops it
    let mut cadence = SimCadence {
        clock: r.clock.clone(),
    };
    // Halt at call 2; calls 3.. all return Ok(None) — the re-armed / folded-
    // away state the running daemon must IGNORE (option a).
    let mut poller = ScriptedPoller {
        halt_at_call: Some(2),
        ..ScriptedPoller::default()
    };
    let cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };

    // 12 wakes: one pre-halt Ok(None), the halt at call 2, then TEN Ok(None)
    // "re-arm" polls. Were the daemon to auto-clear, ticks after call 2 would
    // resume and the post-loop `tick.halted` below would be false.
    let (_tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut last_halt = None;
    let stats = run_loop(
        &mut r,
        &mut cadence,
        &mut poller,
        &cfg,
        Some(12),
        &mut stop,
        &mut last_halt,
    )
    .await
    .unwrap();
    assert_eq!(stats.halts_applied, 1, "applied exactly once: {stats:?}");

    // After ten Ok(None) "re-armed" polls the gate halt STILL governs.
    let tick = r.tick().await.unwrap();
    assert!(
        tick.halted,
        "a running daemon stays halted across a re-arm; only a RESTART resumes (I2, option a)"
    );

    // The halt is audited exactly once (no re-audit across the Ok(None) tail)
    // and the running daemon writes NO clear/rearm/unhalt audit at all — re-arm
    // is restart-gated, never reflected by the live poll loop.
    let halt_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'halt' AND payload->>'source' = 'halt_poll'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        halt_rows, 1,
        "halt audited once; no re-audit across the Ok(None) tail"
    );
    let clear_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind IN ('rearm', 'unhalt', 'halt_clear')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        clear_rows, 0,
        "the running daemon writes NO clear/rearm audit — re-arm is restart-gated"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn a_standing_halt_audits_exactly_once_across_segment_boundaries(pool: PgPool) {
    // Gate finding 2026-06-11 (SECOND gate — the per-segment scope bug):
    // a standing halt must apply ONCE even though `drive` re-enters
    // run_loop every segment. This test CROSSES THE BOUNDARY: caller-owned
    // `last_halt` is threaded across THREE separate run_loop calls (= three
    // ~segments). The earlier single-call test never crossed the boundary,
    // which is exactly how the partial fix looked complete.
    let mut r = compose(&pool).await;
    let mut cadence = SimCadence {
        clock: r.clock.clone(),
    };
    let mut poller = ScriptedPoller {
        standing_from_call: Some(2),
        ..ScriptedPoller::default()
    };
    let cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };
    let (_tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut last_halt: Option<String> = None;
    let mut applied = 0u64;
    for _segment in 0..3 {
        let stats = run_loop(
            &mut r,
            &mut cadence,
            &mut poller,
            &cfg,
            Some(5),
            &mut stop,
            &mut last_halt,
        )
        .await
        .unwrap();
        applied += stats.halts_applied;
    }
    assert_eq!(
        applied, 1,
        "a standing halt applies ONCE across 3 segments, not once per segment"
    );
    let halt_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE kind = 'halt' AND payload->>'source' = 'halt_poll'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        halt_rows, 1,
        "exactly one halt audit row for the standing halt across segments"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn poll_failure_is_counted_never_silent_never_fatal(pool: PgPool) {
    let mut r = compose(&pool).await;
    let mut cadence = SimCadence {
        clock: r.clock.clone(),
    };
    let mut poller = ScriptedPoller {
        fail_at_call: Some(2),
        ..ScriptedPoller::default()
    };
    let cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
    };

    let (_tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut last_halt = None;
    let stats = run_loop(
        &mut r,
        &mut cadence,
        &mut poller,
        &cfg,
        Some(6),
        &mut stop,
        &mut last_halt,
    )
    .await
    .unwrap();
    assert_eq!(stats.poll_failures, 1, "{stats:?}");
    assert_eq!(
        stats.halt_polls, 6,
        "the loop keeps polling after a failure"
    );
    assert_eq!(stats.ticks, 3, "trading continues on last-known halt state");
}

#[test]
fn daily_scheduler_fires_once_per_utc_day() {
    use fortuna_core::clock::UtcTimestamp;
    use fortuna_live::daemon::DailyScheduler;
    let day = |iso: &str| UtcTimestamp::parse_iso8601(iso).unwrap();
    let mut s = DailyScheduler::new();
    // First call ever: due.
    assert!(s.due(day("2026-06-11T00:00:00.000Z")));
    // Same UTC day later: not due.
    assert!(!s.due(day("2026-06-11T23:59:59.000Z")));
    // Next UTC day: due.
    assert!(s.due(day("2026-06-12T00:00:00.000Z")));
    // Same new day again: not due.
    assert!(!s.due(day("2026-06-12T12:00:00.000Z")));
    // Two days later: due (no double-fire for the skipped day, by design).
    assert!(s.due(day("2026-06-14T06:00:00.000Z")));
}

#[test]
fn weekly_scheduler_fires_once_per_monday_aligned_week() {
    use fortuna_core::clock::UtcTimestamp;
    use fortuna_live::daemon::WeeklyScheduler;
    let t = |iso: &str| UtcTimestamp::parse_iso8601(iso).unwrap();
    let mut s = WeeklyScheduler::new();
    // 2026-06-11 is a Thursday; its Monday-aligned week (epoch week 2945) runs
    // Mon 2026-06-08 .. Sun 2026-06-14.
    assert!(s.due(t("2026-06-11T00:00:00.000Z")), "first call ever: due");
    assert!(
        !s.due(t("2026-06-14T23:59:59.000Z")),
        "same week (Sun 6-14): not due"
    );
    // 2026-06-15 is the next Monday => a new week (2946).
    assert!(
        s.due(t("2026-06-15T00:00:00.000Z")),
        "new Monday-aligned week: due"
    );
    assert!(
        !s.due(t("2026-06-21T12:00:00.000Z")),
        "same week again (Sun 6-21): not due"
    );
    // Three weeks later: due (no double-fire for the skipped weeks).
    assert!(
        s.due(t("2026-07-06T06:00:00.000Z")),
        "weeks later (week 2949): due"
    );
}

#[test]
fn monthly_scheduler_fires_once_per_calendar_month() {
    use fortuna_core::clock::UtcTimestamp;
    use fortuna_live::daemon::MonthlyScheduler;
    let t = |iso: &str| UtcTimestamp::parse_iso8601(iso).unwrap();
    let mut s = MonthlyScheduler::new();
    assert!(s.due(t("2026-06-01T00:00:00.000Z")), "first call ever: due");
    assert!(
        !s.due(t("2026-06-30T23:59:59.000Z")),
        "same calendar month: not due"
    );
    assert!(s.due(t("2026-07-01T00:00:00.000Z")), "new month: due");
    assert!(
        !s.due(t("2026-07-15T12:00:00.000Z")),
        "same month again: not due"
    );
    // Year boundary: a new calendar month.
    assert!(
        s.due(t("2027-01-03T06:00:00.000Z")),
        "new year-month: due (no double-fire for skipped months)"
    );
}

#[tokio::test]
async fn terse_daily_digest_labels_its_counters_honestly_as_since_boot() {
    // audit-tail-fix gate finding #3(b): terse_daily_digest reports the
    // runner's CUMULATIVE-since-boot counters; labeling them "the day's"
    // overstates (across a multi-day run they are not a single day's
    // activity). The label must say what it actually reports — true per-UTC-
    // day deltas are the future RICH digest (ledgered in GAPS).
    #[derive(Default)]
    struct NullSink;
    impl AuditSink for NullSink {
        fn append(
            &mut self,
            _kind: &str,
            _ref_id: Option<&str>,
            _payload: serde_json::Value,
        ) -> Result<(), RunnerError> {
            Ok(())
        }
    }
    let mut r =
        SimRunner::new(runner_config(5), vec![strategy()], Box::new(NullSink), t0()).unwrap();
    set_arb_books(&r);
    for _ in 0..3 {
        r.tick().await.unwrap();
    }
    let now = UtcTimestamp::parse_iso8601("2026-06-11T00:00:00.000Z").unwrap();
    let line = fortuna_live::daemon::terse_daily_digest(&r, now);
    assert!(line.contains("2026-06-11"), "carries the UTC date: {line}");
    assert!(
        line.contains("cumulative since boot"),
        "honest about the counter window, never implying 'the day's': {line}"
    );
    assert!(
        line.contains("ticks=3"),
        "reports the since-boot ticks: {line}"
    );
}
