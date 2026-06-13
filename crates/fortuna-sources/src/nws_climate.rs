//! NWS climate-daily grader (F2, Aeolus contract §3.2): the authoritative
//! observed daily-extreme RESOLUTION SOURCE (spec 5.12). It ingests the NWS
//! CLI products (Climatological Report — Daily), which carry the OFFICIAL daily
//! max/min — the same record the market and Aeolus resolve against (a max-of-
//! hourly-observations would be a DERIVED value and would not match).
//!
//! Two-hop: the `/products?type=CLI` list gives summaries (no text); the report
//! text (with the temperatures) is a per-product fetch. The adapter fetches the
//! list, then the text of each not-yet-seen product (bounded per tick), and
//! emits one `nws.cli` signal carrying the RAW `productText`.
//!
//! Deliberately DUMB about the temperatures: the CLI text is fragile to parse
//! (columns jam, e.g. `MINIMUM 7676` = observed 76 + record 76), and a mis-read
//! daily high would mis-GRADE a belief. So the adapter does NOT extract max/min
//! — it carries the raw authoritative text + a robustly-parsed `report_date`
//! for indexing; the GRADER (cognition, at settlement) extracts the official
//! high for the target date, where an ambiguity can be flagged not silently
//! traded.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use fortuna_cognition::signals::{RawSignal, SignalError, Source};
use fortuna_core::clock::{Clock, UtcTimestamp};
use serde_json::{json, Value};

use crate::fetch::{Conditional, FetchClient, FetchOutcome, FetchTransport};

pub const NWS_CLI_KIND: &str = "nws.cli";

/// The CLI climate-daily grader source.
pub struct NwsClimateSource<T: FetchTransport> {
    id: String,
    list_url: String,
    client: FetchClient<T>,
    clock: Arc<dyn Clock>,
    /// Conditional-GET validators for the LIST (the per-product texts are
    /// immutable once issued, so they are fetched without conditionals once).
    etag: Option<String>,
    last_modified: Option<String>,
    /// Bounded set of product ids whose text we already emitted (FIFO evict).
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    seen_cap: usize,
    /// Cap on NEW per-product text fetches per tick (politeness + bounding).
    max_new_per_tick: usize,
}

impl<T: FetchTransport> NwsClimateSource<T> {
    pub fn new(
        id: impl Into<String>,
        list_url: impl Into<String>,
        client: FetchClient<T>,
        clock: Arc<dyn Clock>,
    ) -> NwsClimateSource<T> {
        NwsClimateSource {
            id: id.into(),
            list_url: list_url.into(),
            client,
            clock,
            etag: None,
            last_modified: None,
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
            seen_cap: 4096,
            max_new_per_tick: 30,
        }
    }

    fn remember(&mut self, id: String) {
        if self.seen.insert(id.clone()) {
            self.seen_order.push_back(id);
            while self.seen_order.len() > self.seen_cap {
                if let Some(old) = self.seen_order.pop_front() {
                    self.seen.remove(&old);
                }
            }
        }
    }
}

#[async_trait]
impl<T: FetchTransport> Source for NwsClimateSource<T> {
    fn id(&self) -> &str {
        &self.id
    }

    async fn fetch(&mut self) -> Result<Vec<RawSignal>, SignalError> {
        let cond = Conditional {
            etag: self.etag.clone(),
            last_modified: self.last_modified.clone(),
        };
        let received_at = self.clock.now();
        let list_outcome = self
            .client
            .fetch(&self.list_url, &cond, self.clock.as_ref())
            .await
            .map_err(|e| self.err(e.to_string()))?;
        let body = match list_outcome {
            FetchOutcome::NotModified => return Ok(Vec::new()),
            FetchOutcome::Fetched {
                body,
                etag,
                last_modified,
            } => {
                self.etag = etag;
                self.last_modified = last_modified;
                body
            }
        };

        let entries = parse_list(&body).map_err(|r| self.err(r))?;
        let mut signals = Vec::new();
        let mut fetched = 0usize;
        for entry in entries {
            if fetched >= self.max_new_per_tick {
                break;
            }
            if self.seen.contains(&entry.id) {
                continue;
            }
            // Per-product text fetch (immutable; no conditional). A single bad
            // product is skipped (not marked seen → retried next tick), never
            // failing the whole poll.
            match self
                .client
                .fetch(&entry.url, &Conditional::default(), self.clock.as_ref())
                .await
            {
                Ok(FetchOutcome::Fetched { body, .. }) => {
                    if let Ok(signal) = parse_product(&body, received_at) {
                        signals.push(signal);
                        self.remember(entry.id);
                        fetched += 1;
                    }
                }
                _ => continue,
            }
        }
        Ok(signals)
    }
}

