//! F7 (Track-A) recorded end-to-end: the Aeolus KNYC tmax forecast matched
//! against the RECORDED June-13 KXHIGHNY book → beliefs + auto-confirmed
//! `Direct` edges, 1:1 and order-preserving.
//!
//! No DB, no network, no clock — pure replay over two committed fixtures:
//!   * `fixtures/sources/aeolus/knyc_tmax.json` (parsed via `parse_response`),
//!   * `fixtures/kalshi/markets__high_temp.json` (the 18 verbatim KXHIGHNY
//!     markets; this test uses the 6 that form the June-13 partition).
//!
//! Every ticker asserted here comes from that recorded book — NONE is
//! fabricated. The June-13 set is `determined` (settled); that is fine because
//! [`market_to_bucket`] is pure kind-derivation and does NOT filter on status
//! (the active-only filter is the caller's job), so the settled set still
//! exercises the bucket-construction + belief + edge logic.
//!
//! # Golden regression (C1 decoupling)
//! `market_to_bucket` now operates on venue-neutral [`MarketView`] (not
//! `KalshiMarket` directly). The `golden_regression_market_view_geometry`
//! test proves that `kalshi_market_to_market_view` + `market_to_bucket`
//! produces the EXACT SAME `WeatherBucket` (kind + market_key + floor/cap)
//! the old direct-`KalshiMarket` path did — for every representative
//! strike_type (`between`, `greater`, `less`, unknown).

use fortuna_cognition::aeolus_buckets::BucketKind;
use fortuna_cognition::aeolus_forecast::parse_response;
use fortuna_cognition::discovery::MarketView;
use fortuna_cognition::events::MappingType;
use fortuna_live::aeolus_venue::{aeolus_bucket_edges, market_to_bucket};
use fortuna_venues::kalshi::dto::KalshiMarket;
use fortuna_venues::kalshi::kalshi_market_to_market_view;

const AEOLUS_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/aeolus/knyc_tmax.json"
);
const KALSHI_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/kalshi/markets__high_temp.json"
);

/// Parse the one KNYC tmax forecast (target_date 2026-06-13) from the recorded
/// `{ "forecasts": [...] }` envelope.
fn load_forecast() -> fortuna_cognition::aeolus_forecast::AeolusForecast {
    let body = std::fs::read_to_string(AEOLUS_FIXTURE).expect("read aeolus fixture");
    let mut forecasts = parse_response(&body).expect("parse_response on recorded KNYC envelope");
    assert_eq!(forecasts.len(), 1, "fixture carries exactly one forecast");
    let fc = forecasts.remove(0);
    assert_eq!(fc.target_date(), "2026-06-13");
    fc
}

/// Read the recorded markets array into `Vec<KalshiMarket>` via
/// `serde_json::from_value` (the fixture is `{ "markets": [...], "cursor": ... }`).
fn load_raw_markets() -> Vec<KalshiMarket> {
    let body = std::fs::read_to_string(KALSHI_FIXTURE).expect("read kalshi fixture");
    let root: serde_json::Value = serde_json::from_str(&body).expect("kalshi fixture is json");
    let markets_val = root
        .get("markets")
        .cloned()
        .expect("fixture has a markets array");
    serde_json::from_value(markets_val).expect("markets array deserializes into Vec<KalshiMarket>")
}

/// Convert the recorded raw `KalshiMarket`s to venue-neutral `MarketView`s
/// via the adapter conversion (the only path that populates geometry fields).
fn load_markets() -> Vec<MarketView> {
    load_raw_markets()
        .iter()
        .map(kalshi_market_to_market_view)
        .collect()
}

/// The 6 June-13 markets (event_ticker contains "26JUN13") — the complete
/// partition: T87(less) B87.5 B89.5 B91.5 B93.5(between) T94(greater).
/// Uses `market_id` (== ticker after conversion).
fn june13(markets: &[MarketView]) -> Vec<MarketView> {
    markets
        .iter()
        .filter(|m| m.market_id.contains("26JUN13"))
        .cloned()
        .collect()
}

// ─── GOLDEN REGRESSION (C1 load-bearing) ────────────────────────────────────

