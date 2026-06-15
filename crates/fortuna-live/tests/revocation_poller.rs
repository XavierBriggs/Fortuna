//! I4 revocation (open audit C2): the RevocationHaltPoller wrapper behavior.
//! Sentinel PRESENT => the wrapper reports the standing revocation halt and the
//! inner poller is NOT consulted; sentinel ABSENT => the wrapper delegates to
//! the inner poller verbatim (Ok(None) here, but an inner halt/error would pass
//! through unchanged too). A unique temp dir per test isolates the sentinel.

use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_killswitch::{clear_revocation, revocation_path, write_revocation};
use fortuna_live::run_loop::{HaltPoller, RevocationHaltPoller};
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fortuna-revoke-poller-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An inner poller that records whether it was consulted and yields a DISTINCT
/// signal, so the test can prove the wrapper short-circuits when revoked (the
/// inner's signal must NOT appear) and delegates when clear (it MUST appear).
struct SpyInner {
    consulted: bool,
    yields: Result<Option<String>, String>,
}
impl HaltPoller for SpyInner {
    async fn poll(&mut self) -> Result<Option<String>, String> {
        self.consulted = true;
        self.yields.clone()
    }
}

#[tokio::test]
async fn present_sentinel_halts_and_skips_inner() {
    let dir = unique_dir("present");
    let journal = dir.join("k.jsonl");
    let sentinel = revocation_path(&journal);
    let clock = Arc::new(SimClock::new(t0()));
    write_revocation(&sentinel, clock.as_ref() as &dyn Clock, "freeze_and_cancel").unwrap();

    let mut poller = RevocationHaltPoller {
        revocation_file: sentinel.clone(),
        // The inner yields a DIFFERENT halt; if the wrapper delegated, we'd see it.
        inner: SpyInner {
            consulted: false,
            yields: Ok(Some("inner-distinct-halt".to_string())),
        },
    };
    let out = poller.poll().await;
    match out {
        Ok(Some(r)) => {
            assert!(r.contains("revocation") && r.contains("I4"), "got {r:?}");
            assert!(
                !r.contains("inner-distinct-halt"),
                "the revocation reason wins over the inner: {r:?}"
            );
        }
        other => panic!("present sentinel must halt, got {other:?}"),
    }
    assert!(
        !poller.inner.consulted,
        "a present sentinel short-circuits: the inner poller is NOT consulted"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn absent_sentinel_delegates_to_inner() {
    let dir = unique_dir("absent");
    let journal = dir.join("k.jsonl");
    let sentinel = revocation_path(&journal);
    // Ensure absent (idempotent clear on a never-written sentinel is Ok).
    clear_revocation(&sentinel).unwrap();

    let mut poller = RevocationHaltPoller {
        revocation_file: sentinel.clone(),
        inner: SpyInner {
            consulted: false,
            yields: Ok(None),
        },
    };
    assert_eq!(
        poller.poll().await,
        Ok(None),
        "absent sentinel delegates to the (no-halt) inner"
    );
    assert!(
        poller.inner.consulted,
        "an absent sentinel consults the inner poller"
    );

    // And an inner HALT passes through unchanged when the sentinel is absent.
    let mut with_inner_halt = RevocationHaltPoller {
        revocation_file: sentinel.clone(),
        inner: SpyInner {
            consulted: false,
            yields: Ok(Some("operator halt (durable store)".to_string())),
        },
    };
    assert_eq!(
        with_inner_halt.poll().await,
        Ok(Some("operator halt (durable store)".to_string())),
        "absent sentinel => the inner's own halt passes through verbatim"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
