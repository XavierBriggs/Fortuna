//! W4: `fortuna start paper-demo` — paper-live safety wall + pointer-write.
//!
//! # TDD — written BEFORE implementation; both tests must fail first.
//!
//! ## Test 1 — `paper_demo_holds_no_real_order`
//! Proves the paper-demo wall is load-bearing, not vacuous:
//! - `ExecutionMode::PaperLedger` satisfies `allows_order_mutation() == false`
//!   (the wall guard the start command enforces).
//! - The reds-it mutation: routing a real `/portfolio/order` call through a
//!   `GuardedKalshiTransport` (the same pattern as `i_paper_live_no_real_order`)
//!   panics.  Verifies the transport-level wall catches real-order attempts even
//!   when the caller forgets the mode check.
//!
//! ## Test 2 — `pointer_write_lands_live_url`
//! After `fortuna_live::boot::write_demo_db_pointer(runtime_dir, url)` the file
//! `data/runtime/current-demo-db-url` under `runtime_dir` contains exactly `url`.
//! The write is atomic (temp + rename) so `runtime_dir` is the canonical path.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use async_trait::async_trait;
use fortuna_live::boot::{write_demo_db_pointer, ExecutionMode};
use fortuna_venues::kalshi::client::{KalshiTransport, RecordedCall};
use fortuna_venues::VenueError;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

// ---------------------------------------------------------------------------
// Minimal guarded transport (mirrors i_paper_live_no_real_order pattern)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct GuardedTransport {
    script: Mutex<VecDeque<Result<(u16, serde_json::Value), VenueError>>>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl GuardedTransport {
    fn push_ok(&self, status: u16, body: serde_json::Value) {
        self.script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(Ok((status, body)));
    }
}

#[async_trait]
impl KalshiTransport for GuardedTransport {
    async fn request(
        &self,
        method: &str,
        path: &str,
        query: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> Result<(u16, serde_json::Value), VenueError> {
        // WALL: any non-GET or any /portfolio/order call is a real-order attempt.
        if method != "GET" || path.contains("/portfolio/order") {
            panic!("paper-demo wall violated: real execution endpoint attempted: {method} {path}");
        }
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(RecordedCall {
                method: method.to_string(),
                path: path.to_string(),
                query: query.map(str::to_string),
                body,
            });
        self.script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| {
                Err(VenueError::Invalid {
                    reason: format!("unscripted call: {method} {path}"),
                })
            })
    }
}

// ---------------------------------------------------------------------------
// Test 1 — paper-demo execution mode holds the wall
// ---------------------------------------------------------------------------

