//! T4.2 item 2(v) Sub-slice A2 — the Slack Socket Mode ENVELOPE LOOP, written
//! from docs/research/ops/slack-api-2026-06-09/research.md (sections "Socket Mode"
//! and "Failure semantics") BEFORE the implementation. Mirrors the proven Kalshi
//! WS dial mock pattern (crates/fortuna-venues/src/kalshi/dial.rs:
//! MockWsTransport / MockWsConn / RecordingSleeper). Proves the loop teeth over a
//! MOCK transport (no live socket, no real apps.connections.open):
//!
//! - ack-FIRST: a per-envelope `{"envelope_id":id}` ack is written BEFORE any
//!   processing touches the sink (the 3-second-deadline guarantee).
//! - DEDUP: a redelivered envelope-id is acked again (stop retries); a
//!   durably-handled one is never re-dispatched (Socket Mode is at-least-once).
//! - RECONNECT: the loop survives a transport loss AND Slack's
//!   `disconnect`/refresh_requested lifecycle — capped-exponential backoff on
//!   genuine failures, no escalation on planned refreshes.
//! - CANCEL: a cancel watch stops the loop promptly (synchronous loop-top peek).
//! - PROTOCOL frames: `hello` is observed but never acked; a malformed frame is
//!   skipped (untrusted data, no panic, no ack).

use async_trait::async_trait;
use fortuna_ops::OpsError;
use fortuna_ops::{
    run_socket_loop, EnvelopeOutcome, EphemeralSender, HaltRequestSink, LoopEvent, SlackSocketConn,
    SlackSocketTransport, Sleeper, SocketDial, SocketDisconnect, SocketModeConfig,
    ACTION_REQUEST_HALT, ACTION_REQUEST_REARM,
};
use serde_json::{json, Value};
use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// A shared, ordered log written by BOTH the mock connection (on `send`, i.e. an
// ack) and the mock sink (on `request_halt`) — so a test can prove the ACK was
// written before the SINK was touched, in one totally-ordered sequence.
type Log = Arc<Mutex<Vec<String>>>;

// --------------------------------------------------------------------------
// Mocks — mirror the Kalshi dial's MockWsConn / MockWsTransport / RecordingSleeper.

struct MockConn {
    recvs: VecDeque<Result<Option<String>, SocketDisconnect>>,
    log: Log,
}

#[async_trait]
impl SlackSocketConn for MockConn {
    async fn send(&mut self, frame: &Value) -> Result<(), SocketDisconnect> {
        let id = frame["envelope_id"].as_str().unwrap_or("").to_string();
        self.log.lock().unwrap().push(format!("ack:{id}"));
        Ok(())
    }
    async fn recv(&mut self) -> Result<Option<String>, SocketDisconnect> {
        self.recvs
            .pop_front()
            .unwrap_or(Err(SocketDisconnect::Transport))
    }
}

type ScriptedConnect = Result<Vec<Result<Option<String>, SocketDisconnect>>, SocketDisconnect>;

struct MockTransport {
    connects: Mutex<VecDeque<ScriptedConnect>>,
    count: AtomicUsize,
    log: Log,
}

impl MockTransport {
    fn new(scripts: Vec<ScriptedConnect>, log: Log) -> Self {
        MockTransport {
            connects: Mutex::new(scripts.into()),
            count: AtomicUsize::new(0),
            log,
        }
    }
    fn connect_count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SlackSocketTransport for MockTransport {
    async fn connect(&self) -> Result<Box<dyn SlackSocketConn>, SocketDisconnect> {
        let n = self.count.fetch_add(1, Ordering::SeqCst);
        // Runaway-loop guard: the mock transport otherwise reconnects forever once
        // its script is exhausted (cancel-from-callback is the only terminator), so
        // a stop condition that never fires would HANG the battery. Fail fast and
        // loud instead.
        assert!(
            n < 50,
            "runaway reconnect loop (>50 connects) — a test stop condition never fired"
        );
        let next = self
            .connects
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(SocketDisconnect::Transport));
        match next {
            Ok(frames) => Ok(Box::new(MockConn {
                recvs: frames.into(),
                log: self.log.clone(),
            })),
            Err(cause) => Err(cause),
        }
    }
}

#[derive(Default)]
struct RecordingSleeper {
    slept: Mutex<Vec<Duration>>,
}
#[async_trait]
impl Sleeper for RecordingSleeper {
    async fn sleep(&self, dur: Duration) {
        self.slept.lock().unwrap().push(dur);
    }
}
impl RecordingSleeper {
    fn slept(&self) -> Vec<Duration> {
        self.slept.lock().unwrap().clone()
    }
}

