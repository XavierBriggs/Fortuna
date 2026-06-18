//! F7 (Track-A) — the LIVE Kalshi day-set source, exercised against the RECORDED
//! `fixtures/kalshi/markets__high_temp.json` (18 verbatim KXHIGHNY markets across
//! June 13/14/15). No network: a `MockKalshiTransport` replays the fixture body
//! verbatim, so this proves the date-matching + pagination over REAL recorded
//! data. Every ticker asserted is from that book — none is fabricated.
//!
//! After C1 `day_set` returns `Vec<MarketView>` (venue-neutral). Status is a
//! `String` (`"active"` / `"settled"` / …); event-ticker membership is verified
//! via the market_id prefix (the per-strike market ticker starts with the event
//! ticker, e.g. `KXHIGHNY-26JUN15-T78` starts with `KXHIGHNY-26JUN15`).

use std::sync::Arc;

use fortuna_venues::kalshi::weather::{event_grades_on, KalshiWeatherSource};
use fortuna_venues::kalshi::MockKalshiTransport;
use fortuna_venues::WeatherMarketSource;

const KALSHI_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/kalshi/markets__high_temp.json"
);

/// The verbatim `{ "markets": [...], "cursor": "" }` fixture body.
fn fixture_body() -> serde_json::Value {
    let raw = std::fs::read_to_string(KALSHI_FIXTURE).expect("read kalshi fixture");
    serde_json::from_str(&raw).expect("fixture is json")
}

/// A mock transport that will serve the recorded body once for the day_set page.
fn source_over_fixture() -> KalshiWeatherSource {
    let mock = MockKalshiTransport::new();
    mock.push_ok(200, fixture_body());
    KalshiWeatherSource::new(Arc::new(mock))
}

// ---- pure date-match helper, grounded on the recorded event tickers ----

#[test]
fn event_grades_on_matches_the_recorded_token_exactly() {
    // The three recorded event tickers.
    assert!(event_grades_on("KXHIGHNY-26JUN13", "2026-06-13"));
    assert!(event_grades_on("KXHIGHNY-26JUN14", "2026-06-14"));
    assert!(event_grades_on("KXHIGHNY-26JUN15", "2026-06-15"));
    // A market ticker (event + strike suffix) still matches on its event token.
    assert!(event_grades_on("KXHIGHNY-26JUN13-T94", "2026-06-13"));
    assert!(event_grades_on("KXHIGHNY-26JUN15-B84.5", "2026-06-15"));
}

#[test]
fn event_grades_on_rejects_other_dates_and_malformed_input() {
    // Wrong day / month / year → no match.
    assert!(!event_grades_on("KXHIGHNY-26JUN13", "2026-06-14"));
    assert!(!event_grades_on("KXHIGHNY-26JUN13", "2026-07-13"));
    assert!(!event_grades_on("KXHIGHNY-26JUN13", "2026-06-13-extra"));
    // The '-' boundary prevents an inside-a-run false positive.
    assert!(!event_grades_on("KXHIGHNY-126JUN13", "2026-06-13"));
    // Malformed ISO dates → false, never a panic.
    assert!(!event_grades_on("KXHIGHNY-26JUN13", "not-a-date"));
    assert!(!event_grades_on("KXHIGHNY-26JUN13", "2026/06/13"));
    assert!(!event_grades_on("KXHIGHNY-26JUN13", "2026-13-13")); // month 13
    assert!(!event_grades_on("KXHIGHNY-26JUN13", "2026-06-00")); // day 0
    assert!(!event_grades_on("KXHIGHNY-26JUN13", "2026-06-13-1")); // 4 components
}

// ---- the live source over the recorded multi-day book ----

#[tokio::test]
async fn day_set_returns_only_the_active_june15_event() {
    let src = source_over_fixture();
    let day = src
        .day_set("KXHIGHNY", "2026-06-15")
        .await
        .expect("day_set over the recorded book");
    // June 15 is the 6-market active partition (T78, T85, B78.5, B80.5, B82.5, B84.5).
    assert_eq!(day.len(), 6, "the recorded June-15 day-set is 6 markets");
    // After C1: market_id is the per-strike ticker (e.g. "KXHIGHNY-26JUN15-T78");
    // checking that it starts with the event-ticker prefix proves correct date scope.
    assert!(
        day.iter()
            .all(|m| m.market_id.starts_with("KXHIGHNY-26JUN15")),
        "every returned market grades on June 15"
    );
    // The day_set does NOT filter status — June 15 happens to be all `active`.
    assert!(
        day.iter().all(|m| m.status == "active"),
        "June 15 markets are active"
    );
}

#[tokio::test]
async fn day_set_returns_the_settled_june13_event_unfiltered() {
    let src = source_over_fixture();
    let day = src
        .day_set("KXHIGHNY", "2026-06-13")
        .await
        .expect("day_set over the recorded book");
    // June 13 is the 6-market `determined` partition — day_set returns it
    // verbatim (the tradeable-status filter is the caller's job).
    assert_eq!(day.len(), 6, "the recorded June-13 day-set is 6 markets");
    assert!(
        day.iter().all(|m| m.status == "settled"),
        "June 13 is settled — day_set still returns it (no status filter here)"
    );
}

#[tokio::test]
async fn day_set_for_a_date_with_no_market_is_empty_not_an_error() {
    let src = source_over_fixture();
    // June 16 is absent from the recorded book — "no live market ⇒ not traded",
    // an empty Vec, never an error and never a fabricated market.
    let day = src
        .day_set("KXHIGHNY", "2026-06-16")
        .await
        .expect("a missing day is an empty Vec, not an error");
    assert!(day.is_empty(), "no June-16 markets in the recorded book");
}

#[tokio::test]
async fn day_set_issues_a_read_only_markets_get_scoped_to_the_series() {
    let mock = MockKalshiTransport::new();
    mock.push_ok(200, fixture_body());
    let transport = Arc::new(mock);
    let src = KalshiWeatherSource::new(transport.clone());
    let _ = src
        .day_set("KXHIGHNY", "2026-06-15")
        .await
        .expect("day_set");
    let calls = transport.calls();
    assert_eq!(calls.len(), 1, "single page (fixture cursor is empty)");
    assert_eq!(calls[0].method, "GET", "READ-ONLY: discovery is a GET");
    assert_eq!(calls[0].path, "/markets");
    let q = calls[0].query.as_deref().unwrap_or("");
    assert!(
        q.contains("series_ticker=KXHIGHNY"),
        "scoped to the series, got query {q:?}"
    );
}
