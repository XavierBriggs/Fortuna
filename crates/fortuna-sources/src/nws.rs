//! NWS adapter (design §4.2): api.weather.gov — U.S. Government public-domain
//! weather data (forecast discussions + active alerts). Dossier:
//! `docs/research/sources/nws/dossier.md`.
//!
//! It implements the cognition `Source` trait and nothing more: fetch the one
//! configured endpoint through the shared [`FetchClient`] (politeness, host
//! pin, conditional GET — all upstream), parse the real response shape into
//! `RawSignal`s, emit. It is deliberately dumb — dedup, trust weighting, and
//! triggering all live downstream (spec 5.11: "an adapter that wants to be
//! clever is doing the normalizer's or trigger engine's job").
//!
//! The one piece of NWS-specific knowledge beyond parsing is
//! [`nws_claimed_time`]: the scheduler (Layer 1, design §4.4) needs the
//! source-claimed event time to run the future-dated check, and only code
//! that knows the NWS shape can extract it. It lives here, with the parser.

use std::sync::Arc;

use async_trait::async_trait;
use fortuna_cognition::signals::{RawSignal, SignalError, Source};
use fortuna_core::clock::{Clock, UtcTimestamp};
use serde_json::Value;

use crate::fetch::{Conditional, FetchClient, FetchOutcome, FetchTransport};

/// Which NWS endpoint this instance polls. Each maps to one real response
/// shape captured under `fixtures/sources/nws/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NwsFeed {
    /// `/alerts/active...` — GeoJSON `FeatureCollection`; one signal per
    /// feature, kind `nws.alert`.
    AlertsActive,
    /// `/products?type=AFD...` — JSON-LD `@graph` list of product summaries;
    /// one signal per entry, kind `nws.afd`.
    AfdProducts,
}

impl NwsFeed {
    fn signal_kind(self) -> &'static str {
        match self {
            NwsFeed::AlertsActive => "nws.alert",
            NwsFeed::AfdProducts => "nws.afd",
        }
    }
}

/// The NWS source adapter. Holds the conditional-GET validators across polls
/// so steady-state polling of an unchanged feed costs an empty 304.
pub struct NwsSource<T: FetchTransport> {
    id: String,
    feed: NwsFeed,
    url: String,
    client: FetchClient<T>,
    clock: Arc<dyn Clock>,
    etag: Option<String>,
    last_modified: Option<String>,
}

impl<T: FetchTransport> NwsSource<T> {
    pub fn new(
        id: impl Into<String>,
        feed: NwsFeed,
        url: impl Into<String>,
        client: FetchClient<T>,
        clock: Arc<dyn Clock>,
    ) -> NwsSource<T> {
        NwsSource {
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
impl<T: FetchTransport> Source for NwsSource<T> {
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
                // Refresh conditional validators for the next poll.
                self.etag = etag;
                self.last_modified = last_modified;
                parse(self.feed, &body, received_at).map_err(|reason| SignalError::Fetch {
                    source_id: self.id.clone(),
                    reason,
                })
            }
        }
    }
}

/// Parse a fetched NWS body into `RawSignal`s. Pure (no IO, no clock read —
/// `received_at` is supplied), so it is tested directly against the recorded
/// fixtures. The payload is the raw JSON object passed through untouched
/// (untrusted content, spec 5.11): this adapter interprets nothing.
fn parse(feed: NwsFeed, body: &[u8], received_at: UtcTimestamp) -> Result<Vec<RawSignal>, String> {
    let root: Value =
        serde_json::from_slice(body).map_err(|e| format!("nws body is not JSON: {e}"))?;
    let kind = feed.signal_kind();
    let items = match feed {
        NwsFeed::AlertsActive => root.get("features").and_then(Value::as_array),
        NwsFeed::AfdProducts => root.get("@graph").and_then(Value::as_array),
    };
    let Some(items) = items else {
        // An NWS error envelope (problem+json) or any unexpected shape lands
        // here — surfaced as a fetch error, never silently emitted as signal.
        let container = match feed {
            NwsFeed::AlertsActive => "features[]",
            NwsFeed::AfdProducts => "@graph[]",
        };
        return Err(format!(
            "nws {kind}: expected `{container}` array; got {}",
            shape_hint(&root)
        ));
    };
    Ok(items
        .iter()
        .map(|item| RawSignal {
            kind: kind.to_string(),
            payload: item.clone(),
            received_at,
        })
        .collect())
}

