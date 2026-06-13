//! RSS/Atom adapter (design §4.2): one generic adapter, N configured feeds.
//! `feed-rs` unifies RSS 2.0, RSS 1.0/RDF, Atom, and JSON Feed, so a Fed
//! press-release RSS item and an SEC EDGAR Atom entry both normalize to the
//! same `rss.item` signal shape — that format-normalization is the adapter's
//! legitimate job (spec 5.11), distinct from interpreting the content, which
//! it never does.
//!
//! Like every adapter here it is dumb: fetch the one configured feed through
//! the shared [`FetchClient`] (politeness, host pin, conditional GET), parse,
//! emit one `RawSignal` per entry. Dedup, trust, and triggering are downstream.
//!
//! `rss_claimed_time` exposes the entry's published/updated time for the
//! Layer-1 future-dated check (design §4.4), the way `nws_claimed_time` does.

use std::sync::Arc;

use async_trait::async_trait;
use fortuna_cognition::signals::{RawSignal, SignalError, Source};
use fortuna_core::clock::{Clock, UtcTimestamp};
use serde_json::{json, Value};

use crate::fetch::{Conditional, FetchClient, FetchOutcome, FetchTransport};

/// The signal kind every feed entry normalizes to. The originating feed is
/// carried by the adapter's `source` id (attached downstream by the
/// normalizer), and the domain by the source_registry tags — not the kind.
pub const RSS_ITEM_KIND: &str = "rss.item";

/// One RSS/Atom feed. One instance == one configured feed (one url, one
/// source id, its own trust tier); "N feeds" is N instances.
pub struct RssSource<T: FetchTransport> {
    id: String,
    url: String,
    client: FetchClient<T>,
    clock: Arc<dyn Clock>,
    etag: Option<String>,
    last_modified: Option<String>,
}

impl<T: FetchTransport> RssSource<T> {
    pub fn new(
        id: impl Into<String>,
        url: impl Into<String>,
        client: FetchClient<T>,
        clock: Arc<dyn Clock>,
    ) -> RssSource<T> {
        RssSource {
            id: id.into(),
            url: url.into(),
            client,
            clock,
            etag: None,
            last_modified: None,
        }
    }
}

#[async_trait]
impl<T: FetchTransport> Source for RssSource<T> {
    fn id(&self) -> &str {
        &self.id
    }

    async fn fetch(&mut self) -> Result<Vec<RawSignal>, SignalError> {
        let cond = Conditional {
            etag: self.etag.clone(),
            last_modified: self.last_modified.clone(),
        };
        let received_at = self.clock.now();
        let outcome = self
            .client
            .fetch(&self.url, &cond, self.clock.as_ref())
            .await
            .map_err(|e| SignalError::Fetch {
                source_id: self.id.clone(),
                reason: e.to_string(),
            })?;
        match outcome {
            FetchOutcome::NotModified => Ok(Vec::new()),
            FetchOutcome::Fetched {
                body,
                etag,
                last_modified,
            } => {
                self.etag = etag;
                self.last_modified = last_modified;
                parse_feed(&body, received_at).map_err(|reason| SignalError::Fetch {
                    source_id: self.id.clone(),
                    reason,
                })
            }
        }
    }
}

/// Parse an RSS/Atom/JSON-Feed body into `rss.item` signals. Pure (the
/// `received_at` is supplied), so it is tested directly against recorded
/// fixtures. `feed-rs` leaves missing dates as `None` (no wall-time fill), so
/// this stays deterministic.
fn parse_feed(body: &[u8], received_at: UtcTimestamp) -> Result<Vec<RawSignal>, String> {
    let feed = feed_rs::parser::parse(body).map_err(|e| format!("feed parse: {e}"))?;
    Ok(feed
        .entries
        .into_iter()
        .map(|entry| RawSignal {
            kind: RSS_ITEM_KIND.to_string(),
            payload: entry_payload(entry),
            received_at,
        })
        .collect())
}

