//! Stream ingestion layer: venue-generic StreamEvents + the Kalshi
//! websocket message parser/assembler (doc-derived per
//! docs/research/venue/kalshi-api-2026-06-10 — verbatim official
//! examples; the live socket dial stays fixture-gated like the REST
//! adapter).
//!
//! Doctrine under test:
//! - The parser maps documented frames into canonical YES-space stream
//!   events; sub-cent prices are REJECTED (cents-only core), unknown
//!   frame types are ignored loudly-but-safely, errors surface.
//! - Sequence gaps are DETECTED: trading on a torn book is forbidden;
//!   a gap demands a resubscribe (the composition's job).
//! - The BookAssembler folds snapshot+deltas into a canonical OrderBook
//!   (bids best-first desc, asks asc); empty levels disappear.

use fortuna_core::market::MarketId;
use fortuna_core::money::Cents;
use fortuna_venues::kalshi::ws::{subscribe_orderbook_cmd, KalshiWsEvent, KalshiWsParser};
use fortuna_venues::stream::{BookAssembler, StreamEvent};

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

#[test]
fn subscribe_command_follows_the_documented_shape_and_forces_yes_pricing() {
    let cmd = subscribe_orderbook_cmd(1, &["FED-23DEC-T3.00"]);
    assert_eq!(cmd["id"], 1);
    assert_eq!(cmd["cmd"], "subscribe");
    assert_eq!(cmd["params"]["channels"][0], "orderbook_delta");
    assert_eq!(cmd["params"]["channels"][1], "trade");
    assert_eq!(cmd["params"]["market_tickers"][0], "FED-23DEC-T3.00");
    // Both sides on the yes scale — the no-leg default is the trap the
    // research flags; we never parse no-leg pricing.
    assert_eq!(cmd["params"]["use_yes_price"], true);
}

#[test]
fn snapshot_delta_and_trade_frames_parse_to_canonical_yes_space() {
    let mut parser = KalshiWsParser::new();

    // Subscribe ack (doc-verbatim shape).
    let ack = parser
        .parse_frame(r#"{ "id": 1, "type": "subscribed", "msg": { "channel": "orderbook_delta", "sid": 2 } }"#)
        .unwrap();
    assert!(matches!(ack, KalshiWsEvent::Subscribed { sid: 2, .. }));

    // Snapshot (doc-verbatim example, yes-scale both sides under
    // use_yes_price): yes side = bids, no side = asks.
    let snap = parser
        .parse_frame(
            r#"{ "type": "orderbook_snapshot", "sid": 2, "seq": 2,
                 "msg": { "market_ticker": "FED-23DEC-T3.00",
                          "market_id": "9b0f6b43-5b68-4f9f-9f02-9a2d1b8ac1a1",
                          "yes_dollars_fp": [["0.0800", "300.00"], ["0.2200", "333.00"]],
                          "no_dollars_fp":  [["0.5400", "20.00"],  ["0.5600", "146.00"]] } }"#,
        )
        .unwrap();
    let KalshiWsEvent::Stream(StreamEvent::BookSnapshot {
        market,
        yes_bids,
        yes_asks,
    }) = snap
    else {
        panic!("expected snapshot, got {snap:?}");
    };
    assert_eq!(market, mkt("FED-23DEC-T3.00"));
    // Bids best-first (desc), asks best-first (asc).
    assert_eq!(yes_bids[0].price, Cents::new(22));
    assert_eq!(yes_bids[0].qty.raw(), 333);
    assert_eq!(yes_bids[1].price, Cents::new(8));
    assert_eq!(yes_asks[0].price, Cents::new(54));
    assert_eq!(yes_asks[0].qty.raw(), 20);

    // Delta (doc-verbatim): seq 3 follows 2; side yes at 0.960 -54.
    let delta = parser
        .parse_frame(
            r#"{ "type": "orderbook_delta", "sid": 2, "seq": 3,
                 "msg": { "market_ticker": "FED-23DEC-T3.00",
                          "market_id": "9b0f6b43-5b68-4f9f-9f02-9a2d1b8ac1a1",
                          "price_dollars": "0.960", "delta_fp": "-54.00", "side": "yes",
                          "ts": "2022-11-22T20:44:01Z", "ts_ms": 1669149841000 } }"#,
        )
        .unwrap();
    let KalshiWsEvent::Stream(StreamEvent::BookDelta {
        yes_price,
        delta_contracts,
        side,
        ..
    }) = delta
    else {
        panic!("expected delta, got {delta:?}");
    };
    assert_eq!(yes_price, Cents::new(96));
    assert_eq!(delta_contracts, -54);
    assert_eq!(side, fortuna_core::market::Side::Yes);

    // Public trade (spec-verbatim example).
    let trade = parser
        .parse_frame(
            r#"{ "type": "trade", "sid": 2, "msg": {
                  "trade_id": "d91bc706-ee49-470d-82d8-11418bda6fed",
                  "market_ticker": "FED-23DEC-T3.00",
                  "yes_price_dollars": "0.360", "no_price_dollars": "0.640",
                  "count_fp": "136.00", "taker_side": "no",
                  "ts": 1669149841, "ts_ms": 1669149841000 } }"#,
        )
        .unwrap();
    let KalshiWsEvent::Stream(StreamEvent::Trade {
        market,
        yes_price,
        qty,
    }) = trade
    else {
        panic!("expected trade, got {trade:?}");
    };
    assert_eq!(market, mkt("FED-23DEC-T3.00"));
    assert_eq!(yes_price, Cents::new(36));
    assert_eq!(qty, 136);
}

