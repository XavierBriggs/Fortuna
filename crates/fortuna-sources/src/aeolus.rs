//! Aeolus forecast adapter (design: docs/design/aeolus-fortuna-source-contract.md
//! §4, the F3 task). Aeolus is FORTUNA's PROPRIETARY, operator-owned
//! probabilistic temperature-forecast source. Dossier:
//! `docs/research/sources/aeolus/dossier.md`.
//!
//! Like every adapter here it is deliberately DUMB (spec 5.11): it fetches the
//! one configured `/v2/forecasts` endpoint through the shared [`FetchClient`]
//! (politeness, host pin, conditional GET, and the `x-api-key` auth header —
//! all upstream), splits the `{"forecasts": [ <envelope>, ... ]}` transport
//! wrapper, and emits ONE `RawSignal` per envelope with the raw envelope JSON
//! passed through UNTOUCHED. It does NOT strict-parse the v2 shape, validate
//! σ/units/p, dedup, or trust-weight — those are downstream (cognition
//! `reconciliation.rs`, contract §4/§5). An adapter that wants to be clever is
//! doing the normalizer's or trigger engine's job.
//!
//! The one piece of Aeolus-shape knowledge beyond splitting the wrapper is
//! [`aeolus_claimed_time`]: the scheduler (Layer 1, design §4.4) needs the
//! source-claimed time to run the future-dated check. For a forecast that is
//! `run_at` — the forecast `init_time`, a PAST time (the point-in-time evidence
//! authority, contract §2). `valid_until`/freshness is cognition's concern, not
//! the adapter's.

use std::sync::Arc;

use async_trait::async_trait;
use fortuna_cognition::signals::{RawSignal, SignalError, Source};
use fortuna_core::clock::{Clock, UtcTimestamp};
use serde_json::Value;

use crate::fetch::{Conditional, FetchClient, FetchOutcome, FetchTransport};

/// The signal kind every Aeolus forecast envelope normalizes to. The
/// distribution/skill/resolution/brackets shape rides untouched in the payload;
/// the strict v2 parse happens downstream (contract §4).
pub const AEOLUS_FORECAST_KIND: &str = "aeolus.forecast";

/// The Aeolus source adapter. Holds the conditional-GET validators across polls
/// so steady-state polling of an unchanged forecast costs an empty 304
/// (Aeolus serves a forecast-scoped ETag, contract §3).
pub struct AeolusSource<T: FetchTransport> {
    id: String,
    url: String,
    client: FetchClient<T>,
    clock: Arc<dyn Clock>,
    etag: Option<String>,
    last_modified: Option<String>,
}

impl<T: FetchTransport> AeolusSource<T> {
    pub fn new(
        id: impl Into<String>,
        url: impl Into<String>,
        client: FetchClient<T>,
        clock: Arc<dyn Clock>,
    ) -> AeolusSource<T> {
        AeolusSource {
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
impl<T: FetchTransport> Source for AeolusSource<T> {
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
                parse(&body, received_at).map_err(|reason| SignalError::Fetch {
                    source_id: self.id.clone(),
                    reason,
                })
            }
        }
    }
}

