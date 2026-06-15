//! C-next-1 — the live PerpTick producer KERNEL.
//!
//! The perp basis-v2 strategy (fortuna-runner) consumes
//! `EventPayload::PerpTick { venue, market, marks, funding }` off the bus but is
//! INERT in a live run: nothing feeds it from live venue data (the slice-4
//! finding mirrored on the live side). This module is the missing producer's
//! KERNEL: it assembles a PerpTick's perp-domain components from two PUBLIC,
//! UNAUTHENTICATED REST reads —
//!
//! - `GET /margin/markets/{ticker}`              → the settlement mark + the BRTI reference, and
//! - `GET /margin/funding_rates/estimate?ticker=` → the funding rate + `next_funding_time`
//!
//! both PUBLIC (no auth, NO 401/403) per
//! `docs/research/venue/kinetics-perps-2026-06-10/` (`perps_openapi.yaml` +
//! `research.md`) — via [`KineticsPerpObservation::from_rest`]. The mapping is
//! field-by-field VERBATIM; the load-bearing guard is that an absent BRTI
//! reference fails closed (the basis-v2 A6 anchor must never be silently empty).
//!
//! ## Transport / no-creds decision (mirrors `funding_poller`, option b)
//!
//! The existing signed transport (`ReqwestKalshiTransport::request`) would force
//! a trading credential — the very thing this producer must NOT load. So, exactly
//! as the funding poller, a minimal host-PINNED UNAUTHENTICATED `reqwest` GET
//! lives here ([`KineticsPublicPerpFetch`]) and REUSES the existing venue DTOs
//! (never redefined). The base host is REUSED from
//! [`crate::funding_poller::KINETICS_PUBLIC_REST_BASE_URL`] (PINNED at
//! construction); the request URL is that base + a const path + the `ticker`
//! ARGUMENT only — NEVER any value from a response payload (the SSRF guard; the
//! track-D SSRF BLOCK is the cautionary tale). No signer, no key, no
//! `FORTUNA_*KALSHI*` var is read anywhere on this path.
//!
//! ## Untrusted data (spec 5.11) + no-panic discipline
//!
//! The two responses are UNTRUSTED. A whole-response fetch/parse failure is a
//! flagged fetch failure (alert-and-continue, nothing emitted, never a panic).
//! A well-fetched-but-malformed payload (e.g. the BRTI reference is missing) is
//! QUARANTINED — an alert reason recorded, no tick emitted — rather than emitting
//! a malformed PerpTick. There is no `unwrap`/`expect`/`panic!` on any path here.
//!
//! ## Kernel only — the async loop + daemon wiring is the NEXT slice
//!
//! This module yields the perp-DOMAIN components `(MarketId, PerpMarks,
//! FundingObservation)`; the "kinetics" `venue` id is added by the daemon at
//! `inject_perp_tick` (mirroring [`crate::perp_feed`]'s note), NOT here. The
//! async poll loop, the daemon `drive()` wire-in, and the `main.rs` spawn are a
//! SEPARATE follow-on slice (the funding-poller Part 3 precedent).

use std::time::Duration;

use async_trait::async_trait;
use fortuna_core::market::MarketId;
use fortuna_core::perp::{FundingObservation, PerpMarks};
use fortuna_venues::kinetics::dto::{FundingEstimateResponse, MarginMarket, MarketResponse};
use fortuna_venues::kinetics::perp_observation::KineticsPerpObservation;
use fortuna_venues::VenueError;

use crate::daemon::DaemonError;

/// The PUBLIC market read path (`/margin/markets/{ticker}`): the settlement mark
/// + the BRTI `reference_price`. PUBLIC per the recorded research (no auth).
const PERP_MARKET_PATH_PREFIX: &str = "/margin/markets/";

/// The PUBLIC funding-estimate read path (`/margin/funding_rates/estimate`): the
/// funding rate + `next_funding_time`. PUBLIC per the recorded research.
const PERP_FUNDING_ESTIMATE_PATH: &str = "/margin/funding_rates/estimate";

