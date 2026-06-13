//! Shared HTTP fetch substrate (design §4.2 "FetchClient"), mirroring the
//! Kalshi transport pattern: a mockable [`FetchTransport`] trait does the
//! raw network call; the [`FetchClient`] layers politeness, host pinning,
//! conditional GET, and size caps on top. Error classification matches the
//! venue convention — `RateLimited` / `Timeout` / `Outage` — with the
//! caveat that a timeout MAY have reached the origin (reads are idempotent
//! here, so the scheduler simply retries).
//!
//! Two doctrines from the design are enforced structurally, not by habit:
//! - **SSRF posture (§6):** every URL — including each redirect hop — is
//!   validated https-only against the source's pinned host before any
//!   request goes out. A redirect that leaves the pin is refused, never
//!   followed.
//! - **Injected time:** the politeness limiter reads the `Clock`, never wall
//!   time, so rate behavior is deterministic and replayable under DST.

use std::sync::Mutex;

use async_trait::async_trait;
use fortuna_core::clock::{Clock, UtcTimestamp};

use crate::error::SourcesError;

/// Transport-level fetch failures (design §4.2). Distinct from
/// [`SourcesError`] (config) on purpose: these are per-attempt runtime
/// outcomes the scheduler reacts to, not startup validation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FetchError {
    /// Origin returned HTTP 429. Backoff is the scheduler's job (no retry
    /// in the substrate), mirroring the venue transport.
    #[error("rate limited by origin (HTTP 429)")]
    RateLimited,
    /// Request exceeded the timeout. CRITICAL: it may still have reached the
    /// origin — harmless here because every source fetch is an idempotent
    /// GET.
    #[error("request timed out")]
    Timeout,
    /// Connect/DNS/TLS/network failure.
    #[error("origin outage: {reason}")]
    Outage { reason: String },
    /// A non-2xx, non-429 status the substrate does not special-case.
    #[error("unexpected HTTP status {status}")]
    Http { status: u16 },
    /// Response body exceeded the configured size cap; refused (a feed that
    /// suddenly balloons is a containment concern, design §7).
    #[error("response exceeded size cap: {len} > {cap} bytes")]
    TooLarge { len: usize, cap: usize },
    /// The request URL, or a redirect target, failed the host pin (not
    /// https, or a host other than the source's registered host). SSRF
    /// posture, design §6: refused, never followed.
    #[error("url failed host pin: {reason}")]
    OffPin { reason: String },
    /// The per-host politeness budget had no token available at fetch time.
    /// A LOCAL throttle (we declined to send), never an origin signal — the
    /// scheduler defers and tries on a later tick.
    #[error("local politeness budget exhausted for host")]
    BudgetExhausted,
    /// A redirect chain exceeded the hop limit.
    #[error("too many redirects (> {max})")]
    TooManyRedirects { max: usize },
}

/// The host a source is pinned to (design §6). Construction validates the
/// pinned URL itself, so a malformed registry URL fails closed at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPin {
    host: String,
}

impl HostPin {
    /// Pin to the host of `base_url`. The URL must be https with a host.
    pub fn from_url(base_url: &str) -> Result<HostPin, SourcesError> {
        let host = host_of_https(base_url).map_err(|reason| SourcesError::ConfigInvalid {
            source_id: base_url.to_string(),
            reason,
        })?;
        Ok(HostPin { host })
    }

