//! A2d SLICE 3 part 2 — the funding-rates POLLER that FILLS
//! `funding_rates_historical` (design docs/design/perp-strategies-and-scalar-
//! claims.md §9.1). Parts 1 (the store) and 3 (the resolve/score loop) merged;
//! today NOTHING writes the store outside tests, so production is empty. This is
//! the missing producer: it reads the PUBLIC, unauthenticated `GET
//! /margin/funding_rates/historical` (perps_openapi.yaml line 887 — its only
//! responses are 200/400/500: NO 401/403, the endpoint takes NO auth) and
//! idempotently inserts each finalized {market, funding_time, funding_rate,
//! mark_price} via [`FundingRatesHistoricalRepo`].
//!
//! ## Transport / no-creds decision (option **b**)
//!
//! The endpoint is PUBLIC, but the existing `ReqwestKalshiTransport::request`
//! SIGNS every call (`KalshiSigner::sign` over a parsed RSA private key) — there
//! is no unsigned path, so reusing `KineticsClient::funding_rates_historical`
//! would force a trading credential (the very thing this poller must NOT load).
//! So we take **option (b)**: a minimal, host-PINNED, UNAUTHENTICATED `reqwest`
//! GET lives here ([`KineticsPublicFetch`]) and REUSES the existing venue DTO
//! ([`FundingRatesHistoricalResponse`], never redefined). The base host is PINNED
//! at construction (a const or injected config), NEVER derived from a payload
//! (the track-D SSRF BLOCK is the cautionary tale). No signer, no key, no
//! `FORTUNA_*KALSHI*` var is read anywhere on this path.
//!
//! ## Untrusted data (spec 5.11) + no-panic discipline
//!
//! The response is UNTRUSTED. Each entry's shape is validated (finite
//! funding_rate, non-empty market_ticker, parseable funding_time); a
//! non-conforming entry is QUARANTINED — counted, a structured alert reason
//! recorded for the caller to route — and SKIPPED, while well-formed siblings
//! still insert. A whole-response fetch/parse failure is reported as a flagged
//! fetch failure (alert-and-continue), never a panic. There is no `unwrap`/
//! `expect`/`panic!` on any path here. `mark_price` stays the venue's VERBATIM
//! string (no f64 round-trip, no `Cents`); `funding_rate` is forecast-domain f64.
//!
//! ## Clock-injected, idempotent, append-only
//!
//! [`run_funding_poller`] is Clock-driven: it BACKFILLS once, then polls past each
//! 8h boundary (04:00/12:00/20:00 UTC) computed by [`next_funding_poll_at`], with
//! the wait derived from the injected `&dyn Clock` and cancellable — no raw
//! wall-time sleep keyed to a deadline (mirrors the daemon's poll-and-tick shape;
//! the live `drive()` wire-in is the additive follow-on, Part 3 precedent).
//! `captured_at` is `clock.now()` (the POLL time, not the venue funding_time).
//! `insert`'s `ON CONFLICT DO NOTHING` makes a re-poll a no-op (`Ok(false)`), so
//! the poller is safe to re-run.

use std::time::Duration;

use async_trait::async_trait;
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_ledger::FundingRatesHistoricalRepo;
use fortuna_venues::kinetics::dto::FundingRatesHistoricalResponse;
use fortuna_venues::VenueError;

use crate::daemon::DaemonError;

/// The PUBLIC perps REST root (perps_openapi.yaml server block, doc-verbatim:
/// "Production perps REST API server"). The funding-historical path hangs off
/// this. PINNED here at construction — NEVER taken from a response payload.
pub const KINETICS_PUBLIC_REST_BASE_URL: &str = "https://external-api.kalshi.com/trade-api/v2";

/// The PUBLIC funding-historical path (perps_openapi.yaml line 887).
const FUNDING_HISTORICAL_PATH: &str = "/margin/funding_rates/historical";