/// The per-request HTTP timeout for the public GETs (a slow request must not hang
/// the producer; a timeout classifies as a fetch failure ⇒ alert-and-continue).
pub const PERP_TICK_HTTP_TIMEOUT_SECS: u64 = 20;

/// The fetch seam (creds-less + testable): yields the two PUBLIC payloads a
/// PerpTick is assembled from. Tests inject the recorded fixtures without a
/// network or credentials; the production impl ([`KineticsPublicPerpFetch`])
/// wraps the host-pinned unauthenticated GETs (transport decision b).
#[async_trait]
pub trait PerpTickFetch: Send + Sync {
    /// Fetch the `(market, funding-estimate)` pair for one perp `ticker`.
    async fn fetch_market_and_estimate(
        &self,
        ticker: &str,
    ) -> Result<(MarginMarket, FundingEstimateResponse), VenueError>;
}

/// The production fetch: two minimal, host-PINNED, UNAUTHENTICATED `reqwest` GETs
/// against the PUBLIC market + funding-estimate endpoints (transport decision b).
/// Holds NO signer and NO credential; the base URL is fixed at construction and
/// the request URL is the base + a const path + the `ticker` ARGUMENT only —
/// never a payload value. Reuses the existing venue DTOs for parsing.
pub struct KineticsPublicPerpFetch {
    /// PINNED at construction (the funding poller's prod host, or a recorded host
    /// in a harness), NEVER derived from a payload.
    base_url: String,
    http: reqwest::Client,
}

impl KineticsPublicPerpFetch {
    /// Build the public fetch against `base_url` (PINNED — pass
    /// [`crate::funding_poller::KINETICS_PUBLIC_REST_BASE_URL`] in production, or
    /// a recorded host in a harness). No credential is read or required.
    pub fn new(base_url: &str) -> Result<Self, DaemonError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(PERP_TICK_HTTP_TIMEOUT_SECS))
            .build()
            .map_err(|e| DaemonError::Compose {
                reason: format!("perp_tick_producer: reqwest client construction failed: {e}"),
            })?;
        Ok(KineticsPublicPerpFetch {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        })
    }

    /// The default production fetch: the pinned public host REUSED from the
    /// funding poller (one source of truth for the prod host).
    pub fn production() -> Result<Self, DaemonError> {
        Self::new(crate::funding_poller::KINETICS_PUBLIC_REST_BASE_URL)
    }

    /// One UNAUTHENTICATED GET → parsed `T`. The URL is the PINNED base + the
    /// caller-built `path_and_query` (always const path + the `ticker` arg, never
    /// a payload value — the SSRF guard). No signer, no headers — the endpoints
    /// take no auth. A non-2xx status or a parse failure is `Invalid`; a reqwest
    /// error is classified (timeout vs outage).
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path_and_query: &str,
    ) -> Result<T, VenueError> {
        let url = format!("{}{}", self.base_url, path_and_query);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| classify_fetch_error(path_and_query, &e))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| classify_fetch_error(path_and_query, &e))?;
        if !(200..300).contains(&status) {
            return Err(VenueError::Invalid {
                reason: format!(
                    "perp_tick_producer: public GET {path_and_query} status {status}: {text}"
                ),
            });
        }
        serde_json::from_str::<T>(&text).map_err(|e| VenueError::Invalid {
            reason: format!("perp_tick_producer: GET {path_and_query} did not parse: {e}"),
        })
    }
}

#[async_trait]
impl PerpTickFetch for KineticsPublicPerpFetch {
    async fn fetch_market_and_estimate(
        &self,
        ticker: &str,
    ) -> Result<(MarginMarket, FundingEstimateResponse), VenueError> {
        // Both URLs are PINNED base + const path + the `ticker` ARG only — never a
        // payload value (SSRF guard, mirroring funding_poller's discipline).
        let market_path = format!("{PERP_MARKET_PATH_PREFIX}{ticker}");
        let market = self.get_json::<MarketResponse>(&market_path).await?.market;

        let estimate_path = format!("{PERP_FUNDING_ESTIMATE_PATH}?ticker={ticker}");
        let estimate = self
            .get_json::<FundingEstimateResponse>(&estimate_path)
            .await?;

        Ok((market, estimate))
    }
}