/// GOLDEN REGRESSION: proves that `kalshi_market_to_market_view` + `market_to_bucket`
/// produces the EXACT SAME `WeatherBucket` (kind + market_key + floor/cap) the old
/// direct-`KalshiMarket` path did. One representative of each strike_type.
///
/// Fixture data from `markets__high_temp.json` (recorded 2026-06-14):
///  - KXHIGHNY-26JUN13-T87 : strike_type="less",    cap=87   → LessEq(86)
///  - KXHIGHNY-26JUN13-B89 : strike_type="between",  floor=89, cap=91 → InRange(89,91)
///  - KXHIGHNY-26JUN13-T94 : strike_type="greater", floor=94 → GreaterEq(95)
///
/// These exact bucket values are what the old code produced by reading
/// `m.floor_strike_int()` / `m.cap_strike_int()` / `m.strike_type` on `KalshiMarket`.
/// After C1 the SAME values must come from `m.floor_strike` / `m.cap_strike` /
/// `m.strike_type` on `MarketView` — identical arithmetic, identical output.
#[test]
fn golden_regression_market_view_geometry() {
    let raw = load_raw_markets();

    // ── "less" representative: T87, cap_strike=87 ────────────────────────────
    let less_raw = raw
        .iter()
        .find(|m| m.ticker.contains("26JUN13-T87"))
        .expect("T87 in fixture");
    assert_eq!(less_raw.strike_type.as_deref(), Some("less"));
    assert_eq!(less_raw.cap_strike_int(), Some(87));
    let less_view = kalshi_market_to_market_view(less_raw);
    // Geometry round-trips through MarketView unchanged.
    assert_eq!(less_view.strike_type.as_deref(), Some("less"));
    assert_eq!(less_view.cap_strike, Some(87));
    assert_eq!(less_view.market_id, less_raw.ticker);
    let bucket = market_to_bucket(&less_view).expect("T87 'less' → LessEq bucket");
    assert_eq!(bucket.market_key, less_raw.ticker, "market_key == ticker");
    assert_eq!(
        bucket.kind,
        BucketKind::LessEq { threshold_f: 86 },
        "less: cap-1 = 87-1 = 86"
    );

    // ── "between" representative: B87.5 (covers 87–88) ──────────────────────
    let between_raw = raw
        .iter()
        .find(|m| m.ticker.contains("26JUN13-B87.5"))
        .expect("B87.5 in fixture");
    assert_eq!(between_raw.strike_type.as_deref(), Some("between"));
    let floor = between_raw.floor_strike_int().expect("B87.5 has floor");
    let cap = between_raw.cap_strike_int().expect("B87.5 has cap");
    let between_view = kalshi_market_to_market_view(between_raw);
    assert_eq!(between_view.strike_type.as_deref(), Some("between"));
    assert_eq!(between_view.floor_strike, Some(floor));
    assert_eq!(between_view.cap_strike, Some(cap));
    assert_eq!(between_view.market_id, between_raw.ticker);
    let bucket = market_to_bucket(&between_view).expect("B87.5 'between' → InRange bucket");
    assert_eq!(
        bucket.market_key, between_raw.ticker,
        "market_key == ticker"
    );
    assert_eq!(
        bucket.kind,
        BucketKind::InRange {
            lo_f: floor,
            hi_f: cap
        },
        "between: InRange(floor, cap) unchanged"
    );

    // ── "greater" representative: T94, floor_strike=94 ───────────────────────
    let greater_raw = raw
        .iter()
        .find(|m| m.ticker.contains("26JUN13-T94"))
        .expect("T94 in fixture");
    assert_eq!(greater_raw.strike_type.as_deref(), Some("greater"));
    assert_eq!(greater_raw.floor_strike_int(), Some(94));
    let greater_view = kalshi_market_to_market_view(greater_raw);
    assert_eq!(greater_view.strike_type.as_deref(), Some("greater"));
    assert_eq!(greater_view.floor_strike, Some(94));
    assert_eq!(greater_view.market_id, greater_raw.ticker);
    let bucket = market_to_bucket(&greater_view).expect("T94 'greater' → GreaterEq bucket");
    assert_eq!(
        bucket.market_key, greater_raw.ticker,
        "market_key == ticker"
    );
    assert_eq!(
        bucket.kind,
        BucketKind::GreaterEq { threshold_f: 95 },
        "greater: floor+1 = 94+1 = 95"
    );

    // ── unknown strike_type → None (no fabricated bucket) ────────────────────
    use fortuna_cognition::discovery::MarketView as MV;
    let unknown_view = MV {
        market_id: "FAKE-MARKET".to_string(),
        venue: "kalshi".to_string(),
        title: String::new(),
        category: String::new(),
        volume_contracts: None,
        resolution_source: String::new(),
        close_at: None,
        strike_type: Some("structured".to_string()),
        floor_strike: Some(50),
        cap_strike: Some(60),
        status: "active".to_string(),
    };
    assert!(
        market_to_bucket(&unknown_view).is_none(),
        "unknown strike_type 'structured' yields None (no fabricated bucket)"
    );

    // ── None strike_type → None ───────────────────────────────────────────────
    let no_type_view = MV {
        market_id: "FAKE-MARKET-2".to_string(),
        venue: "kalshi".to_string(),
        title: String::new(),
        category: String::new(),
        volume_contracts: None,
        resolution_source: String::new(),
        close_at: None,
        strike_type: None,
        floor_strike: None,
        cap_strike: None,
        status: String::new(),
    };
    assert!(
        market_to_bucket(&no_type_view).is_none(),
        "None strike_type yields None"
    );
}

// ─── EXISTING INTEGRATION TESTS (updated to use MarketView) ─────────────────