#[test]
fn sequence_gaps_are_detected_never_papered_over() {
    let mut parser = KalshiWsParser::new();
    parser
        .parse_frame(
            r#"{ "type": "orderbook_snapshot", "sid": 7, "seq": 10,
                 "msg": { "market_ticker": "KX-A", "yes_dollars_fp": [], "no_dollars_fp": [] } }"#,
        )
        .unwrap();
    // seq 12 after 10: a delta was LOST — the book is torn.
    let gap = parser
        .parse_frame(
            r#"{ "type": "orderbook_delta", "sid": 7, "seq": 12,
                 "msg": { "market_ticker": "KX-A", "price_dollars": "0.500",
                          "delta_fp": "1.00", "side": "yes", "ts_ms": 1 } }"#,
        )
        .unwrap();
    assert!(
        matches!(
            gap,
            KalshiWsEvent::SeqGap {
                sid: 7,
                expected: 11,
                got: 12
            }
        ),
        "{gap:?}"
    );
}

#[test]
fn sub_cent_prices_and_fractional_counts_are_rejected() {
    let snapshot = r#"{ "type": "orderbook_snapshot", "sid": 9, "seq": 0,
        "msg": { "market_ticker": "KX-D", "yes_dollars_fp": [], "no_dollars_fp": [] } }"#;

    // 96.5c: deci-cent structure — the cents-only core refuses.
    let mut parser = KalshiWsParser::new();
    parser.parse_frame(snapshot).unwrap();
    assert!(parser
        .parse_frame(
            r#"{ "type": "orderbook_delta", "sid": 9, "seq": 1,
                 "msg": { "market_ticker": "KX-D", "price_dollars": "0.965",
                          "delta_fp": "1.00", "side": "yes", "ts_ms": 1 } }"#,
        )
        .is_err());
    // Fractional contracts likewise.
    let mut parser = KalshiWsParser::new();
    parser.parse_frame(snapshot).unwrap();
    assert!(parser
        .parse_frame(
            r#"{ "type": "orderbook_delta", "sid": 9, "seq": 1,
                 "msg": { "market_ticker": "KX-D", "price_dollars": "0.960",
                          "delta_fp": "1.50", "side": "yes", "ts_ms": 1 } }"#,
        )
        .is_err());
    // And a sub-cent snapshot level refuses wholesale.
    let mut parser = KalshiWsParser::new();
    assert!(parser
        .parse_frame(
            r#"{ "type": "orderbook_snapshot", "sid": 9, "seq": 0,
                 "msg": { "market_ticker": "KX-D",
                          "yes_dollars_fp": [["0.0825", "10.00"]], "no_dollars_fp": [] } }"#,
        )
        .is_err());
}