    /// Accept `url` only if it is https AND its host equals the pinned host
    /// (ASCII-case-insensitive). This is the single chokepoint the request
    /// path and every redirect hop both call.
    pub fn admits(&self, url: &str) -> Result<(), FetchError> {
        let host = host_of_https(url).map_err(|reason| FetchError::OffPin { reason })?;
        if host.eq_ignore_ascii_case(&self.host) {
            Ok(())
        } else {
            Err(FetchError::OffPin {
                reason: format!("host `{host}` != pinned `{}`", self.host),
            })
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }
}

/// Extract the host of an https URL, lower-cased. Rejects any other scheme
/// (the SSRF posture is https-only) and any URL without a host. Parsing is
/// deliberately strict and dependency-light: scheme check, then host up to
/// the first `/`, `?`, or `#`, with any `userinfo@` and `:port` stripped.
fn host_of_https(url: &str) -> Result<String, String> {
    const SCHEME: &str = "https://";
    let rest = url
        .strip_prefix(SCHEME)
        .ok_or_else(|| format!("url must be https (SSRF posture): `{url}`"))?;
    // Authority ends at the first path/query/fragment delimiter.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // Drop any userinfo (`user:pass@host`) — keep only what follows the last `@`.
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    // Drop a `:port` suffix. IPv6 literals (`[::1]`) are not a supported
    // source host shape; reject the bracket rather than mis-parse it.
    if host_port.starts_with('[') {
        return Err(format!("bracketed/IPv6 host not supported: `{url}`"));
    }
    let host = host_port.split(':').next().unwrap_or(host_port);
    if host.is_empty() {
        return Err(format!("url has no host: `{url}`"));
    }
    Ok(host.to_ascii_lowercase())
}

/// Conditional-GET validators carried between polls (design §4.2): steady
/// state polling of an unchanged feed costs an empty 304.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conditional {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl Conditional {
    pub fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }
}

/// One raw HTTP response from the transport. Redirects are NOT auto-followed
/// by the transport (the client re-validates each hop), so `location` is
/// surfaced for 3xx.
#[derive(Debug, Clone)]
pub struct RawHttpResponse {
    pub status: u16,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub location: Option<String>,
    pub body: Vec<u8>,
}