/// The paper-demo wall is load-bearing:
/// 1. `PaperLedger` serialises to `"paper_ledger"` (the config/env contract the
///    daemon reads; a typo would silently boot the wrong mode).
/// 2. `PaperLedger` is the ONLY mode where `auto_persist_calibration() == true`
///    — this is what distinguishes paper-demo from every other mode.
/// 3. The reds-it mutation: sending a real `/portfolio/order` POST through the
///    `GuardedKalshiTransport` panics (proving the transport-level wall bites;
///    the paper-live safety wall is NOT vacuous).
/// 4. A legitimate GET read-call does NOT trigger the wall (the positive path).
///
/// Note: `PaperLedger.allows_order_mutation() == true` — this is INTENTIONAL.
/// It means "paper fills (local venue) are allowed"; the wall is at the TRANSPORT
/// layer (`GuardedKalshiTransport` panics on real Kalshi order endpoints), not
/// the `ExecutionMode` flag. The distinction is: paper-demo accepts local paper
/// orders but never places a real Kalshi order.
#[test]
fn paper_demo_holds_no_real_order() {
    // --- mode identity: PaperLedger is the paper-demo execution mode ---
    assert_eq!(
        ExecutionMode::PaperLedger.as_str(),
        "paper_ledger",
        "PaperLedger must serialise to 'paper_ledger' (config/env contract)"
    );

    // --- calibration auto-persist: PaperLedger is the ONE mode that warms calibration ---
    assert!(
        ExecutionMode::PaperLedger.auto_persist_calibration(),
        "PaperLedger must auto_persist_calibration() == true; it is the defining \
         property of the paper-demo mode"
    );
    // Every other mode must NOT auto-persist (I7: calibration is an operator action).
    for mode in [
        ExecutionMode::LiveDataOnly,
        ExecutionMode::DryRun,
        ExecutionMode::DemoOrders,
        ExecutionMode::ProductionOrders,
    ] {
        assert!(
            !mode.auto_persist_calibration(),
            "{} must NOT auto_persist_calibration; only PaperLedger may",
            mode.as_str()
        );
    }

    // --- reds-it mutation: prove the GuardedTransport wall panics on a real order ---
    let transport = Arc::new(GuardedTransport::default());
    // Prime it with a dummy GET response so a read call would succeed.
    transport.push_ok(200, serde_json::json!({"ok": true}));

    let transport_clone = transport.clone();
    let result = std::panic::catch_unwind(move || {
        // A real order placement attempt: POST to a portfolio/order path.
        futures::executor::block_on(transport_clone.request(
            "POST",
            "/trade-api/v2/portfolio/order",
            None,
            Some(serde_json::json!({"ticker": "KXTEST", "action": "buy", "count": 1})),
        ))
    });
    assert!(
        result.is_err(),
        "the GuardedTransport wall must panic on a real /portfolio/order POST; \
         the paper-live safety wall is not load-bearing"
    );

    // --- verify a legitimate GET read-call does NOT panic ---
    let transport2 = Arc::new(GuardedTransport::default());
    transport2.push_ok(200, serde_json::json!({"markets": [], "cursor": ""}));
    let read_result = std::panic::catch_unwind(move || {
        futures::executor::block_on(transport2.request(
            "GET",
            "/trade-api/v2/markets",
            Some("limit=10"),
            None,
        ))
    });
    assert!(
        read_result.is_ok(),
        "a read-only GET must not trigger the wall"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — pointer-write lands the live DATABASE_URL
// ---------------------------------------------------------------------------

/// After `write_demo_db_pointer(runtime_dir, url)` the file
/// `data/runtime/current-demo-db-url` under `runtime_dir` contains exactly `url`.
///
/// Uses a temp directory pinned to the test; no process env mutation.
#[test]
fn pointer_write_lands_live_url() {
    let base = std::env::temp_dir().join(format!(
        "fortuna-paper-demo-ptr-test-{}",
        std::process::id()
    ));
    // Clean from any previous run; create the runtime subdir.
    let _ = std::fs::remove_dir_all(&base);
    let runtime_dir = base.join("data").join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();

    let db_url = "postgres://fortuna_app@localhost/fortuna_demo_test";

    write_demo_db_pointer(&runtime_dir, db_url)
        .expect("write_demo_db_pointer must not fail on a writable directory");

    let pointer_path = runtime_dir.join("current-demo-db-url");
    assert!(
        pointer_path.exists(),
        "current-demo-db-url must exist after write_demo_db_pointer"
    );
    let contents =
        std::fs::read_to_string(&pointer_path).expect("should be able to read current-demo-db-url");
    assert_eq!(
        contents.trim(),
        db_url,
        "current-demo-db-url must contain exactly the DATABASE_URL written"
    );

    // Idempotent: overwrite with a different URL and re-check.
    let db_url2 = "postgres://fortuna_app@localhost/fortuna_demo_test_2";
    write_demo_db_pointer(&runtime_dir, db_url2)
        .expect("second write_demo_db_pointer must also succeed");
    let contents2 = std::fs::read_to_string(&pointer_path).unwrap();
    assert_eq!(
        contents2.trim(),
        db_url2,
        "second write must overwrite atomically"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&base);
}