#[test]
fn error_frames_surface_and_unknown_types_are_ignored() {
    let mut parser = KalshiWsParser::new();
    let err = parser
        .parse_frame(
            r#"{ "id": 123, "type": "error", "msg": { "code": 6, "msg": "Already subscribed" } }"#,
        )
        .unwrap();
    assert!(matches!(err, KalshiWsEvent::Error { code: 6, .. }));

    let other = parser
        .parse_frame(r#"{ "type": "market_lifecycle_v2", "sid": 1, "msg": {} }"#)
        .unwrap();
    assert!(matches!(other, KalshiWsEvent::Ignored { .. }));
}

#[test]
fn the_assembler_folds_snapshot_and_deltas_into_a_canonical_book() {
    use fortuna_core::clock::UtcTimestamp;
    let at = UtcTimestamp::parse_iso8601("2026-06-11T12:00:00.000Z").unwrap();
    let mut asm = BookAssembler::new();

    let book = asm
        .apply(
            StreamEvent::BookSnapshot {
                market: mkt("KX-A"),
                yes_bids: vec![fortuna_venues::PriceLevel {
                    price: Cents::new(55),
                    qty: fortuna_core::market::Contracts::new(50),
                }],
                yes_asks: vec![fortuna_venues::PriceLevel {
                    price: Cents::new(60),
                    qty: fortuna_core::market::Contracts::new(40),
                }],
            },
            at,
        )
        .unwrap()
        .expect("snapshot yields a book");
    assert_eq!(book.yes_bids[0].price, Cents::new(55));

    // Delta: add 10 to the bid at 55, open a new bid at 54, then remove
    // the ask entirely.
    asm.apply(
        StreamEvent::BookDelta {
            market: mkt("KX-A"),
            side: fortuna_core::market::Side::Yes,
            yes_price: Cents::new(55),
            delta_contracts: 10,
        },
        at,
    )
    .unwrap();
    asm.apply(
        StreamEvent::BookDelta {
            market: mkt("KX-A"),
            side: fortuna_core::market::Side::Yes,
            yes_price: Cents::new(54),
            delta_contracts: 25,
        },
        at,
    )
    .unwrap();
    let book = asm
        .apply(
            StreamEvent::BookDelta {
                market: mkt("KX-A"),
                side: fortuna_core::market::Side::No,
                yes_price: Cents::new(60),
                delta_contracts: -40,
            },
            at,
        )
        .unwrap()
        .expect("deltas yield the updated book");

    assert_eq!(book.yes_bids[0].price, Cents::new(55));
    assert_eq!(book.yes_bids[0].qty.raw(), 60);
    assert_eq!(book.yes_bids[1].price, Cents::new(54));
    assert!(book.yes_asks.is_empty(), "emptied level disappears");

    // A delta for an unsnapshotted market is a torn-state error.
    assert!(asm
        .apply(
            StreamEvent::BookDelta {
                market: mkt("KX-UNSEEN"),
                side: fortuna_core::market::Side::Yes,
                yes_price: Cents::new(50),
                delta_contracts: 1,
            },
            at,
        )
        .is_err());

    // Negative beyond zero clamps... no: OVERDRAW is a torn state too.
    assert!(asm
        .apply(
            StreamEvent::BookDelta {
                market: mkt("KX-A"),
                side: fortuna_core::market::Side::Yes,
                yes_price: Cents::new(55),
                delta_contracts: -1_000,
            },
            at,
        )
        .is_err());
}