/// Normalize one `feed-rs` entry into a canonical JSON item. Heterogeneous
/// feed formats collapse to the same shape here; the values are passed through
/// untouched (untrusted content, spec 5.11 — never interpreted).
fn entry_payload(entry: feed_rs::model::Entry) -> Value {
    let title = entry.title.map(|t| t.content);
    let summary = entry.summary.map(|t| t.content);
    // The "alternate" link is the canonical article URL; fall back to the
    // first link if rel is unset (common in RSS 2.0).
    let link = entry
        .links
        .iter()
        .find(|l| l.rel.as_deref() == Some("alternate"))
        .or_else(|| entry.links.first())
        .map(|l| l.href.clone());
    let published = entry.published.map(chrono_to_iso8601);
    let updated = entry.updated.map(chrono_to_iso8601);
    let categories: Vec<String> = entry.categories.into_iter().map(|c| c.term).collect();
    json!({
        "id": entry.id,
        "title": title,
        "link": link,
        "summary": summary,
        "published": published,
        "updated": updated,
        "categories": categories,
    })
}

/// `feed-rs` dates are chrono `DateTime<Utc>`; render them in the system's
/// canonical fixed-ms ISO8601 (via `UtcTimestamp`) so they match every other
/// timestamp in FORTUNA and round-trip through `parse_iso8601`.
fn chrono_to_iso8601(dt: chrono::DateTime<chrono::Utc>) -> String {
    match UtcTimestamp::from_epoch_millis(dt.timestamp_millis()) {
        Ok(ts) => ts.to_iso8601(),
        // Out-of-range epoch (astronomically unlikely from a real feed) —
        // keep the raw RFC3339 rather than drop the field.
        Err(_) => dt.to_rfc3339(),
    }
}

