//! T4.1 hard requirement 5 (kickoff): the daemon drives ticks from a
//! real-time scheduler at the EDGE while the composition stays
//! deterministic under SimClock — the cadence driver is parameterized.
//! Halt-state poll runs at <=500ms (ASSUMPTIONS pin) and poll failures
//! are counted loudly (the Slack alert rides the req-3 degrade wiring).
//! Written red-first against a run_loop that did not exist.

use fortuna_core::clock::{Clock, SimClock};
use fortuna_ledger::PgIntentJournal;
use fortuna_live::audit_bridge::PgAuditSink;
use fortuna_live::run_loop::{run_loop, CadenceDriver, HaltPoller, LoopConfig};
use fortuna_runner::SimRunner;
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
#[derive(Default)]
struct ScriptedPoller {
    calls: u64,
    halt_at_call: Option<u64>,
    fail_at_call: Option<u64>,
}

impl HaltPoller for ScriptedPoller {
    async fn poll(&mut self) -> Result<Option<String>, String> {
        self.calls += 1;
        if Some(self.calls) == self.fail_at_call {
            return Err("halt store unreachable (scripted)".to_string());
        }
        if Some(self.calls) == self.halt_at_call {
            return Ok(Some("operator halt (scripted)".to_string()));
        }
        Ok(None)
    }
}

async fn compose(pool: &PgPool) -> SimRunner<PgIntentJournal> {
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
    let stats = run_loop(&mut r, &mut cadence, &mut poller, &cfg, Some(10))
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

    let stats = run_loop(&mut r, &mut cadence, &mut poller, &cfg, Some(8))
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

    let stats = run_loop(&mut r, &mut cadence, &mut poller, &cfg, Some(6))
        .await
        .unwrap();
    assert_eq!(stats.poll_failures, 1, "{stats:?}");
    assert_eq!(
        stats.halt_polls, 6,
        "the loop keeps polling after a failure"
    );
    assert_eq!(stats.ticks, 3, "trading continues on last-known halt state");
}