/// How many recent entries to request (ticker=None ⇒ across-all-markets). The
/// recorded fixture used the `limit` param (NOT start_ts/end_ts); a sufficient
/// limit + the idempotent insert make backfill + incremental polling correct
/// without start_ts (the openapi's start_ts/end_ts is an unused efficiency
/// refinement — ledgered in GAPS). 11 markets × ~8h cadence, so 500 covers a
/// generous multi-day window per poll.
const FUNDING_HISTORICAL_LIMIT: i64 = 500;

/// The per-request HTTP timeout for the public GET (a slow request must not hang
/// the loop; a timeout classifies as a fetch failure ⇒ alert-and-continue).
pub const FUNDING_POLL_HTTP_TIMEOUT_SECS: u64 = 20;

/// The loop's clock-polling granularity: `run_funding_poller` re-reads the
/// injected clock this often while waiting for the next boundary, racing the
/// cancel signal. Bounds how long after a boundary (or a cancel) the loop reacts;
/// it is NOT a wall-time sleep keyed to the boundary deadline (the injected clock
/// is the timing authority — a SimClock loop reacts as fast as it is advanced).
const POLL_TICK_MS: u64 = 1_000;

/// The fetch seam (creds-less + testable): yields the PUBLIC funding-historical
/// response. Tests inject the recorded fixture without a network or credentials;
/// the production impl ([`KineticsPublicFetch`]) wraps the host-pinned
/// unauthenticated GET (transport decision b).
#[async_trait]
pub trait FundingHistFetch: Send + Sync {
    /// Fetch the funding history across ALL markets (ticker=None) — the recorded
    /// `(ticker=None, limit)` request shape.
    async fn fetch_all(&self) -> Result<FundingRatesHistoricalResponse, VenueError>;
}

/// The production fetch: a minimal, host-PINNED, UNAUTHENTICATED `reqwest` GET
/// against the PUBLIC funding-historical endpoint (transport decision b). Holds
/// NO signer and NO credential; the base URL is fixed at construction and the
/// query carries only `limit` (ticker omitted ⇒ all markets), exactly the
/// recorded request shape. Reuses the existing venue DTO for parsing.
pub struct KineticsPublicFetch {
    /// PINNED at construction (a const default or operator-injected config),
    /// NEVER derived from a payload.
    base_url: String,
    limit: i64,
    http: reqwest::Client,
}

impl KineticsPublicFetch {
    /// Build the public fetch against `base_url` (PINNED — pass
    /// [`KINETICS_PUBLIC_REST_BASE_URL`] in production, or a recorded host in a
    /// harness). No credential is read or required.
    pub fn new(base_url: &str, limit: i64) -> Result<Self, DaemonError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(FUNDING_POLL_HTTP_TIMEOUT_SECS))
            .build()
            .map_err(|e| DaemonError::Compose {
                reason: format!("funding_poller: reqwest client construction failed: {e}"),
            })?;
        Ok(KineticsPublicFetch {
            base_url: base_url.trim_end_matches('/').to_string(),
            limit,
            http,
        })
    }

    /// The default production fetch: the pinned public host + the recorded limit.
    pub fn production() -> Result<Self, DaemonError> {
        Self::new(KINETICS_PUBLIC_REST_BASE_URL, FUNDING_HISTORICAL_LIMIT)
    }
}

#[async_trait]
impl FundingHistFetch for KineticsPublicFetch {
    async fn fetch_all(&self) -> Result<FundingRatesHistoricalResponse, VenueError> {
        // ticker omitted ⇒ all markets (perps_openapi.yaml: "Leave empty to query
        // across all markets"); only `limit` is sent, matching the recorded shape.
        let url = format!(
            "{}{}?limit={}",
            self.base_url, FUNDING_HISTORICAL_PATH, self.limit
        );
        // UNAUTHENTICATED: no signer, no headers — the endpoint takes no auth.
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| classify_fetch_error(&e))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| classify_fetch_error(&e))?;
        if !(200..300).contains(&status) {
            return Err(VenueError::Invalid {
                reason: format!(
                    "funding_poller: public GET {FUNDING_HISTORICAL_PATH} status {status}: {text}"
                ),
            });
        }
        serde_json::from_str::<FundingRatesHistoricalResponse>(&text).map_err(|e| {
            VenueError::Invalid {
                reason: format!("funding_poller: response did not parse as funding history: {e}"),
            }
        })
    }
}