/// Extract the entry's claimed time for the Layer-1 future-dated check:
/// `published` if present, else `updated`. `None` when neither is present or
/// parseable (the validator then skips the future check for that item).
pub fn rss_claimed_time(signal: &RawSignal) -> Option<UtcTimestamp> {
    if signal.kind != RSS_ITEM_KIND {
        return None;
    }
    let field = signal
        .payload
        .get("published")
        .and_then(Value::as_str)
        .or_else(|| signal.payload.get("updated").and_then(Value::as_str))?;
    UtcTimestamp::parse_iso8601(field).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::{FetchCaps, FetchError, HostPin, RawHttpResponse};
    use std::sync::Mutex;

    const FED_RSS2: &[u8] = include_bytes!("../../../fixtures/sources/rss/fed_press_rss2.xml");
    const SEC_ATOM: &[u8] = include_bytes!("../../../fixtures/sources/rss/sec_edgar_atom.xml");
    const MALFORMED: &[u8] = include_bytes!("../../../fixtures/sources/rss/malformed.xml");

    fn ts(ms: i64) -> UtcTimestamp {
        UtcTimestamp::from_epoch_millis(ms).unwrap()
    }

    // --- pure parser tests (fixtures-first; real RSS 2.0 + real Atom) -----

    #[test]
    fn parses_real_rss2_feed() {
        let signals = parse_feed(FED_RSS2, ts(1000)).unwrap();
        assert_eq!(signals.len(), 2);
        let s = &signals[0];
        assert_eq!(s.kind, RSS_ITEM_KIND);
        assert_eq!(s.received_at, ts(1000));
        assert!(s.payload["title"]
            .as_str()
            .unwrap()
            .contains("Federal Reserve"));
        assert!(s.payload["link"].as_str().unwrap().starts_with("https://"));
        // RSS 2.0 pubDate -> published, normalized to fixed-ms ISO8601.
        assert!(s.payload["published"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn parses_real_atom_feed_to_the_same_shape() {
        let signals = parse_feed(SEC_ATOM, ts(2000)).unwrap();
        assert_eq!(signals.len(), 2);
        let s = &signals[0];
        assert_eq!(s.kind, RSS_ITEM_KIND);
        // Atom <updated> populates `updated`; same canonical payload keys as RSS.
        for key in [
            "id",
            "title",
            "link",
            "summary",
            "published",
            "updated",
            "categories",
        ] {
            assert!(s.payload.get(key).is_some(), "missing key {key}");
        }
        assert!(s.payload["title"].as_str().unwrap().contains("8-K"));
        assert!(s.payload["updated"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn malformed_feed_is_a_fetch_error_not_a_panic() {
        let err = parse_feed(MALFORMED, ts(0)).unwrap_err();
        assert!(err.contains("feed parse"), "{err}");
    }

    // --- claimed-time extraction (Layer 1 input) -------------------------

    #[test]
    fn claimed_time_prefers_published_then_updated() {
        // RSS 2.0 item: published present.
        let rss = &parse_feed(FED_RSS2, ts(0)).unwrap()[0];
        assert!(rss_claimed_time(rss).is_some());

        // An item with only `updated` falls back to it.
        let only_updated = RawSignal {
            kind: RSS_ITEM_KIND.into(),
            payload: json!({"updated": "2026-06-12T10:00:00.000Z", "published": null}),
            received_at: ts(0),
        };
        assert_eq!(
            rss_claimed_time(&only_updated),
            Some(UtcTimestamp::parse_iso8601("2026-06-12T10:00:00.000Z").unwrap())
        );
    }

    #[test]
    fn claimed_time_none_for_wrong_kind_or_no_dates() {
        let s = RawSignal {
            kind: "other".into(),
            payload: json!({"published": "2026-06-12T10:00:00.000Z"}),
            received_at: ts(0),
        };
        assert!(rss_claimed_time(&s).is_none());
        let s = RawSignal {
            kind: RSS_ITEM_KIND.into(),
            payload: json!({"published": null, "updated": null}),
            received_at: ts(0),
        };
        assert!(rss_claimed_time(&s).is_none());
    }

    // --- full Source::fetch path through a scripted transport ------------

    struct ScriptedTransport {
        responses: Mutex<Vec<Result<RawHttpResponse, FetchError>>>,
    }

    #[async_trait]
    impl FetchTransport for ScriptedTransport {
        async fn get(
            &self,
            _url: &str,
            _conditional: &Conditional,
        ) -> Result<RawHttpResponse, FetchError> {
            self.responses.lock().unwrap().remove(0)
        }
    }

    fn resp(status: u16, body: &[u8]) -> RawHttpResponse {
        RawHttpResponse {
            status,
            etag: Some("\"v1\"".into()),
            last_modified: None,
            location: None,
            body: body.to_vec(),
        }
    }

    fn source(responses: Vec<Result<RawHttpResponse, FetchError>>) -> RssSource<ScriptedTransport> {
        let pin = HostPin::from_url("https://www.federalreserve.gov/").unwrap();
        let client = FetchClient::new(
            ScriptedTransport {
                responses: Mutex::new(responses),
            },
            pin,
            60,
            FetchCaps::default(),
        );
        RssSource::new(
            "rss_fed_press",
            "https://www.federalreserve.gov/feeds/press_all.xml",
            client,
            Arc::new(fortuna_core::clock::SimClock::new(ts(7000))),
        )
    }

    #[tokio::test]
    async fn fetch_emits_items_stamped_with_clock_now() {
        let mut src = source(vec![Ok(resp(200, FED_RSS2))]);
        let signals = src.fetch().await.unwrap();
        assert_eq!(signals.len(), 2);
        assert_eq!(signals[0].received_at, ts(7000));
    }

    #[tokio::test]
    async fn fetch_304_yields_no_items() {
        let mut src = source(vec![Ok(resp(304, b""))]);
        assert!(src.fetch().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn fetch_maps_malformed_body_to_signal_error() {
        let mut src = source(vec![Ok(resp(200, MALFORMED))]);
        let err = src.fetch().await.unwrap_err();
        assert!(matches!(err, SignalError::Fetch { .. }));
    }
}