/// The successful outcome of a [`FetchClient::fetch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    /// 304: nothing changed since the carried validators.
    NotModified,
    /// 2xx with a body within the size cap; fresh validators for next poll.
    Fetched {
        body: Vec<u8>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

/// One raw HTTP GET with NO automatic redirect following and the
/// conditional-GET headers applied. Network failures are classified into
/// [`FetchError`]. (The real impl wraps reqwest; tests use a scripted mock.)
#[async_trait]
pub trait FetchTransport: Send + Sync {
    async fn get(
        &self,
        url: &str,
        conditional: &Conditional,
    ) -> Result<RawHttpResponse, FetchError>;
}

/// Per-host politeness limiter (design §4.2 "per-host politeness token
/// bucket"). Implemented as GCRA: `budget_per_min` tokens may burst, then
/// requests are spaced at the emission interval; refill is integer-exact in
/// microseconds with no remainder loss. All time comes from the injected
/// `Clock`, so behavior is deterministic under SimClock and DST.
#[derive(Debug)]
pub struct PoliteLimiter {
    /// microseconds per token (= 60_000_000 / budget).
    interval_micros: i64,
    /// burst tolerance in microseconds (= (budget - 1) * interval).
    tau_micros: i64,
    /// theoretical arrival time; None until the first acquire.
    tat_micros: Mutex<Option<i64>>,
}

impl PoliteLimiter {
    /// `budget_per_min` must be > 0 (config validation guarantees this).
    pub fn new(budget_per_min: u32) -> PoliteLimiter {
        let budget = budget_per_min.max(1) as i64;
        let interval_micros = 60_000_000 / budget;
        PoliteLimiter {
            interval_micros,
            tau_micros: (budget - 1) * interval_micros,
            tat_micros: Mutex::new(None),
        }
    }

    /// Try to consume one token as of `now`. Returns true (allowed) or false
    /// (throttled — caller defers). Monotonic in intent: a `now` earlier than
    /// the last is treated as no-progress and simply throttles, never grants
    /// extra budget.
    pub fn try_acquire(&self, now: UtcTimestamp) -> bool {
        let now_micros = now.epoch_millis().saturating_mul(1000);
        let mut guard = self
            .tat_micros
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match *guard {
            None => {
                *guard = Some(now_micros + self.interval_micros);
                true
            }
            Some(tat) => {
                if now_micros >= tat - self.tau_micros {
                    let base = now_micros.max(tat);
                    *guard = Some(base + self.interval_micros);
                    true
                } else {
                    false
                }
            }
        }
    }
}

/// Caps the [`FetchClient`] applies to every fetch.
#[derive(Debug, Clone)]
pub struct FetchCaps {
    /// Reject bodies larger than this (design §4.2, and §7 containment).
    pub max_body_bytes: usize,
    /// Maximum redirect hops to follow (each re-validated against the pin).
    pub max_redirects: usize,
}

impl Default for FetchCaps {
    fn default() -> FetchCaps {
        FetchCaps {
            max_body_bytes: 8 * 1024 * 1024,
            max_redirects: 3,
        }
    }
}

/// The shared fetch substrate. Owns one transport, one host pin, one
/// politeness limiter, and the caps. Adapters call `fetch`; nothing else
/// here decides what to fetch.
pub struct FetchClient<T: FetchTransport> {
    transport: T,
    pin: HostPin,
    limiter: PoliteLimiter,
    caps: FetchCaps,
}

impl<T: FetchTransport> FetchClient<T> {
    pub fn new(transport: T, pin: HostPin, budget_per_min: u32, caps: FetchCaps) -> FetchClient<T> {
        FetchClient {
            transport,
            pin,
            limiter: PoliteLimiter::new(budget_per_min),
            caps,
        }
    }

    /// Fetch `url` carrying conditional-GET validators, reading `now` from the
    /// injected clock for the politeness budget. Validates the pin up front
    /// and on every redirect hop; consumes one politeness token per network
    /// call; enforces the size cap.
    pub async fn fetch(
        &self,
        url: &str,
        conditional: &Conditional,
        clock: &dyn Clock,
    ) -> Result<FetchOutcome, FetchError> {
        // Validate the initial URL against the pin BEFORE spending a token or
        // touching the network.
        self.pin.admits(url)?;

        let mut current = url.to_string();
        let mut hops = 0usize;
        loop {
            if !self.limiter.try_acquire(clock.now()) {
                return Err(FetchError::BudgetExhausted);
            }
            let resp = self.transport.get(&current, conditional).await?;
            match resp.status {
                304 => return Ok(FetchOutcome::NotModified),
                200..=299 => {
                    if resp.body.len() > self.caps.max_body_bytes {
                        return Err(FetchError::TooLarge {
                            len: resp.body.len(),
                            cap: self.caps.max_body_bytes,
                        });
                    }
                    return Ok(FetchOutcome::Fetched {
                        body: resp.body,
                        etag: resp.etag,
                        last_modified: resp.last_modified,
                    });
                }
                300..=399 => {
                    let location = resp.location.ok_or(FetchError::Http {
                        status: resp.status,
                    })?;
                    // Re-validate the hop against the pin BEFORE following it.
                    self.pin.admits(&location)?;
                    hops += 1;
                    if hops > self.caps.max_redirects {
                        return Err(FetchError::TooManyRedirects {
                            max: self.caps.max_redirects,
                        });
                    }
                    current = location;
                }
                429 => return Err(FetchError::RateLimited),
                other => return Err(FetchError::Http { status: other }),
            }
        }
    }

    pub fn pin(&self) -> &HostPin {
        &self.pin
    }
}

/// The real transport: one reqwest client with auto-redirect DISABLED (the
/// [`FetchClient`] re-validates every hop against the pin) and a request
/// timeout so the `Timeout` classification can fire. Thin by design — it
/// signs nothing and decides nothing; it is exercised through fixtures and
/// integration, not unit tests (mirrors `ReqwestKalshiTransport`).
pub struct ReqwestFetchTransport {
    http: reqwest::Client,
    user_agent: String,
}

impl ReqwestFetchTransport {
    pub fn new(timeout: std::time::Duration, user_agent: &str) -> Result<Self, SourcesError> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| SourcesError::ConfigInvalid {
                source_id: "<fetch-transport>".into(),
                reason: format!("reqwest client construction failed: {e}"),
            })?;
        Ok(ReqwestFetchTransport {
            http,
            user_agent: user_agent.to_string(),
        })
    }

    fn header(resp: &reqwest::Response, name: reqwest::header::HeaderName) -> Option<String> {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }
}