/// Split a fetched Aeolus body — `{"forecasts": [ <envelope>, ... ]}` — into one
/// `RawSignal` per envelope. Pure (no IO, no clock read — `received_at` is
/// supplied), so it is tested directly against the recorded fixtures. The
/// payload is the raw envelope JSON passed through untouched (untrusted content,
/// spec 5.11): this adapter validates NONE of the v2 shape (that is the
/// downstream strict parse, contract §4). A non-JSON body or a missing
/// `forecasts` array is a fetch error, never silently emitted.
fn parse(body: &[u8], received_at: UtcTimestamp) -> Result<Vec<RawSignal>, String> {
    let root: Value =
        serde_json::from_slice(body).map_err(|e| format!("aeolus body is not JSON: {e}"))?;
    let forecasts = root
        .get("forecasts")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            // A typed error body `{ "error": { ... } }` (contract §3) or any
            // unexpected shape lands here — surfaced as a fetch error.
            format!(
                "aeolus {AEOLUS_FORECAST_KIND}: expected `forecasts[]` array; got {}",
                shape_hint(&root)
            )
        })?;
    Ok(forecasts
        .iter()
        .map(|envelope| RawSignal {
            kind: AEOLUS_FORECAST_KIND.to_string(),
            payload: envelope.clone(),
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
        Value::Null => "null".to_string(),
        Value::Bool(_) => "a bool".to_string(),
        Value::Number(_) => "a number".to_string(),
        Value::String(_) => "a string".to_string(),
    }
}

/// Extract the source-claimed time from an Aeolus signal, for the Layer 1
/// future-dated check (design §4.4). For a forecast that is `run_at` — the
/// forecast `init_time` (a PAST time; the point-in-time authority for the
/// belief's evidence, contract §2). Aeolus times are RFC3339 with an offset;
/// `UtcTimestamp` normalizes them to UTC. Returns `None` when the field is
/// absent or unparseable (the validator then skips the future check for that
/// item — fail-open on a missing optional, never panic).
pub fn aeolus_claimed_time(signal: &RawSignal) -> Option<UtcTimestamp> {
    if signal.kind != AEOLUS_FORECAST_KIND {
        return None;
    }
    let raw = signal.payload.get("run_at").and_then(Value::as_str)?;
    UtcTimestamp::parse_iso8601(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::{FetchCaps, FetchError, HostPin, RawHttpResponse};
    use std::sync::Mutex;

    const TMAX: &[u8] = include_bytes!("../../../fixtures/sources/aeolus/knyc_tmax.json");
    const TMIN: &[u8] = include_bytes!("../../../fixtures/sources/aeolus/knyc_tmin.json");

    fn ts(ms: i64) -> UtcTimestamp {
        UtcTimestamp::from_epoch_millis(ms).unwrap()
    }

    // --- pure parser tests (fixtures-first; the REAL Aeolus captures) -----

    #[test]
    fn parses_real_tmax_forecast_one_signal_raw_envelope() {
        let signals = parse(TMAX, ts(1000)).unwrap();
        assert_eq!(signals.len(), 1);
        let s = &signals[0];
        assert_eq!(s.kind, "aeolus.forecast");
        assert_eq!(s.received_at, ts(1000));
        // The raw envelope rides through untouched: μ is present (the load-
        // bearing payload) and the variable is tmax.
        assert!(s.payload["distribution"]["mu"].is_number());
        assert_eq!(s.payload["variable"].as_str(), Some("tmax"));
        // Schema string passes through unparsed (strict parse is downstream).
        assert_eq!(s.payload["schema"].as_str(), Some("aeolus.forecast/v2"));
    }

    #[test]
    fn parses_real_tmin_forecast_variable_tmin() {
        let signals = parse(TMIN, ts(2000)).unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].kind, "aeolus.forecast");
        assert_eq!(signals[0].payload["variable"].as_str(), Some("tmin"));
        assert!(signals[0].payload["distribution"]["mu"].is_number());
    }

    #[test]
    fn malformed_body_is_a_fetch_error_not_a_panic() {
        let err = parse(b"<html>down</html>", ts(0)).unwrap_err();
        assert!(err.contains("not JSON"), "{err}");
    }

    #[test]
    fn an_error_envelope_is_a_fetch_error_not_a_signal() {
        // A typed error body has no `forecasts` array.
        let body = br#"{"error":{"code":"unauthorized","message":"bad key"}}"#;
        let err = parse(body, ts(0)).unwrap_err();
        assert!(err.contains("forecasts[]"), "{err}");
        assert!(err.contains("keys"), "{err}");
    }

    #[test]
    fn empty_forecasts_array_yields_no_signals() {
        let signals = parse(br#"{"forecasts":[]}"#, ts(0)).unwrap();
        assert!(signals.is_empty());
    }

    // --- claimed-time extraction (Layer 1 input) -------------------------

    #[test]
    fn claimed_time_reads_run_at() {
        let s = &parse(TMAX, ts(0)).unwrap()[0];
        // Fixture run_at is 2026-06-13T00:00:00+00:00 == ...Z.
        let got = aeolus_claimed_time(s).expect("forecast has a run_at");
        assert_eq!(
            got,
            UtcTimestamp::parse_iso8601("2026-06-13T00:00:00Z").unwrap()
        );
    }

    #[test]
    fn claimed_time_is_none_for_wrong_kind_or_missing_field() {
        let other = RawSignal {
            kind: "nws.afd".into(),
            payload: serde_json::json!({"run_at": "2026-06-13T00:00:00+00:00"}),
            received_at: ts(0),
        };
        assert!(aeolus_claimed_time(&other).is_none());
        let no_field = RawSignal {
            kind: AEOLUS_FORECAST_KIND.into(),
            payload: serde_json::json!({}),
            received_at: ts(0),
        };
        assert!(aeolus_claimed_time(&no_field).is_none());
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

    fn source(
        transport: ScriptedTransport,
        clock: Arc<dyn Clock>,
    ) -> AeolusSource<ScriptedTransport> {
        let pin = HostPin::from_url("https://forecasts.aeolus.internal/").unwrap();
        let client = FetchClient::new(transport, pin, 60, FetchCaps::default());
        AeolusSource::new(
            "aeolus_knyc_tmax",
            "https://forecasts.aeolus.internal/v2/forecasts?station=KNYC&variable=tmax",
            client,
            clock,
        )
    }

    #[tokio::test]
    async fn fetch_emits_signal_stamped_with_clock_now() {
        let clock = Arc::new(fortuna_core::clock::SimClock::new(ts(5000)));
        let t = ScriptedTransport::new(vec![Ok(body(200, Some("\"v1\""), TMAX))]);
        let mut src = source(t, clock);
        let signals = src.fetch().await.unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].received_at, ts(5000));
        assert_eq!(signals[0].kind, "aeolus.forecast");
    }

    #[tokio::test]
    async fn conditional_get_304_yields_no_signals_and_sends_stored_validators() {
        let clock = Arc::new(fortuna_core::clock::SimClock::new(ts(0)));
        let t = ScriptedTransport::new(vec![
            Ok(body(200, Some("\"etag-A\""), TMAX)),
            Ok(body(304, None, b"")),
        ]);
        let pin = HostPin::from_url("https://forecasts.aeolus.internal/").unwrap();
        let client = FetchClient::new(t, pin, 60, FetchCaps::default());
        let mut src = AeolusSource::new(
            "aeolus",
            "https://forecasts.aeolus.internal/v2/forecasts?station=KNYC&variable=tmax",
            client,
            clock,
        );
        // First poll: 200, stores the etag, emits one signal.
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
    async fn fetch_maps_malformed_body_to_signal_error() {
        let clock = Arc::new(fortuna_core::clock::SimClock::new(ts(0)));
        let t = ScriptedTransport::new(vec![Ok(body(200, None, b"not json"))]);
        let mut src = source(t, clock);
        let err = src.fetch().await.unwrap_err();
        assert!(matches!(err, SignalError::Fetch { .. }));
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
