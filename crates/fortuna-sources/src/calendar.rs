//! Macro release-calendar adapter (design §4.2): the BLS economic-release
//! schedule + latest-numbers feed. Two signal kinds, the pair the smart
//! release-aware cadence (design §3.4 of the Aeolus contract / D9) is built
//! around:
//!
//! - **`release_scheduled`** (from the BLS iCalendar `bls.ics`): an UPCOMING
//!   release with its scheduled UTC time. The scheduler reads `scheduled_at`
//!   to open a tight polling window around the release.
//! - **`release_printed`** (from `bls_latest.rss`): a release just published.
//!
//! Dumb like every adapter: fetch, parse, emit. The one piece of domain
//! knowledge it owns is the iCalendar timezone conversion (`DTSTART;TZID=
//! US-Eastern` naive local → UTC), which only code that knows the format can
//! do — done deterministically via `chrono-tz` (pinned tz db), no wall time.
//!
//! IMPORTANT (Layer-1 interaction): a `release_scheduled` time is INTENTIONALLY
//! in the future. It is DATA about a future event, not an "occurred-at" claim,
//! so `calendar_claimed_time` returns `None` for it — it must never trip the
//! scheduler's future-dated reject (received_at honesty: we learned of the
//! schedule now). `release_printed` carries a real past publish time.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{NaiveDateTime, TimeZone, Utc};
use fortuna_cognition::signals::{RawSignal, SignalError, Source};
use fortuna_core::clock::{Clock, UtcTimestamp};
use serde_json::{json, Value};

use crate::fetch::{Conditional, FetchClient, FetchOutcome, FetchTransport};
use crate::rss::parse_feed_kind;

pub const RELEASE_SCHEDULED_KIND: &str = "release_scheduled";
pub const RELEASE_PRINTED_KIND: &str = "release_printed";

/// Which BLS feed this instance polls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarFeed {
    /// `bls.ics` — the iCalendar release schedule → `release_scheduled`.
    Schedule,
    /// `bls_latest.rss` — latest published numbers → `release_printed`.
    LatestReleases,
}

pub struct CalendarSource<T: FetchTransport> {
    id: String,
    feed: CalendarFeed,
    url: String,
    client: FetchClient<T>,
    clock: Arc<dyn Clock>,
    etag: Option<String>,
    last_modified: Option<String>,
}

impl<T: FetchTransport> CalendarSource<T> {
    pub fn new(
        id: impl Into<String>,
        feed: CalendarFeed,
        url: impl Into<String>,
        client: FetchClient<T>,
        clock: Arc<dyn Clock>,
    ) -> CalendarSource<T> {
        CalendarSource {
            id: id.into(),
            feed,
            url: url.into(),
            client,
            clock,
            etag: None,
            last_modified: None,
        }
    }
}

#[async_trait]
impl<T: FetchTransport> Source for CalendarSource<T> {
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
                let result = match self.feed {
                    CalendarFeed::Schedule => parse_ics(&body, received_at),
                    CalendarFeed::LatestReleases => {
                        parse_feed_kind(&body, received_at, RELEASE_PRINTED_KIND)
                    }
                };
                result.map_err(|reason| SignalError::Fetch {
                    source_id: self.id.clone(),
                    reason,
                })
            }
        }
    }
}