fn classify_fetch_error(path: &str, e: &reqwest::Error) -> VenueError {
    if e.is_timeout() {
        VenueError::Timeout {
            operation: format!("perp_tick_producer: public GET {path} timed out"),
        }
    } else {
        VenueError::Outage {
            venue: "kinetics".to_string(),
            reason: format!("perp_tick_producer: public GET {path}: {e}"),
        }
    }
}

/// One poll's outcome (the perp-domain tick + the alert reasons the caller
/// routes). A fetch failure sets `fetch_failed` + `fetch_alert` with no tick; a
/// well-fetched-but-malformed payload sets `quarantined` + `quarantine_alert`
/// with no tick; a clean poll yields `tick = Some(..)`. The "kinetics" `venue`
/// id is NOT added here — the daemon adds it at `inject_perp_tick` (this kernel
/// yields the perp-domain components, mirroring `perp_feed`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PerpTickPollReport {
    /// True when the market/estimate pair could not be fetched/parsed
    /// (alert-and-continue; no tick). Distinct from `quarantined` — a fetch
    /// failure never reaches the mapping step.
    pub fetch_failed: bool,
    /// The fetch-failure reason (Some iff `fetch_failed`) — a structured alert.
    pub fetch_alert: Option<String>,
    /// The assembled perp-domain tick (Some only on a clean fetch + mapping). The
    /// daemon wraps `(market, marks, funding)` into a `PerpTick` by adding the
    /// kinetics venue id.
    pub tick: Option<(MarketId, PerpMarks, FundingObservation)>,
    /// True when the venue data was fetched but malformed (e.g. the BRTI
    /// reference was absent) and so was QUARANTINED — no tick emitted.
    pub quarantined: bool,
    /// The quarantine reason (Some iff `quarantined`) — a structured alert.
    pub quarantine_alert: Option<String>,
}

/// Poll ONE perp tick: fetch the `(market, estimate)` pair, then assemble the
/// perp-domain components via [`KineticsPerpObservation::from_rest`]. A
/// whole-response fetch/parse failure is a flagged fetch failure
/// (alert-and-continue); a well-fetched-but-malformed payload is QUARANTINED
/// (UNTRUSTED DATA, spec 5.11 — an absent BRTI anchor fails closed here). NEVER
/// panics; emits no tick on either failure path.
pub async fn poll_perp_ticks_once<F: PerpTickFetch + ?Sized>(
    fetch: &F,
    ticker: &str,
) -> PerpTickPollReport {
    // (a) FETCH the pair. A failure is alert-and-continue: a flagged report, no
    // tick, never a panic.
    let (market, estimate) = match fetch.fetch_market_and_estimate(ticker).await {
        Ok(pair) => pair,
        Err(e) => {
            return PerpTickPollReport {
                fetch_failed: true,
                fetch_alert: Some(format!("perp tick poll fetch failed: {e}")),
                ..PerpTickPollReport::default()
            };
        }
    };

    // (b) MAP. A malformed payload (e.g. the A6 BRTI reference absent) is
    // QUARANTINED — counted + alerted, no tick — never a malformed PerpTick.
    match KineticsPerpObservation::from_rest(&market, &estimate) {
        Ok(obs) => PerpTickPollReport {
            tick: Some((obs.market, obs.marks, obs.funding)),
            ..PerpTickPollReport::default()
        },
        Err(e) => PerpTickPollReport {
            quarantined: true,
            quarantine_alert: Some(format!("quarantined perp tick (malformed venue data): {e}")),
            ..PerpTickPollReport::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_public_base_url_is_the_pinned_prod_host() {
        // The producer REUSES the funding poller's pinned host — one source of
        // truth, payload-independent (the SSRF guard).
        assert_eq!(
            crate::funding_poller::KINETICS_PUBLIC_REST_BASE_URL,
            "https://external-api.kalshi.com/trade-api/v2"
        );
    }

    #[test]
    fn the_timeout_const_is_pinned() {
        assert_eq!(PERP_TICK_HTTP_TIMEOUT_SECS, 20);
    }
}