fn classify_fetch_error(e: &reqwest::Error) -> VenueError {
    if e.is_timeout() {
        VenueError::Timeout {
            operation: format!("funding_poller: public GET {FUNDING_HISTORICAL_PATH} timed out"),
        }
    } else {
        VenueError::Outage {
            venue: "kinetics".to_string(),
            reason: format!("funding_poller: public GET {FUNDING_HISTORICAL_PATH}: {e}"),
        }
    }
}

/// One poll's outcome (counts + the alert reasons the caller routes to Slack).
/// A fetch failure is `fetch_failed=true` with `fetch_alert` set and the counts
/// zero; otherwise `fetched = inserted + skipped_dup + quarantined`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FundingPollReport {
    /// True when the WHOLE response could not be fetched/parsed (alert-and-
    /// continue; nothing was inserted). Mutually distinct from per-entry
    /// quarantine — a fetch failure never reaches the per-entry loop.
    pub fetch_failed: bool,
    /// The fetch-failure reason (Some iff `fetch_failed`) — a structured alert
    /// for the caller to route.
    pub fetch_alert: Option<String>,
    /// Entries received from the venue (0 on a fetch failure).
    pub fetched: usize,
    /// Newly inserted rows (`insert` returned `Ok(true)`).
    pub inserted: usize,
    /// Entries already recorded (`insert` returned `Ok(false)` — idempotent
    /// re-poll, `ON CONFLICT DO NOTHING`).
    pub skipped_dup: usize,
    /// Entries QUARANTINED for a shape violation (non-finite rate, empty ticker,
    /// unparseable funding_time) — counted + alerted, never inserted.
    pub quarantined: usize,
    /// One structured alert reason per quarantined entry (the caller routes these
    /// to Slack; counted == `quarantined`).
    pub quarantine_alerts: Vec<String>,
}

/// The next 8h funding boundary (04:00/12:00/20:00 UTC) STRICTLY AFTER `now`.
///
/// PURE + Clock-free (operates on the passed `UtcTimestamp`, no `SystemTime`).
/// "Strictly after" means a poll sitting EXACTLY on a boundary returns the NEXT
/// one (so the loop fires once per boundary, never spins on the same instant);
/// from 20:00 it rolls over to the next day's 04:00 (and across month/year ends,
/// since the arithmetic is in epoch-millis). Implementation: floor `now` to the
/// UTC day, then advance through the day's three boundaries (and the next day's
/// first) until one is strictly greater — at most four `checked_add_millis`
/// steps, each on the day-floor base, so no accumulation drift. On the
/// (practically impossible) arithmetic overflow at the i64 epoch-millis
/// extremes, it returns `now` unchanged rather than panicking (the loop then
/// polls immediately — a benign, non-crashing degrade).
pub fn next_funding_poll_at(now: UtcTimestamp) -> UtcTimestamp {
    // Day-floor: epoch-millis floored to a multiple of 86_400_000 (UTC midnight).
    const DAY_MS: i64 = 24 * 60 * 60 * 1_000;
    let ms = now.epoch_millis();
    let day_floor_ms = ms - ms.rem_euclid(DAY_MS);
    let Ok(midnight) = UtcTimestamp::from_epoch_millis(day_floor_ms) else {
        return now;
    };
    // The three same-day boundaries (04:00, 12:00, 20:00) and the next day's
    // first (28:00 == next-day 04:00), as offsets from UTC midnight.
    for hours in [4i64, 12, 20, 28] {
        match midnight.checked_add_millis(hours * 60 * 60 * 1_000) {
            Ok(boundary) if boundary > now => return boundary,
            Ok(_) => continue,
            // Overflow at the epoch extremes: degrade to `now` (poll immediately),
            // never panic.
            Err(_) => return now,
        }
    }
    // Unreachable in practice (28h > any time-of-day), but total: the next-day
    // 04:00 always exists below the i64 ceiling except at the extreme handled
    // above, so fall back to `now` rather than an unwrap.
    now
}