/// Parse a BLS iCalendar body into `release_scheduled` signals. Pure and
/// deterministic (the tz conversion uses the pinned `chrono-tz` db, no wall
/// time). Fail-closed: a `VEVENT` with a `DTSTART` we cannot convert fails the
/// whole parse, surfacing format drift loudly rather than emitting a wrong
/// schedule.
fn parse_ics(body: &[u8], received_at: UtcTimestamp) -> Result<Vec<RawSignal>, String> {
    let text = std::str::from_utf8(body).map_err(|e| format!("ics not utf-8: {e}"))?;
    let unfolded = unfold(text);
    let mut signals = Vec::new();
    let mut in_event = false;
    let mut uid: Option<String> = None;
    let mut summary: Option<String> = None;
    let mut dtstart: Option<UtcTimestamp> = None;
    let mut categories: Vec<String> = Vec::new();
    for line in unfolded.lines() {
        match line {
            "BEGIN:VEVENT" => {
                in_event = true;
                uid = None;
                summary = None;
                dtstart = None;
                categories = Vec::new();
            }
            "END:VEVENT" => {
                in_event = false;
                // A schedule entry needs a time and a name to be useful.
                let (Some(at), Some(name)) = (dtstart, summary.clone()) else {
                    // Incomplete VEVENT (no DTSTART/SUMMARY) — skip it; this is
                    // a calendar housekeeping entry, not a release.
                    continue;
                };
                signals.push(RawSignal {
                    kind: RELEASE_SCHEDULED_KIND.to_string(),
                    payload: json!({
                        "uid": uid.clone(),
                        "release": name,
                        "scheduled_at": at.to_iso8601(),
                        "categories": categories.clone(),
                    }),
                    received_at,
                });
            }
            _ if in_event => {
                let (name, value) = split_property(line);
                match name {
                    "UID" => uid = Some(value.to_string()),
                    "SUMMARY" => summary = Some(unescape_text(value)),
                    "CATEGORIES" => {
                        categories = value
                            .split(',')
                            .map(|c| c.trim().to_string())
                            .filter(|c| !c.is_empty())
                            .collect();
                    }
                    n if n == "DTSTART" || n.starts_with("DTSTART;") => {
                        dtstart = Some(parse_dtstart(line)?);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(signals)
}

/// RFC 5545 line unfolding: a line beginning with a space or tab continues the
/// previous line. (BLS does not fold today, but real iCalendar does.)
fn unfold(text: &str) -> String {
    let mut out = String::new();
    for raw in text.replace("\r\n", "\n").split('\n') {
        if let Some(rest) = raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')) {
            out.push_str(rest);
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(raw);
        }
    }
    out
}

/// Split an iCalendar content line into (property-name, value). The name is
/// everything before the first `:` or `;`; the value is everything after the
/// first `:`. Returns the full property name including params for DTSTART
/// detection upstream.
fn split_property(line: &str) -> (&str, &str) {
    let name_end = line.find([':', ';']).unwrap_or(line.len());
    let name = &line[..name_end];
    let value = line.split_once(':').map(|(_, v)| v).unwrap_or("");
    (name, value)
}

/// Parse a `DTSTART...` line to UTC. Handles `DTSTART:...Z` (already UTC) and
/// `DTSTART;TZID=US-Eastern:...` (naive local in the named zone). A floating
/// time (no `Z`, no `TZID`) is refused — we never guess a zone.
fn parse_dtstart(line: &str) -> Result<UtcTimestamp, String> {
    let (params, value) = line
        .split_once(':')
        .ok_or_else(|| format!("DTSTART has no value: `{line}`"))?;
    if let Some(naive_str) = value.strip_suffix('Z') {
        let naive = parse_ics_naive(naive_str)?;
        return millis_to_ts(Utc.from_utc_datetime(&naive).timestamp_millis());
    }
    if let Some(tzid) = params.split("TZID=").nth(1).map(|s| {
        s.split(';').next().unwrap_or(s) // drop any trailing params
    }) {
        let tz = map_tzid(tzid)?;
        let naive = parse_ics_naive(value)?;
        // `.earliest()` resolves a fall-back ambiguous local time to its first
        // occurrence and any normal time to itself; only a spring-forward gap
        // (no such local time) yields None. Release times are on the hour and
        // never land in the gap, but this is the safe, deterministic choice.
        let dt = tz
            .from_local_datetime(&naive)
            .earliest()
            .ok_or_else(|| format!("DTSTART `{value}` falls in a DST gap for {tzid}"))?;
        return millis_to_ts(dt.with_timezone(&Utc).timestamp_millis());
    }
    Err(format!(
        "DTSTART `{value}` has neither Z nor TZID (floating time unsupported)"
    ))
}

fn parse_ics_naive(s: &str) -> Result<NaiveDateTime, String> {
    NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S")
        .map_err(|e| format!("bad iCalendar datetime `{s}`: {e}"))
}

fn millis_to_ts(millis: i64) -> Result<UtcTimestamp, String> {
    UtcTimestamp::from_epoch_millis(millis).map_err(|e| format!("datetime out of range: {e}"))
}

/// Map an iCalendar TZID to a tz-db zone. BLS uses the non-standard
/// `US-Eastern`; its VTIMEZONE block defines exactly the standard US Eastern
/// DST rules, i.e. `America/New_York`. Unknown zones are refused, not guessed.
fn map_tzid(tzid: &str) -> Result<chrono_tz::Tz, String> {
    match tzid {
        "US-Eastern" | "US/Eastern" | "America/New_York" => Ok(chrono_tz::America::New_York),
        other => Err(format!("unsupported TZID `{other}` (no zone mapping)")),
    }
}

/// Minimal iCalendar TEXT unescaping for SUMMARY (`\,` `\;` `\\` `\n`).
fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(other) => out.push(other), // \, \; \\ -> the literal char
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Claimed-time for the Layer-1 future-dated check. `release_scheduled`
/// returns `None` ON PURPOSE — its `scheduled_at` is a future time carried as
/// data, not an occurred-at claim, and must not be rejected as future-dated.
/// `release_printed` returns its real (past) publish time.
pub fn calendar_claimed_time(signal: &RawSignal) -> Option<UtcTimestamp> {
    match signal.kind.as_str() {
        RELEASE_SCHEDULED_KIND => None,
        RELEASE_PRINTED_KIND => {
            let raw = signal
                .payload
                .get("published")
                .and_then(Value::as_str)
                .or_else(|| signal.payload.get("updated").and_then(Value::as_str))?;
            UtcTimestamp::parse_iso8601(raw).ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::{FetchCaps, FetchError, HostPin, RawHttpResponse};
    use std::sync::Mutex;

    const SCHEDULE: &[u8] = include_bytes!("../../../fixtures/sources/calendar/bls_schedule.ics");
    const LATEST: &[u8] = include_bytes!("../../../fixtures/sources/calendar/bls_latest.rss");

    fn ts(ms: i64) -> UtcTimestamp {
        UtcTimestamp::from_epoch_millis(ms).unwrap()
    }

    // --- iCalendar schedule parse (fixtures-first) -----------------------

    #[test]
    fn parses_real_bls_schedule_with_correct_dst_offsets() {
        let signals = parse_ics(SCHEDULE, ts(1000)).unwrap();
        assert_eq!(signals.len(), 2);
        for s in &signals {
            assert_eq!(s.kind, RELEASE_SCHEDULED_KIND);
            assert_eq!(s.received_at, ts(1000));
            assert!(s.payload["release"].as_str().is_some());
            assert!(s.payload["scheduled_at"].as_str().unwrap().ends_with('Z'));
        }
        // The January event is 10:00 Eastern in EST (UTC−5) -> 15:00:00Z.
        let jan = signals
            .iter()
            .find(|s| {
                s.payload["scheduled_at"]
                    .as_str()
                    .unwrap()
                    .starts_with("2026-01")
            })
            .expect("january event");
        assert_eq!(
            jan.payload["scheduled_at"].as_str().unwrap(),
            "2026-01-07T15:00:00.000Z"
        );
        // The July event is 10:00 Eastern in EDT (UTC−4) -> 14:00:00Z.
        let jul = signals
            .iter()
            .find(|s| {
                s.payload["scheduled_at"]
                    .as_str()
                    .unwrap()
                    .starts_with("2026-07")
            })
            .expect("july event");
        assert_eq!(
            jul.payload["scheduled_at"].as_str().unwrap(),
            "2026-07-01T14:00:00.000Z"
        );
    }

    #[test]
    fn dtstart_utc_z_form_is_supported() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:x\nDTSTART:20260612T123000Z\nSUMMARY:CPI\nEND:VEVENT\nEND:VCALENDAR\n";
        let s = parse_ics(ics.as_bytes(), ts(0)).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(
            s[0].payload["scheduled_at"].as_str().unwrap(),
            "2026-06-12T12:30:00.000Z"
        );
    }

    #[test]
    fn floating_time_and_unknown_tz_are_refused() {
        let floating = "BEGIN:VEVENT\nUID:x\nDTSTART:20260612T123000\nSUMMARY:CPI\nEND:VEVENT\n";
        assert!(parse_ics(floating.as_bytes(), ts(0)).is_err());
        let badtz = "BEGIN:VEVENT\nUID:x\nDTSTART;TZID=Mars/Olympus:20260612T123000\nSUMMARY:CPI\nEND:VEVENT\n";
        assert!(parse_ics(badtz.as_bytes(), ts(0)).is_err());
    }

    #[test]
    fn incomplete_vevent_is_skipped_not_emitted() {
        // No DTSTART -> not a release; skipped, not an error.
        let ics = "BEGIN:VEVENT\nUID:x\nSUMMARY:housekeeping\nEND:VEVENT\n";
        assert!(parse_ics(ics.as_bytes(), ts(0)).unwrap().is_empty());
    }

    #[test]
    fn summary_escapes_are_unescaped() {
        let ics =
            "BEGIN:VEVENT\nUID:x\nDTSTART:20260101T000000Z\nSUMMARY:Jobs\\, etc\nEND:VEVENT\n";
        let s = parse_ics(ics.as_bytes(), ts(0)).unwrap();
        assert_eq!(s[0].payload["release"].as_str().unwrap(), "Jobs, etc");
    }

    // --- latest-releases RSS parse ---------------------------------------

    #[test]
    fn parses_latest_releases_rss_as_release_printed() {
        let signals = parse_feed_kind(LATEST, ts(2000), RELEASE_PRINTED_KIND).unwrap();
        assert!(!signals.is_empty());
        assert_eq!(signals[0].kind, RELEASE_PRINTED_KIND);
        assert!(signals[0].payload["published"].as_str().is_some());
    }

    // --- claimed-time semantics (the load-bearing nuance) ----------------

    #[test]
    fn scheduled_claimed_time_is_none_so_it_never_trips_the_future_check() {
        let scheduled = &parse_ics(SCHEDULE, ts(0)).unwrap()[0];
        // Even though scheduled_at is a real future time, the Layer-1 input is
        // None (it is data about the future, not an occurred-at claim).
        assert!(calendar_claimed_time(scheduled).is_none());
    }

    #[test]
    fn printed_claimed_time_is_the_publish_time() {
        let printed = &parse_feed_kind(LATEST, ts(0), RELEASE_PRINTED_KIND).unwrap()[0];
        assert!(calendar_claimed_time(printed).is_some());
    }

    // --- full Source::fetch path -----------------------------------------

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

    fn source(feed: CalendarFeed, body: &[u8]) -> CalendarSource<ScriptedTransport> {
        let pin = HostPin::from_url("https://www.bls.gov/").unwrap();
        let client = FetchClient::new(
            ScriptedTransport {
                responses: Mutex::new(vec![Ok(ok(body))]),
            },
            pin,
            60,
            FetchCaps::default(),
        );
        CalendarSource::new(
            "bls_schedule",
            feed,
            "https://www.bls.gov/schedule/news_release/bls.ics",
            client,
            Arc::new(fortuna_core::clock::SimClock::new(ts(9000))),
        )
    }

    #[tokio::test]
    async fn fetch_schedule_emits_release_scheduled() {
        let mut src = source(CalendarFeed::Schedule, SCHEDULE);
        let signals = src.fetch().await.unwrap();
        assert_eq!(signals.len(), 2);
        assert_eq!(signals[0].kind, RELEASE_SCHEDULED_KIND);
        assert_eq!(signals[0].received_at, ts(9000));
    }

    #[tokio::test]
    async fn fetch_latest_emits_release_printed() {
        let mut src = source(CalendarFeed::LatestReleases, LATEST);
        let signals = src.fetch().await.unwrap();
        assert!(!signals.is_empty());
        assert_eq!(signals[0].kind, RELEASE_PRINTED_KIND);
    }
}
