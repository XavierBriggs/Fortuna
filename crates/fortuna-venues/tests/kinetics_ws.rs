//! T5.B4 slice-4 tests: the kinetics WS session layer vs the RECORDED
//! streams. Both captured .jsonl files replay end-to-end with zero
//! errors, zero unknown frames, and ZERO sequence gaps (the recordings
//! are continuous); the snapshot normalizes the recorded worst->best
//! ordering; the subscribe builders equal the recorder's accepted
//! commands byte-for-byte as JSON; synthetic gap/torn scenarios pin the
//! resync discipline.

use fortuna_core::market::Contracts;
use fortuna_core::perp::PerpPrice;
use fortuna_venues::kinetics::client::BookSide;
use fortuna_venues::kinetics::ws::{
    subscribe_book_cmd, subscribe_private_cmd, subscribe_ticker_cmd, KineticsWsEvent,
    KineticsWsSession, MARGIN_WS_DEMO_URL, MARGIN_WS_SIGNING_PATH,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/kinetics-perps")
}

fn replay(file: &str) -> (Vec<KineticsWsEvent>, BTreeMap<&'static str, usize>) {
    let raw = fs::read_to_string(fixtures_dir().join(file)).expect("ws fixture");
    let mut session = KineticsWsSession::new();
    let mut events = Vec::new();
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let event = session
            .parse_frame(line)
            .unwrap_or_else(|e| panic!("{file}: {e}\n{line}"));
        let key = match &event {
            KineticsWsEvent::Subscribed { .. } => "subscribed",
            KineticsWsEvent::BookSnapshot { .. } => "snapshot",
            KineticsWsEvent::BookDelta { .. } => "delta",
            KineticsWsEvent::Ticker { .. } => "ticker",
            KineticsWsEvent::Trade { .. } => "trade",
            KineticsWsEvent::UserOrder { .. } => "user_order",
            KineticsWsEvent::GroupUpdate { .. } => "group_update",
            KineticsWsEvent::SeqGap { .. } => "seq_gap",
            KineticsWsEvent::Ignored { .. } => "ignored",
        };
        *counts.entry(key).or_insert(0) += 1;
        events.push(event);
    }
    (events, counts)
}

#[test]
fn public_stream_replays_with_zero_gaps_and_zero_unknowns() {
    let (events, counts) = replay("ws__public_orderbook_ticker.jsonl");
    assert_eq!(
        counts.get("seq_gap"),
        None,
        "recorded stream must be gapless"
    );
    assert_eq!(
        counts.get("ignored"),
        None,
        "recorded stream must be fully typed"
    );
    assert_eq!(counts.get("snapshot").copied(), Some(1));
    assert_eq!(counts.get("subscribed").copied(), Some(3));
    assert!(counts.get("delta").copied().unwrap_or(0) > 2_000);
    assert!(counts.get("trade").copied().unwrap_or(0) > 1_000);
    assert!(counts.get("ticker").copied().unwrap_or(0) > 10);

    // The snapshot normalized the recorded worst->best ordering.
    let snapshot = events
        .iter()
        .find_map(|e| match e {
            KineticsWsEvent::BookSnapshot { bids, asks, .. } => Some((bids, asks)),
            _ => None,
        })
        .expect("one snapshot");
    let (bids, asks) = snapshot;
    assert!(bids.windows(2).all(|w| w[0].0 >= w[1].0), "bids best-first");
    assert!(asks.windows(2).all(|w| w[0].0 <= w[1].0), "asks best-first");
    assert!(bids[0].0 < asks[0].0, "normalized book is uncrossed");
}

#[test]
fn private_stream_replays_and_types_the_lifecycle() {
    let (events, counts) = replay("ws__private_lifecycle.jsonl");
    assert_eq!(counts.get("ignored"), None);
    assert_eq!(counts.get("subscribed").copied(), Some(3));
    assert!(counts.get("user_order").copied().unwrap_or(0) > 0);
    assert!(counts.get("group_update").copied().unwrap_or(0) > 0);

    // The first user_order echoes the recorded GTC create.
    let order = events
        .iter()
        .find_map(|e| match e {
            KineticsWsEvent::UserOrder {
                order_id,
                client_order_id,
                side,
                price,
                remaining_count,
                ..
            } => Some((order_id, client_order_id, side, price, remaining_count)),
            _ => None,
        })
        .expect("at least one user_order");
    assert_eq!(order.0, "c445aeac-f95b-4c96-8086-faacebfd300d");
    assert_eq!(order.1, "99845c0f-725c-4a4a-8955-a95a30e58072");
    assert_eq!(*order.2, BookSide::Bid);
    assert_eq!(*order.3, PerpPrice::new(53_829));
    assert_eq!(*order.4, Contracts::new(1));

    // Group updates: "created" carries the limit; "triggered" omits it
    // (recorded) and must type as None, not error.
    let limits: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            KineticsWsEvent::GroupUpdate {
                event_type,
                contracts_limit,
                ..
            } => Some((event_type.clone(), *contracts_limit)),
            _ => None,
        })
        .collect();
    assert!(limits
        .iter()
        .any(|(t, l)| t == "created" && *l == Some(Contracts::new(10))));
    assert!(limits.iter().any(|(t, l)| t == "triggered" && l.is_none()));
}

