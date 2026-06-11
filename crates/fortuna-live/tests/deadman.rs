//! T4.1 dead-man heartbeat: daemon::deadman_tick pings when DUE, records
//! success, and escalates a typed failure — with a MOCK ping transport
//! that NEVER touches the real FORTUNA_DEADMAN_URL (the kickoff hazard:
//! the first real ping arms the external monitor). The DeadmanPinger's
//! cadence is unit-tested in fortuna-ops; this pins the daemon's tick +
//! escalation seam.

use async_trait::async_trait;
use fortuna_core::clock::UtcTimestamp;
use fortuna_live::daemon::deadman_tick;
use fortuna_ops::{DeadmanConfig, DeadmanPinger, OpsError, PingTransport};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

struct MockPing {
    pings: Arc<AtomicU64>,
    fail: bool,
}

#[async_trait]
impl PingTransport for MockPing {
    async fn ping(&self, _url: &str) -> Result<(), OpsError> {
        self.pings.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(OpsError::Http { status: 503 })
        } else {
            Ok(())
        }
    }
}

fn at(secs: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(1_781_000_000_000 + secs * 1000).unwrap()
}

fn pinger(pings: Arc<AtomicU64>, fail: bool) -> DeadmanPinger {
    DeadmanPinger::new(
        "https://hc.example/never-hit".to_string(),
        &DeadmanConfig {
            ping_interval_secs: 60,
        },
        Box::new(MockPing { pings, fail }),
    )
    .unwrap()
}

#[tokio::test]
async fn pings_on_first_tick_then_only_after_an_interval() {
    let pings = Arc::new(AtomicU64::new(0));
    let mut p = pinger(pings.clone(), false);

    // First tick: due (never pinged) -> ping.
    assert!(deadman_tick(&mut p, at(0), |_| panic!("no failure")).await);
    assert_eq!(pings.load(Ordering::SeqCst), 1);

    // 30s later: NOT due (interval 60s) -> no ping.
    assert!(!deadman_tick(&mut p, at(30), |_| panic!("no failure")).await);
    assert_eq!(pings.load(Ordering::SeqCst), 1);

    // 60s after the recorded ping: due -> ping.
    assert!(deadman_tick(&mut p, at(60), |_| panic!("no failure")).await);
    assert_eq!(pings.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn ping_failure_escalates_and_does_not_record() {
    let pings = Arc::new(AtomicU64::new(0));
    let mut p = pinger(pings.clone(), true);
    let mut escalations = 0u32;

    assert!(deadman_tick(&mut p, at(0), |_e| escalations += 1).await);
    assert_eq!(pings.load(Ordering::SeqCst), 1);
    assert_eq!(escalations, 1, "the failed ping escalated");

    // Failure did NOT record_ping, so the next tick is STILL due (we keep
    // trying to reach the monitor rather than backing off silently).
    assert!(deadman_tick(&mut p, at(1), |_e| escalations += 1).await);
    assert_eq!(escalations, 2);
}
