//! Live smoke test: run the real source adapters against the LIVE upstreams and
//! print what comes back. This is a manual diagnostic (hits the network), NOT a
//! test — run it by hand:
//!
//!   cargo run -p fortuna-sources --example live_smoke
//!
//! It exercises the production path end to end: ReqwestFetchTransport (host-pin,
//! https-only, conditional GET, politeness) + the adapter parsers + the Layer-1
//! claimed-time extractors. GDELT is omitted (aggressive rate limit).

use std::sync::Arc;
use std::time::Duration;

use fortuna_cognition::signals::{RawSignal, Source};
use fortuna_core::clock::RealClock;
use fortuna_sources::{
    calendar_claimed_time, nws_claimed_time, rss_claimed_time, CalendarFeed, CalendarSource,
    FetchCaps, FetchClient, HostPin, NwsFeed, NwsSource, ReqwestFetchTransport, RssSource,
};

const UA: &str = "(fortuna-live-smoke, xbriggs03@gmail.com)";

fn client(base_url: &str) -> FetchClient<ReqwestFetchTransport> {
    let pin = HostPin::from_url(base_url).expect("valid pinned base url");
    let transport =
        ReqwestFetchTransport::new(Duration::from_secs(20), UA).expect("build transport");
    FetchClient::new(transport, pin, 60, FetchCaps::default())
}

/// Fetch a source, print a compact summary + the first few signals.
async fn show(
    label: &str,
    mut source: impl Source,
    claimed: fn(&RawSignal) -> Option<fortuna_core::clock::UtcTimestamp>,
    field: &str,
) {
    print!("\n── {label}  ");
    match source.fetch().await {
        Ok(signals) => {
            println!("✓ {} signal(s)", signals.len());
            for s in signals.iter().take(3) {
                let v = s
                    .payload
                    .get(field)
                    .and_then(|x| x.as_str())
                    .or_else(|| {
                        s.payload
                            .get("properties")
                            .and_then(|p| p.get(field))
                            .and_then(|x| x.as_str())
                    })
                    .unwrap_or("(n/a)");
                let shown: String = v.chars().take(72).collect();
                let when = claimed(s)
                    .map(|t| t.to_iso8601())
                    .unwrap_or_else(|| "—".to_string());
                println!("   [{}] {}  (claimed: {})", s.kind, shown, when);
            }
        }
        Err(e) => println!("✗ error: {e}"),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let clock = Arc::new(RealClock);
    println!("FORTUNA sources — LIVE smoke test (real upstreams)");

    // --- NWS (weather): alerts + forecast discussions ---
    show(
        "NWS active alerts (TX)",
        NwsSource::new(
            "nws_alerts_tx",
            NwsFeed::AlertsActive,
            "https://api.weather.gov/alerts/active?area=TX",
            client("https://api.weather.gov/"),
            clock.clone(),
        ),
        nws_claimed_time,
        "event",
    )
    .await;

    show(
        "NWS Area Forecast Discussions",
        NwsSource::new(
            "nws_afd",
            NwsFeed::AfdProducts,
            "https://api.weather.gov/products?type=AFD",
            client("https://api.weather.gov/"),
            clock.clone(),
        ),
        nws_claimed_time,
        "issuingOffice",
    )
    .await;

    // --- RSS / Atom (macro, markets) ---
    show(
        "Federal Reserve press releases (RSS)",
        RssSource::new(
            "rss_fed_press",
            "https://www.federalreserve.gov/feeds/press_all.xml",
            client("https://www.federalreserve.gov/"),
            clock.clone(),
        ),
        rss_claimed_time,
        "title",
    )
    .await;

    show(
        "SEC EDGAR 8-K filings (Atom)",
        RssSource::new(
            "rss_sec_edgar",
            "https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=8-K&company=&dateb=&owner=include&count=10&output=atom",
            client("https://www.sec.gov/"),
            clock.clone(),
        ),
        rss_claimed_time,
        "title",
    )
    .await;

    // --- Calendar (macro release schedule + latest numbers) ---
    show(
        "BLS release schedule (iCalendar → release_scheduled)",
        CalendarSource::new(
            "calendar_bls_schedule",
            CalendarFeed::Schedule,
            "https://www.bls.gov/schedule/news_release/bls.ics",
            client("https://www.bls.gov/"),
            clock.clone(),
        ),
        calendar_claimed_time,
        "release",
    )
    .await;

    show(
        "BLS latest numbers (RSS → release_printed)",
        CalendarSource::new(
            "calendar_bls_latest",
            CalendarFeed::LatestReleases,
            "https://www.bls.gov/feed/bls_latest.rss",
            client("https://www.bls.gov/"),
            clock.clone(),
        ),
        calendar_claimed_time,
        "title",
    )
    .await;

    println!("\nDone.");
}
