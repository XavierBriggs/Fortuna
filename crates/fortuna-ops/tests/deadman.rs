//! Dead-man pinger tests, written from spec Section 8: "FORTUNA pings an
//! external monitor (off-ITHACA) every minute; missed pings alert via the
//! monitor's own channel." Cadence math uses injected time only (SimClock);
//! the runner owns the loop, this crate owns the pieces.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use futures::executor::block_on;

use fortuna_ops::{DeadmanConfig, DeadmanPinger, OpsError, PingTransport};

#[derive(Clone, Default)]
struct MockPing {
    urls: Arc<Mutex<Vec<String>>>,
    script: Arc<Mutex<VecDeque<Result<(), OpsError>>>>,
}

impl MockPing {
    fn respond(&self, response: Result<(), OpsError>) {
        self.script.lock().unwrap().push_back(response);
    }

    fn urls(&self) -> Vec<String> {
        self.urls.lock().unwrap().clone()
    }
}

#[async_trait]
impl PingTransport for MockPing {
    async fn ping(&self, url: &str) -> Result<(), OpsError> {
        self.urls.lock().unwrap().push(url.to_string());
        self.script.lock().unwrap().pop_front().unwrap_or(Ok(()))
    }
}

fn pinger_with(mock: &MockPing, interval_secs: u64) -> DeadmanPinger {
    DeadmanPinger::new(
        "https://hc.example/ping/uuid-secret".to_string(),
        &DeadmanConfig {
            ping_interval_secs: interval_secs,
        },
        Box::new(mock.clone()),
    )
    .unwrap()
}

fn sim_clock() -> SimClock {
    SimClock::new(UtcTimestamp::from_epoch_millis(1_700_000_000_000).unwrap())
}

// ------------------------------------------------------------ cadence math --

#[test]
fn first_call_is_always_due() {
    let pinger = pinger_with(&MockPing::default(), 60);
    let clock = sim_clock();
    assert!(pinger.due(clock.now()));
}

#[test]
fn not_due_before_a_full_interval_has_elapsed() {
    let mut pinger = pinger_with(&MockPing::default(), 60);
    let clock = sim_clock();
    pinger.record_ping(clock.now());
    clock.advance_millis(59_999).unwrap();
    assert!(!pinger.due(clock.now()));
}

#[test]
fn due_exactly_at_the_interval_boundary() {
    let mut pinger = pinger_with(&MockPing::default(), 60);
    let clock = sim_clock();
    pinger.record_ping(clock.now());
    clock.advance_millis(60_000).unwrap();
    assert!(pinger.due(clock.now()));
}

#[test]
fn record_ping_resets_the_cadence_window_over_multiple_cycles() {
    let mut pinger = pinger_with(&MockPing::default(), 60);
    let clock = sim_clock();
    for _ in 0..3 {
        assert!(pinger.due(clock.now()));
        pinger.record_ping(clock.now());
        clock.advance_millis(30_000).unwrap();
        assert!(!pinger.due(clock.now()));
        clock.advance_millis(30_000).unwrap();
        assert!(pinger.due(clock.now()));
    }
}

#[test]
fn overdue_long_past_the_interval_is_still_due() {
    let mut pinger = pinger_with(&MockPing::default(), 60);
    let clock = sim_clock();
    pinger.record_ping(clock.now());
    clock.advance_millis(3_600_000).unwrap();
    assert!(pinger.due(clock.now()));
}

#[test]
fn backwards_time_is_not_due() {
    // A `now` earlier than the recorded ping is a clock anomaly. due()=false
    // fails toward the external monitor alerting (the safe direction: the
    // system must never paper over its own time defects by pinging anyway).
    let mut pinger = pinger_with(&MockPing::default(), 60);
    let later = UtcTimestamp::from_epoch_millis(1_700_000_120_000).unwrap();
    let earlier = UtcTimestamp::from_epoch_millis(1_700_000_000_000).unwrap();
    pinger.record_ping(later);
    assert!(!pinger.due(earlier));
}

#[test]
fn zero_interval_is_rejected_at_construction() {
    // DeadmanPinger intentionally has no Debug (it holds the secret ping
    // URL), so unwrap_err() is unavailable; match instead.
    let result = DeadmanPinger::new(
        "https://hc.example/ping/uuid-secret".to_string(),
        &DeadmanConfig {
            ping_interval_secs: 0,
        },
        Box::new(MockPing::default()),
    );
    match result {
        Err(OpsError::Config { .. }) => {}
        Err(other) => panic!("expected Config error, got {other:?}"),
        Ok(_) => panic!("construction unexpectedly succeeded"),
    }
}

// ------------------------------------------------------------------- pings --

#[test]
fn ping_hits_the_configured_url() {
    let mock = MockPing::default();
    let pinger = pinger_with(&mock, 60);
    block_on(pinger.ping()).unwrap();
    assert_eq!(mock.urls(), vec!["https://hc.example/ping/uuid-secret"]);
}

#[test]
fn ping_http_failure_surfaces_typed_for_the_runner_to_escalate() {
    // Spec Section 8: "Slack delivery failures escalate through the dead-man
    // monitor's channel" — the runner needs the typed error to do that.
    let mock = MockPing::default();
    mock.respond(Err(OpsError::Http { status: 500 }));
    let pinger = pinger_with(&mock, 60);
    let err = block_on(pinger.ping()).unwrap_err();
    assert!(matches!(err, OpsError::Http { status: 500 }), "got {err:?}");
}

#[test]
fn ping_transport_failure_surfaces_typed() {
    let mock = MockPing::default();
    mock.respond(Err(OpsError::Transport {
        reason: "connection refused".to_string(),
    }));
    let pinger = pinger_with(&mock, 60);
    let err = block_on(pinger.ping()).unwrap_err();
    assert!(matches!(err, OpsError::Transport { .. }), "got {err:?}");
}