/// Poll the funding history ONCE: fetch all markets, VALIDATE each entry's shape
/// (UNTRUSTED DATA, spec 5.11 — quarantine+skip a non-conforming entry), and
/// idempotently insert each well-formed entry with `captured_at = now`. Returns a
/// [`FundingPollReport`]; a whole-response fetch/parse failure is reported as a
/// flagged fetch failure (alert-and-continue), NEVER a panic. A repo (ledger)
/// error on an otherwise well-formed insert bubbles as `DaemonError` (the caller
/// treats it as alert-and-continue at the loop level) — it is a real
/// infrastructure fault, not untrusted data.
pub async fn poll_funding_rates_once<F: FundingHistFetch + ?Sized>(
    fetch: &F,
    repo: &FundingRatesHistoricalRepo,
    now: UtcTimestamp,
) -> Result<FundingPollReport, DaemonError> {
    let captured_at = now.to_iso8601();

    // (a) FETCH the whole response. A failure is alert-and-continue: a flagged
    // report, nothing written, never a panic.
    let response = match fetch.fetch_all().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(FundingPollReport {
                fetch_failed: true,
                fetch_alert: Some(format!("funding poll fetch failed: {e}")),
                ..FundingPollReport::default()
            });
        }
    };

    let mut report = FundingPollReport {
        fetched: response.funding_rates.len(),
        ..FundingPollReport::default()
    };

    // (b) per-entry: VALIDATE shape, then idempotent-insert. A shape violation is
    // QUARANTINED (counted + alerted, skipped); a well-formed entry inserts.
    for entry in &response.funding_rates {
        // Shape validation (UNTRUSTED): non-finite rate, empty ticker, or
        // unparseable funding_time ⇒ quarantine.
        if !entry.funding_rate.is_finite() {
            report.quarantined += 1;
            report.quarantine_alerts.push(format!(
                "quarantined funding entry (non-finite rate) for {:?} @ {:?}",
                entry.market_ticker, entry.funding_time
            ));
            continue;
        }
        if entry.market_ticker.trim().is_empty() {
            report.quarantined += 1;
            report.quarantine_alerts.push(format!(
                "quarantined funding entry (empty market_ticker) @ {:?}",
                entry.funding_time
            ));
            continue;
        }
        let Ok(funding_time) = UtcTimestamp::parse_iso8601(&entry.funding_time) else {
            report.quarantined += 1;
            report.quarantine_alerts.push(format!(
                "quarantined funding entry (unparseable funding_time {:?}) for {:?}",
                entry.funding_time, entry.market_ticker
            ));
            continue;
        };
        // Normalize funding_time to the canonical millisecond form the store +
        // the resolve/score loop agree on (`.000Z`), so the cursor read and the
        // belief lookup key align. mark_price stays VERBATIM (no f64 touch).
        let ft_canon = funding_time.to_iso8601();
        let inserted = repo
            .insert(
                &entry.market_ticker,
                &ft_canon,
                entry.funding_rate,
                &entry.mark_price,
                &captured_at,
            )
            .await
            .map_err(|e| DaemonError::Compose {
                reason: format!(
                    "funding_rates_historical insert {} @ {ft_canon}: {e}",
                    entry.market_ticker
                ),
            })?;
        if inserted {
            report.inserted += 1;
        } else {
            report.skipped_dup += 1;
        }
    }

    Ok(report)
}