#[test]
fn june13_partition_yields_six_beliefs_and_edges_summing_to_one() {
    let fc = load_forecast();
    let markets = load_markets();
    let june13 = june13(&markets);
    assert_eq!(june13.len(), 6, "the recorded June-13 set is 6 markets");

    // Build the bucket set from the recorded markets (settled set is fine —
    // market_to_bucket is pure kind derivation, no status filter).
    let buckets: Vec<_> = june13
        .iter()
        .map(|m| market_to_bucket(m).expect("each June-13 market maps to a bucket"))
        .collect();
    assert_eq!(buckets.len(), 6);

    let (drafts, edges) = aeolus_bucket_edges(&fc, &buckets);

    // 1:1, order-preserving: 6 buckets → 6 beliefs → 6 edges.
    assert_eq!(drafts.len(), 6, "one belief per bucket");
    assert_eq!(edges.len(), 6, "one edge per belief (1:1)");

    // Each edge is an auto-confirmed Direct kalshi edge whose event_id and
    // market BOTH point at the recorded ticker.
    for (draft, edge) in drafts.iter().zip(edges.iter()) {
        let ticker = draft
            .event_id
            .strip_prefix("aeolus:")
            .expect("draft event_id is aeolus:{ticker}");
        assert_eq!(edge.mapping, MappingType::Direct);
        assert_eq!(edge.venue, "kalshi");
        assert_eq!(edge.event_id, format!("aeolus:{ticker}"));
        assert_eq!(edge.market.as_str(), ticker, "edge market == the ticker");
        assert_eq!(edge.proposed_by, "aeolus_bucket_match");
        assert_eq!(edge.confirmed_by.as_deref(), Some("discovery:auto"));
        // Every ticker is a recorded KXHIGHNY June-13 ticker — never fabricated.
        assert!(
            ticker.starts_with("KXHIGHNY-26JUN13-"),
            "ticker {ticker} is from the recorded June-13 book"
        );
    }

    // The partition telescopes: ≤86 + [87,88] + [89,90] + [91,92] + [93,94] +
    // ≥95 = 1.0. Use the belief draft `p` values (the matcher's probabilities).
    let sum: f64 = drafts.iter().map(|d| d.p).sum();
    assert!(
        (sum - 1.0).abs() < 1e-3,
        "the 6 partition beliefs' p's sum to ~1.0, got {sum:.17} (|1-sum|={:.3e})",
        (sum - 1.0).abs()
    );
    // Stronger than the contract floor: the telescoping is exact here.
    assert!(
        (sum - 1.0).abs() < 1e-9,
        "telescoping is exact at these μ/σ: got {sum:.17}"
    );
}

#[test]
fn dropping_the_greater_bucket_drops_exactly_its_belief_and_edge() {
    let fc = load_forecast();
    let markets = load_markets();
    let june13 = june13(&markets);

    // Build all 6 buckets, then DROP the T94 (greater) bucket: 6 → 5.
    let mut buckets: Vec<_> = june13
        .iter()
        .map(|m| market_to_bucket(m).expect("maps to a bucket"))
        .collect();
    let before = buckets.len();
    buckets.retain(|b| !b.market_key.contains("T94"));
    assert_eq!(buckets.len(), before - 1, "exactly the T94 bucket removed");
    assert_eq!(buckets.len(), 5);

    let (drafts, edges) = aeolus_bucket_edges(&fc, &buckets);

    // 5 buckets → 5 beliefs → 5 edges (still 1:1).
    assert_eq!(drafts.len(), 5, "five beliefs after the drop");
    assert_eq!(edges.len(), 5, "five edges after the drop");

    // The dropped bucket's belief AND edge are gone: NOTHING references T94.
    // (This reds if the matcher ignored the dropped bucket and re-emitted it.)
    assert!(
        edges.iter().all(|e| !e.market.as_str().contains("T94")),
        "no edge references the dropped T94 market"
    );
    assert!(
        edges.iter().all(|e| !e.event_id.contains("T94")),
        "no edge event_id references the dropped T94 market"
    );
    assert!(
        drafts.iter().all(|d| !d.event_id.contains("T94")),
        "no belief references the dropped T94 market"
    );

    // The remaining 5 no longer telescope to 1 (the ≥95 tail is missing) —
    // confirms the drop actually changed the emitted set.
    let sum: f64 = drafts.iter().map(|d| d.p).sum();
    assert!(
        sum < 1.0 && (1.0 - sum) > 1e-6,
        "without the ≥95 tail the 5 beliefs sum below 1.0, got {sum:.17}"
    );
}

#[test]
fn no_buckets_yields_no_beliefs_and_no_edges() {
    let fc = load_forecast();
    let (drafts, edges) = aeolus_bucket_edges(&fc, &[]);
    assert!(drafts.is_empty(), "no buckets ⇒ no beliefs");
    assert!(edges.is_empty(), "no buckets ⇒ no edges");
}