fn shape_hint(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            // Surface a few keys so an error envelope is recognizable in logs.
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.truncate(6);
            format!("object with keys {keys:?}")
        }
        Value::Array(_) => "a bare array".to_string(),
        other => format!("a {}", type_name(other)),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Extract the source-claimed event/publish time from an NWS signal, for the
/// Layer 1 future-dated check (design §4.4). NWS times are RFC3339 with an
/// offset; `UtcTimestamp` normalizes them to UTC. Returns `None` when the
/// field is absent or unparseable (the validator then simply skips the
/// future check for that item — fail-open on a missing optional, never panic).
pub fn nws_claimed_time(signal: &RawSignal) -> Option<UtcTimestamp> {
    let raw = match signal.kind.as_str() {
        "nws.alert" => signal.payload.get("properties")?.get("sent")?,
        "nws.afd" => signal.payload.get("issuanceTime")?,
        _ => return None,
    };
    UtcTimestamp::parse_iso8601(raw.as_str()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::{FetchCaps, FetchError, HostPin, RawHttpResponse};
    use std::sync::Mutex;

    const ALERTS: &[u8] = include_bytes!("../../../fixtures/sources/nws/alerts_active.json");
    const AFD_LIST: &[u8] = include_bytes!("../../../fixtures/sources/nws/afd_list.json");
    const ERROR_400: &[u8] = include_bytes!("../../../fixtures/sources/nws/error_400.json");

    fn ts(ms: i64) -> UtcTimestamp {
        UtcTimestamp::from_epoch_millis(ms).unwrap()
    }

    // --- pure parser tests (fixtures-first) ------------------------------

    #[test]
    fn parses_real_alert_featurecollection() {
        let signals = parse(NwsFeed::AlertsActive, ALERTS, ts(1000)).unwrap();
        assert_eq!(signals.len(), 1);
        let s = &signals[0];
        assert_eq!(s.kind, "nws.alert");
        assert_eq!(s.received_at, ts(1000));
        assert_eq!(
            s.payload["properties"]["event"].as_str(),
            Some("Severe Thunderstorm Warning")
        );
    }

    #[test]
    fn parses_real_afd_product_list() {
        let signals = parse(NwsFeed::AfdProducts, AFD_LIST, ts(2000)).unwrap();
        assert_eq!(signals.len(), 2);
        assert_eq!(signals[0].kind, "nws.afd");
        assert_eq!(signals[0].payload["issuingOffice"].as_str(), Some("KLUB"));
        assert_eq!(signals[0].payload["productCode"].as_str(), Some("AFD"));
    }

    #[test]
    fn an_error_envelope_is_a_fetch_error_not_a_signal() {
        // The 400 problem+json has no `features` array.
        let err = parse(NwsFeed::AlertsActive, ERROR_400, ts(0)).unwrap_err();
        assert!(err.contains("features[]"), "{err}");
        assert!(err.contains("keys"), "{err}");
    }

    #[test]
    fn non_json_body_is_a_fetch_error() {
        let err = parse(NwsFeed::AlertsActive, b"<html>down</html>", ts(0)).unwrap_err();
        assert!(err.contains("not JSON"), "{err}");
    }

    // --- claimed-time extraction (Layer 1 input) -------------------------

    #[test]
    fn claimed_time_reads_alert_sent_and_afd_issuance() {
        let alert = &parse(NwsFeed::AlertsActive, ALERTS, ts(0)).unwrap()[0];
        // Fixture `sent` is 2026-06-12T22:09:00-05:00 == 03:09:00Z next day.
        let got = nws_claimed_time(alert).expect("alert has a sent time");
        assert_eq!(
            got,
            UtcTimestamp::parse_iso8601("2026-06-13T03:09:00Z").unwrap()
        );

        let afd = &parse(NwsFeed::AfdProducts, AFD_LIST, ts(0)).unwrap()[0];
        let got = nws_claimed_time(afd).expect("afd has an issuanceTime");
        assert_eq!(
            got,
            UtcTimestamp::parse_iso8601("2026-06-13T03:30:00Z").unwrap()
        );
    }

    #[test]
    fn claimed_time_is_none_for_unknown_kind_or_missing_field() {
        let s = RawSignal {
            kind: "nws.other".into(),
            payload: serde_json::json!({}),
            received_at: ts(0),
        };
        assert!(nws_claimed_time(&s).is_none());
        let s = RawSignal {
            kind: "nws.alert".into(),
            payload: serde_json::json!({"properties": {}}),
            received_at: ts(0),
        };
        assert!(nws_claimed_time(&s).is_none());
    }

    // --- full Source::fetch path through a scripted transport ------------

    struct ScriptedTransport {
        responses: Mutex<Vec<Result<RawHttpResponse, FetchError>>>,
        seen_conditionals: Mutex<Vec<Conditional>>,
    }

    impl ScriptedTransport {
        fn new(responses: Vec<Result<RawHttpResponse, FetchError>>) -> ScriptedTransport {
            ScriptedTransport {
                responses: Mutex::new(responses),
                seen_conditionals: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl FetchTransport for ScriptedTransport {
        async fn get(
            &self,
            _url: &str,
            conditional: &Conditional,
        ) -> Result<RawHttpResponse, FetchError> {
            self.seen_conditionals
                .lock()
                .unwrap()
                .push(conditional.clone());
            self.responses.lock().unwrap().remove(0)
        }
    }

    fn body(status: u16, etag: Option<&str>, body: &[u8]) -> RawHttpResponse {
        RawHttpResponse {
            status,
            etag: etag.map(String::from),
            last_modified: None,
            location: None,
            body: body.to_vec(),
        }
    }

    fn source(transport: ScriptedTransport, clock: Arc<dyn Clock>) -> NwsSource<ScriptedTransport> {
        let pin = HostPin::from_url("https://api.weather.gov/").unwrap();
        let client = FetchClient::new(transport, pin, 60, FetchCaps::default());
        NwsSource::new(
            "nws_alerts_tx",
            NwsFeed::AlertsActive,
            "https://api.weather.gov/alerts/active?area=TX",
            client,
            clock,
        )
    }

    #[tokio::test]
    async fn fetch_emits_signals_stamped_with_clock_now() {
        let clock = Arc::new(fortuna_core::clock::SimClock::new(ts(5000)));
        let t = ScriptedTransport::new(vec![Ok(body(200, Some("\"v1\""), ALERTS))]);
        let mut src = source(t, clock);
        let signals = src.fetch().await.unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].received_at, ts(5000));
        assert_eq!(signals[0].kind, "nws.alert");
    }

    #[tokio::test]
    async fn conditional_get_304_yields_no_signals_and_sends_stored_validators() {
        let clock = Arc::new(fortuna_core::clock::SimClock::new(ts(0)));
        let t = ScriptedTransport::new(vec![
            Ok(body(200, Some("\"etag-A\""), ALERTS)),
            Ok(body(304, None, b"")),
        ]);
        // Keep a handle to inspect the conditionals the transport saw.
        let pin = HostPin::from_url("https://api.weather.gov/").unwrap();
        let client = FetchClient::new(t, pin, 60, FetchCaps::default());
        let mut src = NwsSource::new(
            "nws",
            NwsFeed::AlertsActive,
            "https://api.weather.gov/alerts/active?area=TX",
            client,
            clock,
        );
        // First poll: 200, stores the etag, emits a signal.
        assert_eq!(src.fetch().await.unwrap().len(), 1);
        // Second poll: 304, no signals.
        assert_eq!(src.fetch().await.unwrap().len(), 0);
        // The second request must have carried the stored etag.
        let conds = src.client.transport().seen_conditionals.lock().unwrap();
        assert_eq!(conds[0].etag, None, "first poll sends no validator");
        assert_eq!(
            conds[1].etag.as_deref(),
            Some("\"etag-A\""),
            "second poll sends the stored etag"
        );
    }

    #[tokio::test]
    async fn transport_outage_maps_to_signal_error() {
        let clock = Arc::new(fortuna_core::clock::SimClock::new(ts(0)));
        let t = ScriptedTransport::new(vec![Err(FetchError::Outage {
            reason: "dns".into(),
        })]);
        let mut src = source(t, clock);
        let err = src.fetch().await.unwrap_err();
        assert!(matches!(err, SignalError::Fetch { .. }));
    }
}