impl<T: FetchTransport> NwsClimateSource<T> {
    fn err(&self, reason: String) -> SignalError {
        SignalError::Fetch {
            source_id: self.id.clone(),
            reason,
        }
    }
}

struct ListEntry {
    id: String,
    url: String,
}

/// Parse the `/products?type=CLI` `@graph` list into (id, product-url) pairs.
fn parse_list(body: &[u8]) -> Result<Vec<ListEntry>, String> {
    let root: Value =
        serde_json::from_slice(body).map_err(|e| format!("cli list not JSON: {e}"))?;
    let graph = root
        .get("@graph")
        .and_then(Value::as_array)
        .ok_or_else(|| "cli list: expected `@graph` array".to_string())?;
    Ok(graph
        .iter()
        .filter_map(|item| {
            let id = item.get("id").and_then(Value::as_str)?.to_string();
            // Prefer the canonical @id URL; fall back to constructing it.
            let url = item
                .get("@id")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| format!("https://api.weather.gov/products/{id}"));
            Some(ListEntry { id, url })
        })
        .collect())
}

/// Parse a CLI product into an `nws.cli` signal. The RAW `productText` is the
/// authoritative payload; `report_date` is parsed best-effort for indexing.
fn parse_product(body: &[u8], received_at: UtcTimestamp) -> Result<RawSignal, String> {
    let p: Value =
        serde_json::from_slice(body).map_err(|e| format!("cli product not JSON: {e}"))?;
    let text = p
        .get("productText")
        .and_then(Value::as_str)
        .ok_or_else(|| "cli product: no productText".to_string())?;
    let payload = json!({
        "id": p.get("id"),
        "issuingOffice": p.get("issuingOffice"),
        "issuanceTime": p.get("issuanceTime"),
        "report_date": parse_report_date(text),
        "productText": text,
    });
    Ok(RawSignal {
        kind: NWS_CLI_KIND.to_string(),
        payload,
        received_at,
    })
}

