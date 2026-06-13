//! `KineticsPerpObservation::from_ws_ticker` — the venue-side `PerpTick` half
//! built VERBATIM from a WS `ticker` frame (the producer adds the `venue` id to
//! make the bus event). Written from the field-mapping spec BEFORE the builder.
//!
//! SYNTHETIC frames pin the EXACT verbatim mapping (and catch field-swaps,
//! e.g. settlement↔reference); the recorded FIXTURE frame proves it builds on
//! real demo data, re-deriving each field through the same parse path so no
//! capture-specific value is pinned (the corpus re-records — same discipline as
//! `kinetics_dto.rs`).

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::MarketId;
use fortuna_core::perp::PerpPrice;
use fortuna_venues::kinetics::dto::{self, WsFrame};
use fortuna_venues::kinetics::perp_observation::KineticsPerpObservation;
use rust_decimal::Decimal;
use std::path::PathBuf;

/// A complete WS `ticker` frame with the given load-bearing values; the
/// non-load-bearing fields carry plausible fixed values (the builder ignores
/// them). `rate` is injected RAW (a JSON number).
fn ticker_frame(
    market: &str,
    settlement: &str,
    reference: &str,
    rate: &str,
    next_funding_time_ms: i64,
    ts_ms: i64,
) -> String {
    format!(
        r#"{{"type":"ticker","sid":7,"msg":{{
            "market_ticker":"{market}","price":"6.5000","bid":"6.4998","ask":"6.5002",
            "bid_size_fp":"1.00","ask_size_fp":"1.00",
            "funding_rate":{{"rate":{rate},"next_funding_time_ms":{next_funding_time_ms},"ts_ms":{ts_ms}}},
            "reference_price":{{"price":"{reference}","ts_ms":{ts_ms}}},
            "settlement_mark_price":{{"price":"{settlement}","ts_ms":{ts_ms}}},
            "liquidation_mark_price":{{"price":"6.5100","ts_ms":{ts_ms}}},
            "ts_ms":{ts_ms}
        }}}}"#
    )
}

fn build(frame_json: &str) -> Result<KineticsPerpObservation, fortuna_venues::VenueError> {
    let WsFrame::Ticker { msg, .. } = dto::parse_ws_frame(frame_json).expect("ticker frame parses")
    else {
        panic!("expected a ticker frame");
    };
    KineticsPerpObservation::from_ws_ticker(&msg)
}

// ─── 1. exact verbatim mapping (distinct values catch a field-swap) ──────────

// MUTATION: map `settlement_mark_price` to `reference_price` (or swap them) →
// the two distinct prices land in the wrong fields → these assertions red.
#[test]
fn maps_every_ticker_field_verbatim() {
    // settlement 6.5000 → 65000 ; reference 6.4900 → 64900 (DISTINCT, so a
    // settlement↔reference swap is caught). rate 0.0005 ; next 1781294400000 ;
    // ts 1781267870012.
    let obs = build(&ticker_frame(
        "KXBTCPERP",
        "6.5000",
        "6.4900",
        "0.0005",
        1_781_294_400_000,
        1_781_267_870_012,
    ))
    .expect("a well-formed ticker builds");

    assert_eq!(obs.market, MarketId::new("KXBTCPERP").unwrap());
    // settlement → venue_settlement (NOT reference); conservative mark absent.
    assert_eq!(obs.marks.venue_settlement, PerpPrice::new(65_000));
    assert_eq!(obs.marks.conservative, None);
    // reference → funding.reference_price (the DISTINCT price).
    assert_eq!(obs.funding.reference_price, PerpPrice::new(64_900));
    assert_ne!(
        obs.marks.venue_settlement, obs.funding.reference_price,
        "settlement and reference are distinct prices — never the same field"
    );
    // rate f64 → Decimal estimate (re-derived through the same f64→Decimal
    // conversion the builder uses, so the assertion pins the SOURCE field +
    // conversion without depending on the exact float representation).
    assert_eq!(obs.funding.estimate, Decimal::try_from(0.0005_f64).unwrap());
    assert!(
        obs.funding.estimate > Decimal::ZERO,
        "0.0005 is a positive rate"
    );
    // next_funding_time_ms (NOT the frame ts) → next_funding_time.
    assert_eq!(
        obs.funding.next_funding_time,
        UtcTimestamp::from_epoch_millis(1_781_294_400_000).unwrap()
    );
    // the FRAME ts_ms (NOT funding_rate.ts_ms) → obs_at.
    assert_eq!(
        obs.funding.obs_at,
        UtcTimestamp::from_epoch_millis(1_781_267_870_012).unwrap()
    );
    // obs_at and next_funding_time are distinct timestamps (catch a ts swap).
    assert_ne!(obs.funding.obs_at, obs.funding.next_funding_time);
}

// A zero funding rate (the recorded demo reality) maps to Decimal::ZERO, not an
// error.
#[test]
fn zero_funding_rate_is_decimal_zero() {
    let obs = build(&ticker_frame(
        "KXBTCPERP",
        "6.5000",
        "6.5000",
        "0",
        1_781_294_400_000,
        1_781_267_870_012,
    ))
    .expect("zero rate builds");
    assert_eq!(obs.funding.estimate, Decimal::ZERO);
}

// ─── 2. real recorded frame (re-recording-proof: re-derive, never pin) ───────

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/kinetics-perps")
}

// MUTATION: any field-source swap in the builder reds the re-derivation below
// even on the (re-recordable) live values.
#[test]
fn builds_from_the_recorded_ticker_stream() {
    let raw = std::fs::read_to_string(fixture_path().join("ws__public_orderbook_ticker.jsonl"))
        .expect("ws fixture");
    // The FIRST ticker frame in the recorded public stream.
    let msg = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| match dto::parse_ws_frame(l) {
            Ok(WsFrame::Ticker { msg, .. }) => Some(msg),
            _ => None,
        })
        .next()
        .expect("at least one ticker frame in the recorded stream");

    let obs = KineticsPerpObservation::from_ws_ticker(&msg).expect("a recorded ticker builds");

    // Re-derive each field through the SAME parse path (no capture value pinned):
    assert_eq!(obs.market.as_str(), msg.market_ticker);
    assert_eq!(
        obs.marks.venue_settlement,
        dto::parse_perp_price(&msg.settlement_mark_price.price).unwrap()
    );
    assert_eq!(obs.marks.conservative, None);
    assert_eq!(
        obs.funding.reference_price,
        dto::parse_perp_price(&msg.reference_price.price).unwrap()
    );
    assert_eq!(
        obs.funding.estimate,
        Decimal::try_from(msg.funding_rate.rate).unwrap()
    );
    assert_eq!(
        obs.funding.next_funding_time,
        UtcTimestamp::from_epoch_millis(msg.funding_rate.next_funding_time_ms).unwrap()
    );
    assert_eq!(
        obs.funding.obs_at,
        UtcTimestamp::from_epoch_millis(msg.ts_ms).unwrap()
    );
    // Structural: the settlement mark is a real positive price.
    assert!(obs.marks.venue_settlement.raw() > 0);
}

// ─── 3. malformed input → Err, never panic ──────────────────────────────────

// MUTATION: `unwrap()`/`expect()` a price parse instead of `?` → this panics
// instead of returning Err.
#[test]
fn malformed_settlement_price_is_err_not_panic() {
    let frame = ticker_frame(
        "KXBTCPERP",
        "not_a_price",
        "6.5000",
        "0.0005",
        1_781_294_400_000,
        1_781_267_870_012,
    );
    assert!(
        build(&frame).is_err(),
        "a malformed settlement price must be a VenueError, not a panic"
    );
}
