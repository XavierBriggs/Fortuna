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
//! The ADAPTER is deliberately DUMB about the temperatures: it carries the raw
//! authoritative text + a `report_date`, never a derived high/low. The GRADER —
//! [`nws_cli_realized`] in this module — does the high-stakes extraction at
//! settlement: the CLI text is fragile (columns jam, e.g. `MINIMUM 7676` =
//! observed 76 + record 76; missing days print `MM`; record ties print `91R`),
//! and a mis-read daily high would mis-GRADE a belief, so the grader is
//! FAIL-LOUD — any ambiguity (a jam, an `MM`, an absent line, an inverted
//! high<low, an unparseable date) returns `None`, never a fabricated temperature
//! (spec 5.12). F9 (`fortuna_cognition::aeolus_reliability::score_reliability`)
//! consumes the realized °F this produces; the grader lives source-side (here)
//! because the dependency runs sources → cognition, and the composition layer
//! bridges the realized value across.

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

/// Extract the date a CLI report covers from its `... CLIMATE SUMMARY FOR ...`
/// line → `YYYY-MM-DD`. Order-independent: NWS offices write either
/// `<day> <MONTH> <year>` (e.g. Palau `12 JUNE 2026`) or `<MONTH> <day> <year>`
/// (mainland `JUNE 13 2026` / abbreviated `JUN 13 2026`), and the line may carry
/// trailing dots (`2026...`). The month is the recognizable name; the day is the
/// 1–31 integer; the year is the ≥1900 integer — so the three tokens parse
/// regardless of order. Returns `None` (not an error) if absent/unparseable —
/// the raw text remains the authority.
fn parse_report_date(text: &str) -> Option<String> {
    const MARK: &str = "CLIMATE SUMMARY FOR ";
    let idx = text.find(MARK)?;
    let line = text[idx + MARK.len()..].lines().next()?;
    let mut month: Option<u32> = None;
    let mut ints: Vec<i64> = Vec::new();
    for tok in line.split_whitespace().take(4) {
        if let Some(m) = month_number(tok) {
            month.get_or_insert(m);
        } else if let Some(n) = leading_int(tok) {
            ints.push(n);
        }
    }
    let month = month?;
    let day = ints.iter().copied().find(|&n| (1..=31).contains(&n))?;
    let year = ints.iter().copied().find(|&n| n >= 1900)?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// Month number from a full or 3-letter-abbreviated NWS month name (`JUNE` and
/// `JUN` → 6). Matches the leading three letters, so both forms map identically.
fn month_number(name: &str) -> Option<u32> {
    let key: String = name
        .chars()
        .take(3)
        .collect::<String>()
        .to_ascii_uppercase();
    match key.as_str() {
        "JAN" => Some(1),
        "FEB" => Some(2),
        "MAR" => Some(3),
        "APR" => Some(4),
        "MAY" => Some(5),
        "JUN" => Some(6),
        "JUL" => Some(7),
        "AUG" => Some(8),
        "SEP" => Some(9),
        "OCT" => Some(10),
        "NOV" => Some(11),
        "DEC" => Some(12),
        _ => None,
    }
}

/// Leading signed integer of a token: `91R` → 91 (a record-tie flag), `-5` → -5,
/// `2026...` → 2026, `MM` → `None` (missing data), `` → `None`.
fn leading_int(token: &str) -> Option<i64> {
    let bytes = token.as_bytes();
    let neg = bytes.first() == Some(&b'-');
    let start = usize::from(neg);
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == start {
        return None;
    }
    let val: i64 = token[start..end].parse().ok()?;
    Some(if neg { -val } else { val })
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

/// The official observed daily extreme graded from one NWS CLI product — the
/// independent RESOLUTION value an Aeolus weather belief is scored against
/// (Aeolus contract §3.2/§5.12). `high_f`/`low_f` are integer °F (the official
/// record is integer); `realized_f: f64` for F9 is `high_f as f64` (TMAX
/// brackets) or `low_f as f64` (TMIN) — the caller picks per the event's
/// variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealizedExtreme {
    /// The station this realized value is keyed to (caller-supplied; the product
    /// is routed to its station upstream).
    pub station: String,
    /// The date the report covers, `YYYY-MM-DD`.
    pub report_date: String,
    pub high_f: i64,
    pub low_f: i64,
}

/// Plausible observed surface-temperature range (°F). A value outside this is a
/// jammed column (`7676`), a clock time, or garbage — never a real daily
/// extreme, so it is flagged (`None`), never graded.
const TEMP_FLOOR_F: i64 = -80;
const TEMP_CEIL_F: i64 = 140;

/// Grade the official daily MAX/MIN (°F) for `station` out of a CLI product's
/// `productText`. FAIL-LOUD: returns `None` on ANY ambiguity — an absent
/// MAXIMUM/MINIMUM line, a missing value (`MM`), a jammed/implausible value
/// (`7676`), an inverted high<low, or an unparseable report date — NEVER a
/// fabricated temperature (spec 5.12; the F2 deferral: ambiguity is flagged, not
/// silently mis-graded). Pure + deterministic; no `Clock::now`, no panic.
pub fn nws_cli_realized(product_text: &str, station: &str) -> Option<RealizedExtreme> {
    let report_date = parse_report_date(product_text)?;
    let high_f = daily_extreme(product_text, "MAXIMUM")?;
    let low_f = daily_extreme(product_text, "MINIMUM")?;
    // An inverted extreme means the parse latched onto the wrong columns — flag
    // it rather than grade a belief against a corrupt read.
    if high_f < low_f {
        return None;
    }
    Some(RealizedExtreme {
        station: station.to_string(),
        report_date,
        high_f,
        low_f,
    })
}

/// The observed value on the FIRST `<keyword> <number> …` line — the DAILY
/// extreme. The monthly `MAXIMUM TEMPERATURE (F) …` rows have a non-numeric next
/// token and are skipped; a missing daily value (`MM`) is skipped too, so a
/// product whose daily extreme is missing yields `None` (never the monthly
/// value). The matched value's leading integer is taken (`91R` → 91) and
/// range-validated: an out-of-range read (a jam like `7676`) is flagged `None`,
/// never silently used.
fn daily_extreme(text: &str, keyword: &str) -> Option<i64> {
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix(keyword) else {
            continue;
        };
        let Some(token) = rest.split_whitespace().next() else {
            continue;
        };
        // A non-numeric next token ("TEMPERATURE", "MM") is not a daily-value
        // line — skip and keep scanning.
        let Some(value) = leading_int(token) else {
            continue;
        };
        // A matched daily-value line: range-validate. Out of range (a jam) is an
        // ambiguity → flag it, do not fall through to a later/monthly row.
        return (TEMP_FLOOR_F..=TEMP_CEIL_F)
            .contains(&value)
            .then_some(value);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::{FetchCaps, FetchError, HostPin, RawHttpResponse};
    use std::sync::Mutex;

    const LIST: &[u8] = include_bytes!("../../../fixtures/sources/nws_climate/cli_list.json");
    const PRODUCT: &[u8] = include_bytes!("../../../fixtures/sources/nws_climate/cli_product.json");
    // Real captures (2026-06-14): clean mainland CLI products for the grader.
    // Troutdale KPQR uses `JUNE 13 2026` (month-day); Pago Pago NSTU uses the
    // abbreviated `JUN 13 2026`. Both have unambiguous daily MAX/MIN.
    const TROUTDALE: &[u8] =
        include_bytes!("../../../fixtures/sources/nws_climate/cli_product_troutdale.json");
    const PAGO: &[u8] =
        include_bytes!("../../../fixtures/sources/nws_climate/cli_product_pago.json");

    fn product_text(fixture: &[u8]) -> String {
        let v: Value = serde_json::from_slice(fixture).unwrap();
        v["productText"].as_str().unwrap().to_string()
    }

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

    // --- the grader: nws_cli_realized (fixtures-first, fail-loud) ---------

    #[test]
    fn grades_a_clean_cli_product_to_the_exact_high_low() {
        // Troutdale KPQR, "JUNE 13 2026": MAXIMUM 91, MINIMUM 50.
        let r = nws_cli_realized(&product_text(TROUTDALE), "KPDX").unwrap();
        assert_eq!(r.station, "KPDX");
        assert_eq!(r.report_date, "2026-06-13");
        assert_eq!(r.high_f, 91);
        assert_eq!(r.low_f, 50);
    }

    #[test]
    fn grades_an_abbreviated_month_product() {
        // Pago Pago NSTU, "JUN 13 2026": MAXIMUM 82, MINIMUM 75.
        let r = nws_cli_realized(&product_text(PAGO), "NSTU").unwrap();
        assert_eq!(r.report_date, "2026-06-13");
        assert_eq!(r.high_f, 82);
        assert_eq!(r.low_f, 75);
    }

    #[test]
    fn jammed_minimum_column_is_flagged_not_graded() {
        // The real PTKR product: MINIMUM `7676` (obs 76 + record 76 jammed) is an
        // out-of-range read, so the WHOLE grade is None — never a fabricated 76.
        assert!(nws_cli_realized(&product_text(PRODUCT), "PTKR").is_none());
    }

    #[test]
    fn dropping_the_maximum_line_reds_the_grade() {
        // Mutation guard: the grade genuinely depends on the MAXIMUM line.
        let full = product_text(TROUTDALE);
        let mutated: String = full
            .lines()
            .filter(|l| !l.trim_start().starts_with("MAXIMUM "))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(nws_cli_realized(&full, "KPDX").is_some());
        assert!(
            nws_cli_realized(&mutated, "KPDX").is_none(),
            "no MAXIMUM line must fail loud"
        );
    }

    #[test]
    fn missing_value_mm_is_flagged() {
        // `MM` = the daily observation is missing — never grab the record after it.
        let t =
            "CLIMATE SUMMARY FOR JUNE 13 2026\nMAXIMUM   MM   96   1995\nMINIMUM   54   456 AM\n";
        assert!(nws_cli_realized(t, "X").is_none());
    }

    #[test]
    fn no_temperature_section_is_flagged() {
        let t = "CLIMATE SUMMARY FOR JUNE 13 2026\nPRECIPITATION (INCHES)\nYESTERDAY 0.00\n";
        assert!(nws_cli_realized(t, "X").is_none());
    }

    #[test]
    fn inverted_high_below_low_is_flagged() {
        let t = "CLIMATE SUMMARY FOR JUNE 13 2026\nMAXIMUM   40\nMINIMUM   60\n";
        assert!(nws_cli_realized(t, "X").is_none());
    }

    #[test]
    fn unparseable_report_date_is_flagged() {
        let t = "DAILY CLIMATE REPORT\nMAXIMUM   88\nMINIMUM   60\n";
        assert!(nws_cli_realized(t, "X").is_none());
    }

    #[test]
    fn report_date_handles_both_token_orders_and_abbreviations() {
        assert_eq!(
            parse_report_date("CLIMATE SUMMARY FOR 12 JUNE  2026\n"),
            Some("2026-06-12".to_string())
        );
        assert_eq!(
            parse_report_date("CLIMATE SUMMARY FOR JUNE 13 2026...\n"),
            Some("2026-06-13".to_string())
        );
        assert_eq!(
            parse_report_date("CLIMATE SUMMARY FOR JUN 13 2026...\n"),
            Some("2026-06-13".to_string())
        );
    }

    #[test]
    fn leading_int_strips_flags_and_rejects_missing() {
        assert_eq!(leading_int("91R"), Some(91));
        assert_eq!(leading_int("-5"), Some(-5));
        assert_eq!(leading_int("2026..."), Some(2026));
        assert_eq!(leading_int("MM"), None);
        assert_eq!(leading_int(""), None);
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