/// The Clock-driven poller loop: BACKFILL once (poll immediately), then poll past
/// each 8h boundary, until `cancel` fires. The wait is derived from the injected
/// `&dyn Clock` (re-read every [`POLL_TICK_MS`], racing the cancel signal) — NOT
/// a raw wall-time sleep keyed to a deadline, so a SimClock-driven test advances
/// the clock to step the loop deterministically. On a poll's fetch failure /
/// quarantine, the structured alert reasons are surfaced via `on_report` (the
/// caller routes them to Slack + audits) and the loop CONTINUES — a bad poll
/// never halts the loop.
///
/// `cancel` is a `watch::Receiver<()>`: the workspace-native cancellable,
/// awaitable, `select!`-friendly stop signal (the same role a
/// `tokio_util::CancellationToken` plays, with NO new dependency — `tokio` is
/// already a full-feature workspace dep). The owner triggers shutdown by sending
/// on / dropping the paired `watch::Sender`. The live `drive()` wire-in (building
/// the production [`KineticsPublicFetch`] + spawning this) is the additive
/// follow-on, mirroring the Part 3 precedent.
pub async fn run_funding_poller<F, R>(
    fetch: &F,
    repo: &FundingRatesHistoricalRepo,
    clock: &dyn Clock,
    mut cancel: tokio::sync::watch::Receiver<()>,
    mut on_report: R,
) where
    F: FundingHistFetch + ?Sized,
    R: FnMut(&FundingPollReport),
{
    // BACKFILL once, immediately (then settle into the boundary cadence).
    let report = run_one_poll(fetch, repo, clock).await;
    on_report(&report);

    loop {
        let target = next_funding_poll_at(clock.now());
        // Wait until the injected clock reaches `target`, OR cancel fires. The
        // clock is the timing authority: each tick re-reads `clock.now()` and a
        // SimClock advanced past `target` ends the wait on the next tick (the
        // wall-time `sleep` only bounds the re-check granularity, it does NOT key
        // off the boundary deadline).
        loop {
            if clock.now() >= target {
                break;
            }
            tokio::select! {
                _ = cancel.changed() => return,
                _ = tokio::time::sleep(Duration::from_millis(POLL_TICK_MS)) => {}
            }
        }
        // Re-check cancel once more before polling (it may have fired exactly at
        // the boundary).
        if cancel.has_changed().unwrap_or(true) {
            return;
        }
        let report = run_one_poll(fetch, repo, clock).await;
        on_report(&report);
    }
}

/// One poll at `clock.now()`, never panicking: a repo (ledger) error inside
/// `poll_funding_rates_once` is folded into a fetch-failure-shaped report so the
/// loop's `on_report` still surfaces it and the loop continues (the loop must not
/// crash on a transient DB fault).
async fn run_one_poll<F: FundingHistFetch + ?Sized>(
    fetch: &F,
    repo: &FundingRatesHistoricalRepo,
    clock: &dyn Clock,
) -> FundingPollReport {
    match poll_funding_rates_once(fetch, repo, clock.now()).await {
        Ok(report) => report,
        Err(e) => FundingPollReport {
            fetch_failed: true,
            fetch_alert: Some(format!("funding poll errored: {e}")),
            ..FundingPollReport::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The pure boundary math is also exercised adversarially in
    // tests/funding_poller.rs; this unit pins the cadence constant + the
    // host-pinned base URL so a config drift reds here too.

    #[test]
    fn the_public_base_url_is_pinned_to_the_recorded_prod_host() {
        // The perps_openapi.yaml "Production perps REST API server" — pinned, not
        // payload-derived (the SSRF guard).
        assert_eq!(
            KINETICS_PUBLIC_REST_BASE_URL,
            "https://external-api.kalshi.com/trade-api/v2"
        );
    }

    #[test]
    fn next_boundary_is_strictly_after_and_rolls_over() {
        let at = |iso: &str| UtcTimestamp::parse_iso8601(iso).expect("iso");
        // Strictly after a boundary.
        assert_eq!(
            next_funding_poll_at(at("2026-06-11T12:00:00.000Z")),
            at("2026-06-11T20:00:00.000Z")
        );
        // 20:00 -> next-day 04:00.
        assert_eq!(
            next_funding_poll_at(at("2026-06-11T20:00:00.000Z")),
            at("2026-06-12T04:00:00.000Z")
        );
        // Mid-window.
        assert_eq!(
            next_funding_poll_at(at("2026-06-11T05:30:00.000Z")),
            at("2026-06-11T12:00:00.000Z")
        );
    }
}