#[test]
fn ticker_event_carries_funding_and_all_three_marks() {
    let (events, _) = replay("ws__public_orderbook_ticker.jsonl");
    let ticker = events
        .iter()
        .find_map(|e| match e {
            KineticsWsEvent::Ticker {
                next_funding_time_ms,
                settlement_mark,
                liquidation_mark,
                reference_price,
                ..
            } => Some((
                *next_funding_time_ms,
                *settlement_mark,
                *liquidation_mark,
                *reference_price,
            )),
            _ => None,
        })
        .expect("ticker");
    // Funding grid: next funding time lands on the 04/12/20 UTC schedule.
    assert_eq!(ticker.0 % 28_800_000, 14_400_000);
    assert!(ticker.1.raw() > 0 && ticker.2.raw() > 0 && ticker.3.raw() > 0);
}

#[test]
fn subscribe_commands_equal_the_recorders_accepted_shapes() {
    assert_eq!(
        subscribe_book_cmd(1, &["KXBTCPERP1"]),
        serde_json::json!({
            "id": 1,
            "cmd": "subscribe",
            "params": {"channels": ["orderbook_delta", "trade"], "market_tickers": ["KXBTCPERP1"]},
        })
    );
    assert_eq!(
        subscribe_ticker_cmd(2, &["KXBTCPERP1"]),
        serde_json::json!({
            "id": 2,
            "cmd": "subscribe",
            "params": {
                "channels": ["ticker"],
                "market_tickers": ["KXBTCPERP1"],
                "send_initial_snapshot": true,
            },
        })
    );
    assert_eq!(
        subscribe_private_cmd(1),
        serde_json::json!({
            "id": 1,
            "cmd": "subscribe",
            "params": {"channels": ["user_orders", "fill", "order_group_updates"]},
        })
    );
}

#[test]
fn handshake_constants_match_the_recordings_and_research() {
    // Finding 2 (SETTLED): sign the URL path itself; demo host recorded.
    assert_eq!(MARGIN_WS_SIGNING_PATH, "/trade-api/ws/v2/margin");
    assert_eq!(
        MARGIN_WS_DEMO_URL,
        "wss://external-api-margin-ws.demo.kalshi.co/trade-api/ws/v2/margin"
    );
}

// ---- synthetic sequence discipline (the resync contract) ----

fn snapshot_frame(sid: i64, seq: i64) -> String {
    format!(
        r#"{{"type":"orderbook_snapshot","sid":{sid},"seq":{seq},"msg":{{"market_ticker":"KXBTCPERP1","bid":[["6.0000","1.00"]],"ask":[["6.1000","1.00"]]}}}}"#
    )
}

fn delta_frame(sid: i64, seq: i64) -> String {
    format!(
        r#"{{"type":"orderbook_delta","sid":{sid},"seq":{seq},"msg":{{"market_ticker":"KXBTCPERP1","price":"6.0000","delta":"1.00","side":"bid","ts_ms":1}}}}"#
    )
}

#[test]
fn seq_gap_reports_and_does_not_advance_until_fresh_snapshot() {
    let mut s = KineticsWsSession::new();
    assert!(matches!(
        s.parse_frame(&snapshot_frame(1, 5)).unwrap(),
        KineticsWsEvent::BookSnapshot { .. }
    ));
    assert!(matches!(
        s.parse_frame(&delta_frame(1, 6)).unwrap(),
        KineticsWsEvent::BookDelta { .. }
    ));
    // Lost delta 7: gap reported.
    let gap = s.parse_frame(&delta_frame(1, 8)).unwrap();
    let KineticsWsEvent::SeqGap { expected, got, .. } = gap else {
        panic!("expected SeqGap, got {gap:?}");
    };
    assert_eq!((expected, got), (7, 8));
    // The baseline did NOT advance: the next delta still reports.
    assert!(matches!(
        s.parse_frame(&delta_frame(1, 9)).unwrap(),
        KineticsWsEvent::SeqGap { expected: 7, .. }
    ));
    // A fresh snapshot rebaselines; flow resumes.
    assert!(matches!(
        s.parse_frame(&snapshot_frame(1, 9)).unwrap(),
        KineticsWsEvent::BookSnapshot { .. }
    ));
    assert!(matches!(
        s.parse_frame(&delta_frame(1, 10)).unwrap(),
        KineticsWsEvent::BookDelta { .. }
    ));
}

#[test]
fn delta_before_snapshot_is_torn_from_the_start() {
    let mut s = KineticsWsSession::new();
    assert!(matches!(
        s.parse_frame(&delta_frame(3, 1)).unwrap(),
        KineticsWsEvent::SeqGap { expected: 0, .. }
    ));
}

#[test]
fn unknown_frame_type_is_ignored_not_an_error() {
    let mut s = KineticsWsSession::new();
    let event = s
        .parse_frame(r#"{"type":"future_channel","msg":{}}"#)
        .unwrap();
    let KineticsWsEvent::Ignored { frame_type } = event else {
        panic!("expected Ignored, got {event:?}");
    };
    assert_eq!(frame_type, "future_channel");
}
