//! T4.2 item 2(iv) — kill-switch Kalshi plug, MACHINERY proof. The I4
//! freeze-and-cancel logic is venue-agnostic (`freeze_and_cancel(&dyn Venue,
//! ...)`); this proves it works over the REAL `KalshiVenue` adapter driven by a
//! MOCK transport (NO live socket), cancelling every open order through the
//! adapter's `open_orders` + `cancel` (DELETE + reconcile-GET → canceled) path.
//!
//! I4-safe: mock transport + `futures::executor::block_on` (no tokio runtime),
//! and `fortuna-venues` is ALREADY a fortuna-killswitch dependency — so this
//! adds ZERO new crate to the killswitch dep graph (the i4_killswitch_
//! independence invariant test stays green; forbidden = sqlx/postgres/ledger/
//! cognition, none added).
//!
//! The LIVE `freeze --venue kalshi` wiring (FORTUNA_KILLSWITCH_* creds +
//! ReqwestKalshiTransport on a current-thread tokio runtime) is ledgered in
//! GAPS as the next slice; its first live exercise is operator-run AFTER the
//! 27-item paper clearance (the switch must not take its first real cancel path
//! through unverified venue code).
//!
//! NOTE on the open-orders page: the recorded 2026-06-11 session's orders were
//! executed/canceled by capture time, so the open page here is SCRIPTED resting
//! orders built from the recorded `KalshiOrder` shape (status set `resting`) to
//! exercise the freeze-cancel path; the cancel acks + reconcile GETs use the
//! recorded cancel/order shapes verbatim.

use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::market::VenueId;
use fortuna_killswitch::freeze_and_cancel;
use fortuna_venues::kalshi::client::{KalshiTransport, MockKalshiTransport};
use fortuna_venues::kalshi::KalshiVenue;
use futures::executor::block_on;
use serde_json::json;
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-11T06:31:00.000Z").unwrap()
}

fn recorded(name: &str) -> serde_json::Value {
    let path = format!(
        "{}/../../fixtures/kalshi/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse fixture {path}: {e}"))
}

#[test]
fn killswitch_freezes_a_kalshi_venue_cancelling_every_open_order() {
    let order_ids = [
        "2597b999-f887-4195-8bac-c3f97a1f2021",
        "97ec18b7-10d3-4557-9de0-8598aad625f0",
    ];

    // open_orders() page: two RESTING orders (recorded KalshiOrder shape).
    let resting: Vec<serde_json::Value> = order_ids
        .iter()
        .map(|oid| {
            let mut o = recorded("orders__get_after_cancel.json")["order"].clone();
            o["order_id"] = json!(oid);
            o["status"] = json!("resting");
            o["remaining_count_fp"] = json!("1.00");
            o
        })
        .collect();

    let mock = Arc::new(MockKalshiTransport::new());
    mock.push_ok(200, json!({ "orders": resting, "cursor": "" }));
    // Each cancel: DELETE ack (advisory, ignored) + reconcile GET → canceled.
    for oid in order_ids {
        mock.push_ok(200, recorded("orders__cancel_v2.json"));
        let mut canceled = recorded("orders__get_after_cancel.json");
        canceled["order"]["order_id"] = json!(oid);
        canceled["order"]["status"] = json!("canceled");
        mock.push_ok(200, canceled);
    }

    let venue = KalshiVenue::new(
        VenueId::new("kalshi").unwrap(),
        Arc::clone(&mock) as Arc<dyn KalshiTransport>,
        Arc::new(SimClock::new(t0())),
        vec![],
    )
    .unwrap();

    let journal = std::env::temp_dir().join("fortuna-ks-kalshi-freeze.jsonl");
    let _ = std::fs::remove_file(&journal);
    let clock = SimClock::new(t0());

    let report = block_on(freeze_and_cancel(&venue, &clock, &journal)).unwrap();

    // Every open order cancelled through the real KalshiVenue cancel path.
    assert_eq!(report.orders_seen, 2);
    assert_eq!(report.orders_cancelled, 2);
    assert_eq!(report.orders_cancel_failed, 0);

    // The switch's flat-file journal records the freeze (I4: reconstructable
    // even with the audit store down).
    let body = std::fs::read_to_string(&journal).unwrap();
    assert!(
        body.contains("freeze_and_cancel_started"),
        "journal: {body}"
    );
    assert!(body.contains("\"orders_cancelled\":2"), "journal: {body}");
    let _ = std::fs::remove_file(&journal);

    // The DELETE-then-reconcile-GET shape was actually exercised per order:
    // open_orders (1) + 2 × (DELETE + GET) = 5 transport calls.
    assert_eq!(
        mock.calls().len(),
        5,
        "open_orders + 2×(DELETE+reconcile GET)"
    );
}