struct MockSink {
    calls: Mutex<Vec<(String, String)>>,
    log: Log,
    fail: bool,
}
impl MockSink {
    fn new(log: Log) -> Self {
        MockSink {
            calls: Mutex::new(Vec::new()),
            log,
            fail: false,
        }
    }
    fn failing(log: Log) -> Self {
        MockSink {
            calls: Mutex::new(Vec::new()),
            log,
            fail: true,
        }
    }
    fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().unwrap().clone()
    }
}
#[async_trait]
impl HaltRequestSink for MockSink {
    async fn request_halt(&self, envelope_id: &str, reason: &str) -> Result<(), OpsError> {
        self.calls
            .lock()
            .unwrap()
            .push((envelope_id.to_string(), reason.to_string()));
        self.log.lock().unwrap().push(format!("halt:{envelope_id}"));
        if self.fail {
            Err(OpsError::Transport {
                reason: "mock sink failure".into(),
            })
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
struct MockEphemeral;
#[async_trait]
impl EphemeralSender for MockEphemeral {
    async fn send_ephemeral(&self, _text: &str) -> Result<(), OpsError> {
        Ok(())
    }
}

// --------------------------------------------------------------------------
// Frame builders (the wire JSON the transport yields).

fn hello() -> Result<Option<String>, SocketDisconnect> {
    Ok(Some(json!({ "type": "hello" }).to_string()))
}
fn disconnect_frame() -> Result<Option<String>, SocketDisconnect> {
    Ok(Some(
        json!({ "type": "disconnect", "reason": "refresh_requested" }).to_string(),
    ))
}
fn kill_envelope(id: &str, reason: &str) -> Result<Option<String>, SocketDisconnect> {
    Ok(Some(
        json!({
            "envelope_id": id,
            "type": "interactive",
            "payload": {
                "user": { "id": "U_OK" },
                "team": { "id": "T_HOME" },
                "actions": [ { "action_id": ACTION_REQUEST_HALT, "value": reason } ]
            }
        })
        .to_string(),
    ))
}
fn events_api_envelope(id: &str) -> Result<Option<String>, SocketDisconnect> {
    // An envelope type we don't act on but MUST ack so Slack stops retrying.
    Ok(Some(
        json!({ "envelope_id": id, "type": "events_api", "payload": { "event": {} } }).to_string(),
    ))
}

fn cfg() -> SocketModeConfig {
    SocketModeConfig {
        allowed_user_ids: ["U_OK".to_string()].into_iter().collect::<BTreeSet<_>>(),
        allowed_team_id: None,
    }
}

// base/cap chosen so the backoff schedule is unambiguous: 250, 500, 1000, 2000,
// 4000 (cap), 4000, ...
fn dial() -> SocketDial {
    SocketDial::new(Duration::from_millis(250), Duration::from_secs(4))
}

// Drive the loop, collecting LoopEvents, flipping cancel as soon as `stop`
// returns true for the accumulated sequence. Mirrors the dial tests' "cancel from
// the callback" pattern (the only loop terminator, since an exhausted mock
// transport otherwise reconnects forever).
async fn run_until<StopFn>(
    transport: &MockTransport,
    sink: &MockSink,
    eph: &MockEphemeral,
    sleeper: &RecordingSleeper,
    config: &SocketModeConfig,
    stop: StopFn,
) -> Vec<LoopEvent>
where
    StopFn: Fn(&[LoopEvent]) -> bool,
{
    let events: Arc<Mutex<Vec<LoopEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = tokio::sync::watch::channel(false);
    {
        let events = events.clone();
        run_socket_loop(
            transport,
            config,
            sink,
            eph,
            dial(),
            sleeper,
            rx,
            move |ev| {
                let mut g = events.lock().unwrap();
                g.push(ev);
                if stop(g.as_slice()) {
                    let _ = tx.send(true);
                }
            },
        )
        .await;
    }
    Arc::try_unwrap(events).unwrap().into_inner().unwrap()
}

// ==========================================================================

#[tokio::test]
async fn ack_is_written_before_the_sink_is_touched() {
    // hello (no ack) then one kill envelope. The shared log must show the ack
    // landing BEFORE the halt-request — ack-first is the 3s-deadline guarantee.
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let transport = MockTransport::new(
        vec![Ok(vec![hello(), kill_envelope("e1", "runaway")])],
        log.clone(),
    );
    let sink = MockSink::new(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let events = run_until(&transport, &sink, &eph, &sleeper, &cfg(), |evs| {
        matches!(evs.last(), Some(LoopEvent::Dispatched(_)))
    })
    .await;

    assert_eq!(
        events,
        vec![
            LoopEvent::Hello,
            LoopEvent::Dispatched(EnvelopeOutcome::HaltRequested)
        ]
    );
    assert_eq!(
        log.lock().unwrap().clone(),
        vec!["ack:e1".to_string(), "halt:e1".to_string()],
        "the ack is written before the sink is touched"
    );
    assert_eq!(
        sink.calls(),
        vec![("e1".to_string(), "runaway".to_string())]
    );
}

#[tokio::test]
async fn a_redelivered_envelope_is_acked_again_but_not_re_dispatched() {
    // Socket Mode is at-least-once: the SAME envelope_id arrives twice. Both are
    // acked (stop retries); the sink fires exactly ONCE (idempotency).
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let transport = MockTransport::new(
        vec![Ok(vec![
            kill_envelope("e1", "first"),
            kill_envelope("e1", "again"),
        ])],
        log.clone(),
    );
    let sink = MockSink::new(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let events = run_until(&transport, &sink, &eph, &sleeper, &cfg(), |evs| {
        matches!(evs.last(), Some(LoopEvent::Duplicate))
    })
    .await;

    assert_eq!(
        events,
        vec![
            LoopEvent::Dispatched(EnvelopeOutcome::HaltRequested),
            LoopEvent::Duplicate
        ]
    );
    assert_eq!(sink.calls().len(), 1, "the sink fires exactly once");
    assert_eq!(
        log.lock().unwrap().clone(),
        vec![
            "ack:e1".to_string(),
            "halt:e1".to_string(),
            "ack:e1".to_string()
        ],
        "the duplicate is acked again but not re-dispatched"
    );
}

#[tokio::test]
async fn the_loop_reconnects_after_a_transport_loss_and_keeps_processing() {
    // First connection drops mid-stream (ResetWithoutClose, a genuine failure);
    // the loop redials and processes on the second connection.
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let transport = MockTransport::new(
        vec![
            Ok(vec![hello(), Err(SocketDisconnect::ResetWithoutClose)]),
            Ok(vec![hello(), kill_envelope("e2", "after redial")]),
        ],
        log.clone(),
    );
    let sink = MockSink::new(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let events = run_until(&transport, &sink, &eph, &sleeper, &cfg(), |evs| {
        matches!(evs.last(), Some(LoopEvent::Dispatched(_)))
    })
    .await;

    assert_eq!(
        events,
        vec![
            LoopEvent::Hello,
            LoopEvent::Hello,
            LoopEvent::Dispatched(EnvelopeOutcome::HaltRequested)
        ]
    );
    assert_eq!(transport.connect_count(), 2, "redialled once");
    // Assert the schedule PREFIX, not exact length: the trailing backoff after
    // the final dispatch races `cancel` in the select (RecordingSleeper is
    // instant), so its presence is non-deterministic — but the first one (the
    // base backoff after the genuine loss) always lands.
    assert_eq!(
        sleeper.slept().first(),
        Some(&Duration::from_millis(250)),
        "one base backoff after the genuine loss"
    );
    assert_eq!(sink.calls().len(), 1);
}

#[tokio::test]
async fn a_disconnect_envelope_reconnects_without_escalating_the_backoff() {
    // Slack's planned refresh: TWO `disconnect` envelopes in a row, then a real
    // envelope. Each refresh reconnects at BASE — never escalating (a refresh
    // storm must not hot-escalate apps.connections.open).
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let transport = MockTransport::new(
        vec![
            Ok(vec![hello(), disconnect_frame()]),
            Ok(vec![hello(), disconnect_frame()]),
            Ok(vec![hello(), kill_envelope("e3", "finally")]),
        ],
        log.clone(),
    );
    let sink = MockSink::new(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let events = run_until(&transport, &sink, &eph, &sleeper, &cfg(), |evs| {
        matches!(evs.last(), Some(LoopEvent::Dispatched(_)))
    })
    .await;

    assert_eq!(
        events,
        vec![
            LoopEvent::Hello,
            LoopEvent::DisconnectRequested,
            LoopEvent::Hello,
            LoopEvent::DisconnectRequested,
            LoopEvent::Hello,
            LoopEvent::Dispatched(EnvelopeOutcome::HaltRequested),
        ]
    );
    assert_eq!(transport.connect_count(), 3);
    // Non-escalation is the invariant: EVERY recorded backoff is base (a planned
    // refresh resets the counter), regardless of how many land (the trailing one
    // races `cancel`). Two planned refreshes guarantee at least two.
    let slept = sleeper.slept();
    assert!(slept.len() >= 2, "two planned refreshes → ≥2 backoffs");
    assert!(
        slept.iter().all(|d| *d == Duration::from_millis(250)),
        "every reconnect is BASE — no escalation across refreshes: {slept:?}"
    );
    // A disconnect frame is never acked.
    assert_eq!(
        log.lock().unwrap().clone(),
        vec!["ack:e3".to_string(), "halt:e3".to_string()]
    );
}

#[tokio::test]
async fn genuine_connect_failures_escalate_the_backoff() {
    // Two refused connects (502 then transport) escalate base→2×base before the
    // loop finally connects and processes. (The success→reset case is covered by
    // the SocketDial unit test `a_success_resets_the_backoff_to_base`.)
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let transport = MockTransport::new(
        vec![
            Err(SocketDisconnect::ConnectHttpError { status: 502 }),
            Err(SocketDisconnect::Transport),
            Ok(vec![hello(), kill_envelope("e1", "finally")]),
        ],
        log.clone(),
    );
    let sink = MockSink::new(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let events = run_until(&transport, &sink, &eph, &sleeper, &cfg(), |evs| {
        matches!(evs.last(), Some(LoopEvent::Dispatched(_)))
    })
    .await;

    assert_eq!(
        events,
        vec![
            LoopEvent::Hello,
            LoopEvent::Dispatched(EnvelopeOutcome::HaltRequested)
        ]
    );
    assert_eq!(transport.connect_count(), 3);
    // Prefix assert (the trailing backoff after the dispatch races `cancel`).
    let slept = sleeper.slept();
    assert_eq!(
        slept.first(),
        Some(&Duration::from_millis(250)),
        "failure 1"
    );
    assert_eq!(
        slept.get(1),
        Some(&Duration::from_millis(500)),
        "failure 2 escalated"
    );
}

#[tokio::test]
async fn a_pre_cancelled_loop_never_connects() {
    // The synchronous loop-top cancel peek: a loop started already-cancelled does
    // zero work (mirrors run_dial's `if *cancel.borrow() { return }`).
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let transport = MockTransport::new(
        vec![Ok(vec![hello(), kill_envelope("e1", "x")])],
        log.clone(),
    );
    let sink = MockSink::new(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let (tx, rx) = tokio::sync::watch::channel(false);
    tx.send(true).unwrap();
    run_socket_loop(
        &transport,
        &cfg(),
        &sink,
        &eph,
        dial(),
        &sleeper,
        rx,
        |_| {
            panic!("a pre-cancelled loop must not process any frame");
        },
    )
    .await;

    assert_eq!(transport.connect_count(), 0);
    assert!(sink.calls().is_empty());
}

#[tokio::test]
async fn a_malformed_frame_is_skipped_without_panicking_and_without_an_ack() {
    // Untrusted data (spec 5.11): a frame that is not valid envelope JSON is a
    // no-op (no panic), and is NOT acked (there is no envelope_id to ack).
    // Processing continues to the next, valid envelope.
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let transport = MockTransport::new(
        vec![Ok(vec![
            hello(),
            Ok(Some("}{ not json at all".to_string())),
            kill_envelope("e1", "x"),
        ])],
        log.clone(),
    );
    let sink = MockSink::new(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let events = run_until(&transport, &sink, &eph, &sleeper, &cfg(), |evs| {
        matches!(evs.last(), Some(LoopEvent::Dispatched(_)))
    })
    .await;

    assert_eq!(
        events,
        vec![
            LoopEvent::Hello,
            LoopEvent::Unparseable,
            LoopEvent::Dispatched(EnvelopeOutcome::HaltRequested),
        ]
    );
    assert_eq!(
        log.lock().unwrap().clone(),
        vec!["ack:e1".to_string(), "halt:e1".to_string()],
        "only the valid envelope is acked"
    );
}

#[tokio::test]
async fn hello_is_observed_but_never_acked() {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let transport = MockTransport::new(vec![Ok(vec![hello()])], log.clone());
    let sink = MockSink::new(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let events = run_until(&transport, &sink, &eph, &sleeper, &cfg(), |evs| {
        matches!(evs.last(), Some(LoopEvent::Hello))
    })
    .await;

    assert_eq!(events, vec![LoopEvent::Hello]);
    assert!(
        log.lock().unwrap().is_empty(),
        "hello carries no envelope_id and is never acked"
    );
    assert!(sink.calls().is_empty());
}

#[tokio::test]
async fn an_events_api_envelope_is_acked_but_not_acted_on() {
    // A non-action envelope (events_api) MUST be acked (so Slack stops retrying)
    // but routes to NO operator action (IgnoredNonAction). Acking is not acting.
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let transport = MockTransport::new(vec![Ok(vec![events_api_envelope("ev9")])], log.clone());
    let sink = MockSink::new(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let events = run_until(&transport, &sink, &eph, &sleeper, &cfg(), |evs| {
        matches!(evs.last(), Some(LoopEvent::Dispatched(_)))
    })
    .await;

    assert_eq!(
        events,
        vec![LoopEvent::Dispatched(EnvelopeOutcome::IgnoredNonAction)]
    );
    assert_eq!(
        log.lock().unwrap().clone(),
        vec!["ack:ev9".to_string()],
        "acked, but the sink is never touched"
    );
    assert!(sink.calls().is_empty());
}

#[tokio::test]
async fn a_rearm_button_over_the_socket_is_refused_and_never_un_halts() {
    // I2 over the live loop: a re-arm action arriving on the socket is acked
    // (Slack stops retrying) but REFUSED — the sink is never touched.
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let rearm = Ok(Some(
        json!({
            "envelope_id": "r1",
            "type": "interactive",
            "payload": {
                "user": { "id": "U_OK" },
                "team": { "id": "T_HOME" },
                "actions": [ { "action_id": ACTION_REQUEST_REARM, "value": "let me back in" } ]
            }
        })
        .to_string(),
    ));
    let transport = MockTransport::new(vec![Ok(vec![rearm])], log.clone());
    let sink = MockSink::new(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let events = run_until(&transport, &sink, &eph, &sleeper, &cfg(), |evs| {
        matches!(evs.last(), Some(LoopEvent::Dispatched(_)))
    })
    .await;

    assert_eq!(
        events,
        vec![LoopEvent::Dispatched(EnvelopeOutcome::RearmRefused)]
    );
    assert!(
        sink.calls().is_empty(),
        "I2: a re-arm over Slack emits NO halt and CANNOT un-halt"
    );
    assert_eq!(
        log.lock().unwrap().clone(),
        vec!["ack:r1".to_string()],
        "acked (stop retries) but the sink is never touched"
    );
}

#[tokio::test]
async fn a_sink_error_is_surfaced_and_does_not_kill_the_loop() {
    // The sink (gate halt path) fails: the loop surfaces SinkError and keeps
    // running for the NEXT envelope (a failed halt-delivery is not fatal to the
    // listener — the daemon decides escalation).
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let transport = MockTransport::new(
        vec![Ok(vec![
            kill_envelope("e1", "x"),
            events_api_envelope("ev2"),
        ])],
        log.clone(),
    );
    let sink = MockSink::failing(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let events = run_until(&transport, &sink, &eph, &sleeper, &cfg(), |evs| {
        evs.len() == 2
    })
    .await;

    assert_eq!(
        events,
        vec![
            LoopEvent::Dispatched(EnvelopeOutcome::SinkError),
            LoopEvent::Dispatched(EnvelopeOutcome::IgnoredNonAction),
        ]
    );
    assert_eq!(sink.calls().len(), 1, "the halt was attempted once");
}

#[tokio::test]
async fn a_failed_halt_is_retryable_and_not_swallowed_by_dedup() {
    // The dedup must suppress a re-dispatch only for a DURABLY-handled envelope.
    // The sink FAILS, so the halt did not apply — a Slack redelivery of the SAME
    // envelope_id must RE-ATTEMPT the halt (the safe direction), NOT be eaten as a
    // Duplicate. Contrast `a_redelivered_envelope_is_acked_again_but_not_re_dispatched`
    // (succeeding sink → the redelivery IS a Duplicate).
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let transport = MockTransport::new(
        vec![Ok(vec![
            kill_envelope("e1", "halt"),
            kill_envelope("e1", "halt"),
        ])],
        log.clone(),
    );
    let sink = MockSink::failing(log.clone());
    let eph = MockEphemeral;
    let sleeper = RecordingSleeper::default();

    let events = run_until(&transport, &sink, &eph, &sleeper, &cfg(), |evs| {
        evs.len() == 2
    })
    .await;

    assert_eq!(
        events,
        vec![
            LoopEvent::Dispatched(EnvelopeOutcome::SinkError),
            LoopEvent::Dispatched(EnvelopeOutcome::SinkError),
        ],
        "the redelivery is re-dispatched, not swallowed as a Duplicate"
    );
    assert_eq!(
        sink.calls().len(),
        2,
        "a failed halt is re-attempted on redelivery"
    );
    assert_eq!(
        log.lock().unwrap().clone(),
        vec![
            "ack:e1".to_string(),
            "halt:e1".to_string(),
            "ack:e1".to_string(),
            "halt:e1".to_string()
        ],
        "both deliveries are acked AND re-attempted"
    );
}
