//! F7 venue seam (Track-A): the LIVE Kalshi day-set source for the Aeolus
//! weather match. READ-ONLY discovery (`GET /markets?series_ticker=...`) that
//! returns the temperature-bracket markets grading on ONE forecast day, so the
//! cognition matcher can build buckets from the REAL book — never a fabricated
//! ticker. A thin transport wrapper plus a PURE date-match helper; no `Clock`,
//! no panic, no writes.
//!
//! The split of concerns the F7 plug-in relies on:
//!   * [`WeatherMarketSource::day_set`] owns the venue call + the "which markets
//!     belong to date D" filter (date-matching against the RECORDED
//!     `event_ticker`). It returns ALL statuses for the date; the caller filters
//!     to a tradeable status (a settled day is not traded, but it is still the
//!     day-set).
//!   * [`event_grades_on`] is the pure match key — DERIVED from the ISO date and
//!     tested against the date appearing on recorded tickers. It is a MATCH key,
//!     never a constructed market ticker: every bucket/edge keys on the recorded
//!     `market.ticker`.

use crate::kalshi::client::KalshiTransport;
use crate::kalshi::dto::{error_reason, GetMarketsResponse, KalshiMarket};
use crate::VenueError;
use async_trait::async_trait;
use std::sync::Arc;

/// The day-set source the F7 live plug-in consumes. Given a Kalshi temperature
/// SERIES (e.g. `KXHIGHNY`) and a forecast TARGET DATE (`YYYY-MM-DD`), return the
/// markets that grade on that date.
///
/// A date with NO live markets yields an empty `Vec` — NOT an error ("a
/// synthesized event with no live market is simply not traded", contract). A
/// transport or response-parse failure is an `Err`: a malformed venue frame is a
/// hard error, never a fabricated market.
#[async_trait]
pub trait WeatherMarketSource: Send + Sync {
    async fn day_set(
        &self,
        series: &str,
        target_date: &str,
    ) -> Result<Vec<KalshiMarket>, VenueError>;
}

/// 3-letter UPPERCASE month tokens as Kalshi writes them in event tickers
/// (`KXHIGHNY-26JUN13`). Index 0 = January. GROUNDED by
/// `fixtures/kalshi/markets__high_temp.json` (every recorded event_ticker uses
/// `JUN`).
const KALSHI_MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// Does this recorded `event_ticker` grade on `target_date` (`YYYY-MM-DD`)?
///
/// Kalshi temperature events are tickered `{SERIES}-{YY}{MON}{DD}` (e.g.
/// `KXHIGHNY-26JUN13`; the per-strike market ticker appends `-{strike}`). We
/// DERIVE the expected `{YY}{MON}{DD}` token from the ISO date (2-digit year,
/// 3-letter month, ZERO-PADDED 2-digit day — the form recorded in the fixture)
/// and test whether the event_ticker carries it as a `-{token}` suffix on the
/// event portion.
///
/// This is a MATCH key against recorded data — we never construct a market
/// ticker to trade. A malformed/implausible ISO date (wrong shape, month∉1..=12,
/// day∉1..=31) returns `false` (no match — the conservative outcome is "this day
/// is simply not matched", never a wrong trade). Pure; never panics.
pub fn event_grades_on(event_ticker: &str, target_date: &str) -> bool {
    let mut parts = target_date.split('-');
    let (Some(y), Some(m), Some(d), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false; // not exactly YYYY-MM-DD
    };
    let (Ok(year), Ok(month), Ok(day)) = (y.parse::<i32>(), m.parse::<usize>(), d.parse::<u32>())
    else {
        return false;
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return false;
    }
    let yy = year.rem_euclid(100); // 2026 -> 26 (never negative)
    let token = format!("{:02}{}{:02}", yy, KALSHI_MONTHS[month - 1], day); // 26JUN13
                                                                            // The token is a '-'-delimited segment of the ticker — true for the event
                                                                            // ticker ("KXHIGHNY-26JUN13") AND the per-strike market ticker
                                                                            // ("KXHIGHNY-26JUN13-T94"). Matching the whole segment (not a substring)
                                                                            // keeps "26JUN13" from matching inside a longer run like "126JUN13".
    event_ticker.split('-').any(|seg| seg == token)
}

/// Bound the discovery pagination. A single forecast day-set is far under one
/// 1000-row page; the cap is a guard against a runaway cursor, never a real
/// limit (so it does not silently truncate a real day-set).
const MAX_PAGES: usize = 40;

/// The live Kalshi-transport-backed [`WeatherMarketSource`]. Paginates
/// `GET /markets?series_ticker={series}` (READ-ONLY — no orders, no writes) and
/// keeps the markets whose `event_ticker` grades on the target date. Untrusted
/// response data: a frame that fails to parse, or a non-200 status, is a hard
/// `Err` (never a fabricated market). Shares the SAME `Arc<dyn KalshiTransport>`
/// the runner signs with, so it inherits demo/prod routing + auth; it adds no
/// credentials of its own.
pub struct KalshiWeatherSource {
    transport: Arc<dyn KalshiTransport>,
}

impl KalshiWeatherSource {
    pub fn new(transport: Arc<dyn KalshiTransport>) -> Self {
        Self { transport }
    }
}

#[async_trait]
impl WeatherMarketSource for KalshiWeatherSource {
    async fn day_set(
        &self,
        series: &str,
        target_date: &str,
    ) -> Result<Vec<KalshiMarket>, VenueError> {
        let mut out = Vec::new();
        let mut cursor = String::new();
        for _ in 0..MAX_PAGES {
            // No status filter: capture ALL statuses for the series so the
            // caller sees the complete day-set (it applies the tradeable-status
            // filter). series_ticker scopes the page to one series.
            let mut q = format!("series_ticker={series}&limit=1000");
            if !cursor.is_empty() {
                q.push_str(&format!("&cursor={cursor}"));
            }
            let (status, body) = self
                .transport
                .request("GET", "/markets", Some(&q), None)
                .await?;
            if status != 200 {
                return Err(VenueError::Invalid {
                    reason: format!(
                        "kalshi GET /markets (series {series}) returned HTTP {status}: {}",
                        error_reason(&body)
                    ),
                });
            }
            let parsed: GetMarketsResponse =
                serde_json::from_value(body).map_err(|e| VenueError::Invalid {
                    reason: format!(
                        "kalshi GET /markets (series {series}) body did not parse: {e}"
                    ),
                })?;
            for m in parsed.markets {
                if event_grades_on(&m.event_ticker, target_date) {
                    out.push(m);
                }
            }
            cursor = parsed.cursor;
            if cursor.is_empty() {
                break;
            }
        }
        Ok(out)
    }
}