/// Extract the date a CLI report covers from its `... CLIMATE SUMMARY FOR
/// <day> <MONTH> <year>` line → `YYYY-MM-DD`. Returns `None` (not an error) if
/// the line is absent/unparseable — the raw text remains the authority.
fn parse_report_date(text: &str) -> Option<String> {
    let idx = text.find("CLIMATE SUMMARY FOR ")?;
    let tail = &text[idx + "CLIMATE SUMMARY FOR ".len()..];
    let line = tail.lines().next()?;
    let mut tok = line.split_whitespace();
    let day: u32 = tok.next()?.parse().ok()?;
    let month = month_number(tok.next()?)?;
    let year: i32 = tok.next()?.parse().ok()?;
    if !(1..=31).contains(&day) {
        return None;
    }
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

fn month_number(name: &str) -> Option<u32> {
    match name.to_ascii_uppercase().as_str() {
        "JANUARY" => Some(1),
        "FEBRUARY" => Some(2),
        "MARCH" => Some(3),
        "APRIL" => Some(4),
        "MAY" => Some(5),
        "JUNE" => Some(6),
        "JULY" => Some(7),
        "AUGUST" => Some(8),
        "SEPTEMBER" => Some(9),
        "OCTOBER" => Some(10),
        "NOVEMBER" => Some(11),
        "DECEMBER" => Some(12),
        _ => None,
    }
}

/// Claimed time for the Layer-1 future-check: the report's `issuanceTime`
/// (a past time — the report is issued the morning after the day it covers).
pub fn nws_climate_claimed_time(signal: &RawSignal) -> Option<UtcTimestamp> {
    if signal.kind != NWS_CLI_KIND {
        return None;
    }
    let raw = signal.payload.get("issuanceTime").and_then(Value::as_str)?;
    UtcTimestamp::parse_iso8601(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::{FetchCaps, FetchError, HostPin, RawHttpResponse};
    use std::sync::Mutex;

    const LIST: &[u8] = include_bytes!("../../../fixtures/sources/nws_climate/cli_list.json");
    const PRODUCT: &[u8] = include_bytes!("../../../fixtures/sources/nws_climate/cli_product.json");

    fn ts(ms: i64) -> UtcTimestamp {
        UtcTimestamp::from_epoch_millis(ms).unwrap()
    }

    // --- pure parsers (fixtures-first) -----------------------------------

    #[test]
    fn parses_the_cli_list_into_product_urls() {
        let entries = parse_list(LIST).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries[0]
            .url
            .starts_with("https://api.weather.gov/products/"));
    }

    #[test]
    fn parses_a_cli_product_with_raw_text_and_report_date() {
        let s = parse_product(PRODUCT, ts(1000)).unwrap();
        assert_eq!(s.kind, NWS_CLI_KIND);
        assert_eq!(s.received_at, ts(1000));
        // Raw text preserved (the authoritative record).
        assert!(s.payload["productText"]
            .as_str()
            .unwrap()
            .contains("MAXIMUM"));
        // Report date parsed: "12 JUNE 2026" -> 2026-06-12.
        assert_eq!(s.payload["report_date"].as_str(), Some("2026-06-12"));
    }

    #[test]
    fn report_date_is_none_when_absent_not_an_error() {
        assert_eq!(parse_report_date("no summary line here"), None);
        // A product with no summary line still parses (raw text is authority).
        let body = serde_json::to_vec(&json!({
            "id": "x", "issuanceTime": "2026-06-13T17:00:00+00:00",
            "productText": "SOME MALFORMED REPORT"
        }))
        .unwrap();
        let s = parse_product(&body, ts(0)).unwrap();
        assert!(s.payload["report_date"].is_null());
    }

    #[test]
    fn claimed_time_reads_issuance_time() {
        let s = parse_product(PRODUCT, ts(0)).unwrap();
        assert!(nws_climate_claimed_time(&s).is_some());
        // wrong kind -> none
        let other = RawSignal {
            kind: "nws.afd".into(),
            payload: json!({"issuanceTime": "2026-06-13T17:00:00+00:00"}),
            received_at: ts(0),
        };
        assert!(nws_climate_claimed_time(&other).is_none());
    }

    // --- two-hop Source::fetch -------------------------------------------

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

    fn ok(body: &[u8]) -> RawHttpResponse {
        RawHttpResponse {
            status: 200,
            etag: Some("\"v1\"".into()),
            last_modified: None,
            location: None,
            body: body.to_vec(),
        }
    }

    fn source(
        responses: Vec<Result<RawHttpResponse, FetchError>>,
    ) -> NwsClimateSource<ScriptedTransport> {
        let pin = HostPin::from_url("https://api.weather.gov/").unwrap();
        let client = FetchClient::new(
            ScriptedTransport {
                responses: Mutex::new(responses),
            },
            pin,
            60,
            FetchCaps::default(),
        );
        NwsClimateSource::new(
            "nws_cli",
            "https://api.weather.gov/products?type=CLI&limit=3",
            client,
            Arc::new(fortuna_core::clock::SimClock::new(ts(7000))),
        )
    }

    #[tokio::test]
    async fn two_hop_fetch_emits_cli_signals_and_dedups_next_tick() {
        // tick 1: list (3 distinct product ids) + each product's text.
        let mut src = source(vec![
            Ok(ok(LIST)),
            Ok(ok(PRODUCT)),
            Ok(ok(PRODUCT)),
            Ok(ok(PRODUCT)),
        ]);
        let out = src.fetch().await.unwrap();
        assert_eq!(out.len(), 3, "one signal per CLI product in the list");
        assert!(out.iter().all(|s| s.kind == NWS_CLI_KIND));
        assert!(out.iter().all(|s| s.received_at == ts(7000)));

        // tick 2: same list — all 3 ids now SEEN -> no new product fetch, no
        // signal. Only the list is scripted; a re-fetch would panic the mock.
        {
            let mut q = src.client.transport().responses.lock().unwrap();
            q.push(Ok(ok(LIST)));
        }
        let out2 = src.fetch().await.unwrap();
        assert!(out2.is_empty(), "already-seen products are not re-emitted");
    }

    #[tokio::test]
    async fn list_304_yields_no_signals() {
        let resp304 = RawHttpResponse {
            status: 304,
            etag: None,
            last_modified: None,
            location: None,
            body: vec![],
        };
        let mut src = source(vec![Ok(resp304)]);
        assert!(src.fetch().await.unwrap().is_empty());
    }
}