#[async_trait]
impl FetchTransport for ReqwestFetchTransport {
    async fn get(
        &self,
        url: &str,
        conditional: &Conditional,
    ) -> Result<RawHttpResponse, FetchError> {
        use reqwest::header;
        let mut req = self
            .http
            .get(url)
            .header(header::USER_AGENT, &self.user_agent);
        if let Some(etag) = &conditional.etag {
            req = req.header(header::IF_NONE_MATCH, etag);
        }
        if let Some(lm) = &conditional.last_modified {
            req = req.header(header::IF_MODIFIED_SINCE, lm);
        }
        let resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                FetchError::Timeout
            } else {
                FetchError::Outage {
                    reason: e.to_string(),
                }
            }
        })?;
        let status = resp.status().as_u16();
        let etag = Self::header(&resp, header::ETAG);
        let last_modified = Self::header(&resp, header::LAST_MODIFIED);
        let location = Self::header(&resp, header::LOCATION);
        let body = resp
            .bytes()
            .await
            .map_err(|e| FetchError::Outage {
                reason: format!("reading body: {e}"),
            })?
            .to_vec();
        Ok(RawHttpResponse {
            status,
            etag,
            last_modified,
            location,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fortuna_core::clock::SimClock;
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;

    fn ts(ms: i64) -> UtcTimestamp {
        UtcTimestamp::from_epoch_millis(ms).unwrap()
    }

    // --- host pinning / SSRF posture (design §6) -------------------------

    #[test]
    fn pin_admits_same_host_https_only() {
        let pin = HostPin::from_url("https://api.weather.gov/products").unwrap();
        assert_eq!(pin.host(), "api.weather.gov");
        assert!(pin.admits("https://api.weather.gov/anything?x=1").is_ok());
        // Case-insensitive host.
        assert!(pin.admits("https://API.Weather.GOV/x").is_ok());
    }

    #[test]
    fn pin_refuses_http_other_host_and_userinfo_tricks() {
        let pin = HostPin::from_url("https://api.weather.gov/").unwrap();
        // Plain http.
        assert!(matches!(
            pin.admits("http://api.weather.gov/x"),
            Err(FetchError::OffPin { .. })
        ));
        // Different host.
        assert!(matches!(
            pin.admits("https://evil.example.com/x"),
            Err(FetchError::OffPin { .. })
        ));
        // userinfo@ smuggling the real host into the authority: the host is
        // what follows the last '@', so this is evil.example.com — refused.
        assert!(matches!(
            pin.admits("https://api.weather.gov@evil.example.com/x"),
            Err(FetchError::OffPin { .. })
        ));
        // port does not change the host identity.
        assert!(pin.admits("https://api.weather.gov:443/x").is_ok());
    }

    #[test]
    fn pin_construction_rejects_non_https() {
        assert!(HostPin::from_url("http://x.test/").is_err());
        assert!(HostPin::from_url("ftp://x.test/").is_err());
        assert!(HostPin::from_url("https:///nopath").is_err());
    }

    // --- politeness limiter (GCRA) --------------------------------------

    #[test]
    fn limiter_bursts_then_throttles_at_the_same_instant() {
        // budget 6/min => 6 may burst at t0, the 7th is throttled.
        let lim = PoliteLimiter::new(6);
        let t0 = ts(1_000_000);
        for i in 0..6 {
            assert!(lim.try_acquire(t0), "burst token {i} should be granted");
        }
        assert!(
            !lim.try_acquire(t0),
            "7th at the same instant must throttle"
        );
    }

    #[test]
    fn limiter_refills_at_the_emission_interval() {
        // budget 6/min => one token every 10_000 ms.
        let lim = PoliteLimiter::new(6);
        let t0 = ts(0);
        for _ in 0..6 {
            assert!(lim.try_acquire(t0));
        }
        assert!(
            !lim.try_acquire(ts(9_999)),
            "just before the interval: throttled"
        );
        assert!(
            lim.try_acquire(ts(10_000)),
            "at the interval: one token back"
        );
        assert!(
            !lim.try_acquire(ts(10_001)),
            "and immediately throttled again"
        );
    }

    #[test]
    fn limiter_budget_one_is_strict_spacing() {
        let lim = PoliteLimiter::new(1);
        assert!(lim.try_acquire(ts(0)));
        assert!(!lim.try_acquire(ts(59_999)));
        assert!(lim.try_acquire(ts(60_000)));
    }

    #[test]
    fn limiter_clock_going_backwards_never_grants_extra() {
        let lim = PoliteLimiter::new(2);
        assert!(lim.try_acquire(ts(100_000)));
        assert!(lim.try_acquire(ts(100_000))); // burst of 2
        assert!(!lim.try_acquire(ts(100_000)));
        // A backwards clock must not refill.
        assert!(!lim.try_acquire(ts(0)));
    }

    // --- FetchClient orchestration --------------------------------------

    struct MockTransport {
        scripted: StdMutex<VecDeque<Result<RawHttpResponse, FetchError>>>,
        seen: StdMutex<Vec<String>>,
    }

    impl MockTransport {
        fn new(responses: Vec<Result<RawHttpResponse, FetchError>>) -> MockTransport {
            MockTransport {
                scripted: StdMutex::new(responses.into()),
                seen: StdMutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl FetchTransport for MockTransport {
        async fn get(
            &self,
            url: &str,
            _conditional: &Conditional,
        ) -> Result<RawHttpResponse, FetchError> {
            self.seen.lock().unwrap().push(url.to_string());
            self.scripted
                .lock()
                .unwrap()
                .pop_front()
                .expect("unscripted transport call")
        }
    }

    fn ok_body(status: u16, body: &[u8]) -> RawHttpResponse {
        RawHttpResponse {
            status,
            etag: Some("\"v1\"".into()),
            last_modified: None,
            location: None,
            body: body.to_vec(),
        }
    }

    fn client<T: FetchTransport>(t: T, budget: u32, caps: FetchCaps) -> FetchClient<T> {
        let pin = HostPin::from_url("https://api.weather.gov/").unwrap();
        FetchClient::new(t, pin, budget, caps)
    }

    #[tokio::test]
    async fn fetch_returns_body_on_200_within_cap() {
        let t = MockTransport::new(vec![Ok(ok_body(200, b"hello"))]);
        let c = client(t, 10, FetchCaps::default());
        let clock = SimClock::new(ts(0));
        let out = c
            .fetch("https://api.weather.gov/x", &Conditional::default(), &clock)
            .await
            .unwrap();
        assert_eq!(
            out,
            FetchOutcome::Fetched {
                body: b"hello".to_vec(),
                etag: Some("\"v1\"".into()),
                last_modified: None,
            }
        );
    }

    #[tokio::test]
    async fn fetch_maps_304_to_not_modified() {
        let t = MockTransport::new(vec![Ok(ok_body(304, b""))]);
        let c = client(t, 10, FetchCaps::default());
        let clock = SimClock::new(ts(0));
        let out = c
            .fetch(
                "https://api.weather.gov/x",
                &Conditional {
                    etag: Some("\"v1\"".into()),
                    last_modified: None,
                },
                &clock,
            )
            .await
            .unwrap();
        assert_eq!(out, FetchOutcome::NotModified);
    }

    #[tokio::test]
    async fn fetch_enforces_size_cap() {
        let t = MockTransport::new(vec![Ok(ok_body(200, b"toolong"))]);
        let caps = FetchCaps {
            max_body_bytes: 3,
            max_redirects: 3,
        };
        let c = client(t, 10, caps);
        let clock = SimClock::new(ts(0));
        let err = c
            .fetch("https://api.weather.gov/x", &Conditional::default(), &clock)
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::TooLarge { len: 7, cap: 3 });
    }

    #[tokio::test]
    async fn fetch_refuses_url_off_pin_before_touching_network() {
        let t = MockTransport::new(vec![]); // no scripted call => panics if reached
        let c = client(t, 10, FetchCaps::default());
        let clock = SimClock::new(ts(0));
        let err = c
            .fetch(
                "https://evil.example.com/x",
                &Conditional::default(),
                &clock,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, FetchError::OffPin { .. }));
    }

    #[tokio::test]
    async fn fetch_follows_on_pin_redirect_but_refuses_off_pin_redirect() {
        // On-pin redirect is followed.
        let on = MockTransport::new(vec![
            Ok(RawHttpResponse {
                status: 301,
                etag: None,
                last_modified: None,
                location: Some("https://api.weather.gov/moved".into()),
                body: vec![],
            }),
            Ok(ok_body(200, b"final")),
        ]);
        let c = client(on, 10, FetchCaps::default());
        let clock = SimClock::new(ts(0));
        let out = c
            .fetch("https://api.weather.gov/x", &Conditional::default(), &clock)
            .await
            .unwrap();
        assert!(matches!(out, FetchOutcome::Fetched { .. }));

        // Off-pin redirect is refused, not followed.
        let off = MockTransport::new(vec![Ok(RawHttpResponse {
            status: 302,
            etag: None,
            last_modified: None,
            location: Some("https://evil.example.com/x".into()),
            body: vec![],
        })]);
        let c = client(off, 10, FetchCaps::default());
        let err = c
            .fetch("https://api.weather.gov/x", &Conditional::default(), &clock)
            .await
            .unwrap_err();
        assert!(matches!(err, FetchError::OffPin { .. }));
    }

    #[tokio::test]
    async fn fetch_caps_redirect_hops() {
        // A redirect loop within the pin is bounded by max_redirects.
        let responses: Vec<_> = (0..10)
            .map(|_| {
                Ok(RawHttpResponse {
                    status: 301,
                    etag: None,
                    last_modified: None,
                    location: Some("https://api.weather.gov/loop".into()),
                    body: vec![],
                })
            })
            .collect();
        let t = MockTransport::new(responses);
        let caps = FetchCaps {
            max_body_bytes: 1024,
            max_redirects: 2,
        };
        let c = client(t, 10, caps);
        let clock = SimClock::new(ts(0));
        let err = c
            .fetch("https://api.weather.gov/x", &Conditional::default(), &clock)
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::TooManyRedirects { max: 2 });
    }

    #[tokio::test]
    async fn fetch_propagates_429_as_rate_limited() {
        let t = MockTransport::new(vec![Ok(ok_body(429, b""))]);
        let c = client(t, 10, FetchCaps::default());
        let clock = SimClock::new(ts(0));
        let err = c
            .fetch("https://api.weather.gov/x", &Conditional::default(), &clock)
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::RateLimited);
    }

    #[tokio::test]
    async fn fetch_throttles_locally_when_budget_exhausted() {
        // budget 1/min: first fetch spends the only token; the second at the
        // same instant is refused LOCALLY without a transport call.
        let t = MockTransport::new(vec![Ok(ok_body(200, b"a"))]);
        let c = client(t, 1, FetchCaps::default());
        let clock = SimClock::new(ts(0));
        let cond = Conditional::default();
        assert!(c
            .fetch("https://api.weather.gov/x", &cond, &clock)
            .await
            .is_ok());
        let err = c
            .fetch("https://api.weather.gov/x", &cond, &clock)
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::BudgetExhausted);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use fortuna_core::clock::UtcTimestamp;
    use proptest::prelude::*;

    fn ts(ms: i64) -> UtcTimestamp {
        UtcTimestamp::from_epoch_millis(ms).unwrap()
    }

    proptest! {
        /// The politeness invariant (design §4.2): over ANY non-decreasing
        /// sequence of attempt times, the number granted never exceeds the
        /// token-bucket bound — burst capacity plus what refilled across the
        /// span. This is the "never exceeds budget" property: a source can
        /// never be made to hammer a host faster than configured.
        #[test]
        fn limiter_never_exceeds_the_refill_bound(
            budget in 1u32..120,
            // Non-negative inter-arrival gaps in ms; arbitrary count/spacing.
            gaps in proptest::collection::vec(0i64..120_000, 0..300),
        ) {
            let lim = PoliteLimiter::new(budget);
            let mut now_ms = 0i64;
            let mut granted = 0i64;
            let mut first: Option<i64> = None;
            let mut last = 0i64;
            for g in &gaps {
                now_ms += g;
                if first.is_none() { first = Some(now_ms); }
                last = now_ms;
                if lim.try_acquire(ts(now_ms)) {
                    granted += 1;
                }
            }
            let span_ms = last - first.unwrap_or(0);
            // Token bucket bound: capacity(=budget) + refilled over the span.
            // +1 absorbs the integer interval boundary.
            let bound = budget as i64 + (span_ms * budget as i64) / 60_000 + 1;
            prop_assert!(
                granted <= bound,
                "granted {granted} exceeded bound {bound} (budget {budget}, span {span_ms}ms)"
            );
        }

        /// Whatever else happens, the limiter grants at least one token (the
        /// first attempt always succeeds) and never panics.
        #[test]
        fn limiter_first_attempt_always_granted(budget in 1u32..120, start in 0i64..1_000_000_000) {
            let lim = PoliteLimiter::new(budget);
            prop_assert!(lim.try_acquire(ts(start)));
        }
    }
}
