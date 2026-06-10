//! Transport layer: every Kalshi REST call goes through `KalshiTransport`.
//!
//! `ReqwestKalshiTransport` is the real HTTP client: it signs EVERY request
//! with `KalshiSigner` (timestamp from the injected `Clock`, never wall
//! time), and classifies transport-level failures:
//!
//! - HTTP 429 -> `VenueError::RateLimited` (body shape per research §12);
//! - network/connect errors -> `VenueError::Outage`;
//! - timeouts -> `VenueError::Timeout` — CRITICAL: per the docs a timed-out
//!   request MAY HAVE EXECUTED at the venue; callers must reconcile.
//!
//! No retries live here: retry/backoff policy belongs to the caller.
//!
//! `MockKalshiTransport` is the scripted transport used by the adapter test
//! suite (doc-derived samples today, operator-recorded fixtures later).

use crate::kalshi::auth::KalshiSigner;
use crate::VenueError;
use async_trait::async_trait;
use fortuna_core::clock::Clock;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Production REST root (research §2, doc-verbatim; the `external-api` hosts
/// are "the recommended hosts for API traders". Also supported:
/// `https://api.elections.kalshi.com/trade-api/v2`).
pub const KALSHI_PROD_BASE_URL: &str = "https://external-api.kalshi.com/trade-api/v2";
/// Demo REST root (also supported: `https://demo-api.kalshi.co/trade-api/v2`).
/// Demo credentials do not work on prod and vice versa.
pub const KALSHI_DEMO_BASE_URL: &str = "https://external-api.demo.kalshi.co/trade-api/v2";

/// One REST request. `path` is venue-relative (e.g. `/portfolio/balance`);
/// the transport owns the base URL and the `/trade-api/v2` signing prefix.
/// Returns the HTTP status and the parsed JSON body (Null when empty,
/// String when not JSON); 429/network/timeout are returned as errors.
#[async_trait]
pub trait KalshiTransport: Send + Sync {
    async fn request(
        &self,
        method: &str,
        path: &str,
        query: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> Result<(u16, serde_json::Value), VenueError>;
}

/// The real transport. Signing path = base-URL path + request path with the
/// query stripped, exactly as the docs' own example computes it
/// (`urlparse(base_url + path).path`).
pub struct ReqwestKalshiTransport {
    base_url: String,
    base_path: String,
    signer: KalshiSigner,
    clock: Arc<dyn Clock>,
    http: reqwest::Client,
}

impl ReqwestKalshiTransport {
    /// `base_url` comes from config (`KALSHI_PROD_BASE_URL` /
    /// `KALSHI_DEMO_BASE_URL`); `timeout` bounds every request so the
    /// Timeout classification can actually fire.
    pub fn new(
        base_url: &str,
        signer: KalshiSigner,
        clock: Arc<dyn Clock>,
        timeout: Duration,
    ) -> Result<Self, VenueError> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let parsed = reqwest::Url::parse(&base_url).map_err(|e| VenueError::Invalid {
            reason: format!("kalshi base url {base_url:?} did not parse: {e}"),
        })?;
        let base_path = parsed.path().trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| VenueError::Invalid {
                reason: format!("reqwest client construction failed: {e}"),
            })?;
        Ok(ReqwestKalshiTransport {
            base_url,
            base_path,
            signer,
            clock,
            http,
        })
    }
}

/// `{base}{path}` plus optional `?query`.
pub(crate) fn join_url(base_url: &str, path: &str, query: Option<&str>) -> String {
    match query {
        Some(q) if !q.is_empty() => format!("{base_url}{path}?{q}"),
        _ => format!("{base_url}{path}"),
    }
}

#[async_trait]
impl KalshiTransport for ReqwestKalshiTransport {
    async fn request(
        &self,
        method: &str,
        path: &str,
        query: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> Result<(u16, serde_json::Value), VenueError> {
        let url = join_url(&self.base_url, path, query);
        let sign_path = format!("{}{path}", self.base_path);
        let timestamp_ms = self.clock.now().epoch_millis();
        let headers = self.signer.sign(method, &sign_path, timestamp_ms)?;

        let http_method = reqwest::Method::from_bytes(method.to_ascii_uppercase().as_bytes())
            .map_err(|e| VenueError::Invalid {
                reason: format!("invalid HTTP method {method:?}: {e}"),
            })?;
        let mut req = self.http.request(http_method, &url);
        for (name, value) in headers.as_header_pairs() {
            req = req.header(name, value);
        }
        if let Some(json) = &body {
            req = req.json(json);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| classify_send_error(method, path, &e))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| classify_send_error(method, path, &e))?;

        if status == 429 {
            // Body is `{"error": "too many requests"}` (research §12); no
            // Retry-After header is sent. Backoff policy is the caller's.
            return Err(VenueError::RateLimited);
        }

        let json = if text.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text))
        };
        Ok((status, json))
    }
}

fn classify_send_error(method: &str, path: &str, e: &reqwest::Error) -> VenueError {
    if e.is_timeout() {
        // Doc warning carried into the error contract: the request may have
        // executed at the venue even though we never saw the response.
        VenueError::Timeout {
            operation: format!(
                "{method} {path} (request timed out; it MAY HAVE EXECUTED at the venue — \
                 reconcile orders/fills before assuming either outcome)"
            ),
        }
    } else {
        VenueError::Outage {
            venue: "kalshi".to_string(),
            reason: format!("{method} {path}: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Scripted transport for tests / fixture replay
// ---------------------------------------------------------------------------

/// One request as the adapter issued it (assertable in tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedCall {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub body: Option<serde_json::Value>,
}

/// FIFO-scripted transport. Tests (and, later, fixture replays) queue
/// `(status, body)` responses or errors; every call is recorded for
/// assertion. An unscripted call is an error, never a guess.
#[derive(Default)]
pub struct MockKalshiTransport {
    script: Mutex<VecDeque<Result<(u16, serde_json::Value), VenueError>>>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl MockKalshiTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a `(status, body)` response.
    pub fn push_ok(&self, status: u16, body: serde_json::Value) {
        self.script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(Ok((status, body)));
    }

    /// Queue a transport-level error (RateLimited / Outage / Timeout / ...).
    pub fn push_err(&self, err: VenueError) {
        self.script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(Err(err));
    }

    /// Everything the adapter asked for, in order.
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl KalshiTransport for MockKalshiTransport {
    async fn request(
        &self,
        method: &str,
        path: &str,
        query: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> Result<(u16, serde_json::Value), VenueError> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(RecordedCall {
                method: method.to_string(),
                path: path.to_string(),
                query: query.map(str::to_string),
                body,
            });
        self.script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| {
                Err(VenueError::Invalid {
                    reason: format!("unscripted transport call: {method} {path}"),
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_url_appends_query_only_when_present() {
        assert_eq!(
            join_url("https://x/trade-api/v2", "/markets", Some("limit=5")),
            "https://x/trade-api/v2/markets?limit=5"
        );
        assert_eq!(
            join_url("https://x/trade-api/v2", "/markets", None),
            "https://x/trade-api/v2/markets"
        );
        assert_eq!(
            join_url("https://x/trade-api/v2", "/markets", Some("")),
            "https://x/trade-api/v2/markets"
        );
    }

    #[test]
    fn base_urls_match_the_research_doc() {
        assert_eq!(
            KALSHI_PROD_BASE_URL,
            "https://external-api.kalshi.com/trade-api/v2"
        );
        assert_eq!(
            KALSHI_DEMO_BASE_URL,
            "https://external-api.demo.kalshi.co/trade-api/v2"
        );
    }
}
