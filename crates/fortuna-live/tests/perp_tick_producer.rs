//! C-next-1 tests: the live PerpTick producer KERNEL
//! (`fortuna_live::perp_tick_producer`). The producer assembles a PerpTick's
//! perp-domain components from two PUBLIC unauthenticated REST reads —
//! `GET /margin/markets/{ticker}` (mark + BRTI reference) and
//! `GET /margin/funding_rates/estimate?ticker=` (funding rate + next_funding_time)
//! — via `KineticsPerpObservation::from_rest`, fail-closed (spec 5.11). The async
//! loop + daemon wiring is a SEPARATE next slice; these pin the KERNEL.
//!
//! Written FROM the prompt/spec text BEFORE the implementation (TDD). A fake
//! `PerpTickFetch` injects the COMMITTED fixtures (or a crafted malformed
//! market) — no network, no credentials. Coverage, adversarially:
//!   - the HAPPY path: the fake returns the parsed committed fixtures →
//!     `report.tick` is Some, market == KXBTCPERP1, the funding reference_price ==
//!     the recorded "6.3676"; `fetch_failed`/`quarantined` both false;
//!   - a whole-pair FETCH FAILURE (the fetch trait returns Err): alert-and-
//!     continue — `fetch_failed`, no tick, a fetch_alert reason, NO panic;
//!   - the QUARANTINE path (UNTRUSTED DATA, spec 5.11): a market MISSING the A6
//!     BRTI `reference_price` → `quarantined`, no tick, a quarantine_alert
//!     containing "malformed" — NO panic;
//!   - the public base host is the PINNED prod host REUSED from the funding
//!     poller (the SSRF guard's one source of truth).
//!
//! ## Mutation-check note (for a reviewer)
//!
//! Every assertion has teeth:
//!   - DROP the BRTI-reference fail-closed guard in `from_rest` (emit an
//!     anchorless tick) and `a_market_missing_the_brti_reference_is_quarantined`
//!     reds: `quarantined` would be false and a tick would be emitted.
//!   - SWALLOW the fetch error (e.g. emit an empty tick) and
//!     `a_fetch_failure_is_alert_and_continue_not_a_panic` reds: `fetch_failed`
//!     would be false / a tick would appear.
//!   - REPOINT the pinned host to a payload-derived or wrong base and
//!     `the_public_base_url_is_the_pinned_prod_host` reds.

use async_trait::async_trait;
use fortuna_live::perp_tick_producer::{poll_perp_ticks_once, PerpTickFetch};
use fortuna_venues::kinetics::dto::{self, FundingEstimateResponse, MarginMarket, MarketResponse};
use fortuna_venues::VenueError;

/// The operator-recorded market fixture (`{"market":{...}}`, KXBTCPERP1).
const MARKET_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/kinetics-perps/markets__single.json"
));
/// The operator-recorded funding-estimate fixture (KXBTCPERP1).
const ESTIMATE_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/kinetics-perps/funding__rates_estimate.json"
));

fn recorded_market() -> MarginMarket {
    serde_json::from_str::<MarketResponse>(MARKET_FIXTURE)
        .expect("markets__single fixture parses as MarketResponse")
        .market
}

fn recorded_estimate() -> FundingEstimateResponse {
    serde_json::from_str::<FundingEstimateResponse>(ESTIMATE_FIXTURE)
        .expect("funding__rates_estimate fixture parses")
}

/// What a [`FakeFetch`] yields. The DTOs are cloneable; the failure case is a
/// flag (the `VenueError` is built fresh per call, since `VenueError` is not
/// `Clone`).
enum FakeOutcome {
    /// Return this `(market, estimate)` pair.
    Pair(Box<(MarginMarket, FundingEstimateResponse)>),
    /// Return a simulated outage `VenueError` (alert-and-continue path).
    Outage,
}

/// A fake fetch that yields whatever it is built with — no network, no
/// credentials.
struct FakeFetch {
    outcome: FakeOutcome,
}

impl FakeFetch {
    fn ok(market: MarginMarket, estimate: FundingEstimateResponse) -> Self {
        FakeFetch {
            outcome: FakeOutcome::Pair(Box::new((market, estimate))),
        }
    }

    fn outage() -> Self {
        FakeFetch {
            outcome: FakeOutcome::Outage,
        }
    }
}

#[async_trait]
impl PerpTickFetch for FakeFetch {
    async fn fetch_market_and_estimate(
        &self,
        _ticker: &str,
    ) -> Result<(MarginMarket, FundingEstimateResponse), VenueError> {
        match &self.outcome {
            FakeOutcome::Pair(pair) => Ok((**pair).clone()),
            FakeOutcome::Outage => Err(VenueError::Outage {
                venue: "kinetics".to_string(),
                reason: "simulated outage".to_string(),
            }),
        }
    }
}

#[tokio::test]
async fn poll_once_yields_the_mapped_perp_tick_from_recorded_market_and_estimate() {
    let fetch = FakeFetch::ok(recorded_market(), recorded_estimate());
    let report = poll_perp_ticks_once(&fetch, "KXBTCPERP1").await;

    assert!(!report.fetch_failed, "clean fetch: {report:?}");
    assert!(!report.quarantined, "well-formed payload: {report:?}");
    assert!(report.fetch_alert.is_none());
    assert!(report.quarantine_alert.is_none());

    let (market, _marks, funding) = report.tick.expect("a tick is emitted");
    assert_eq!(
        market,
        fortuna_core::market::MarketId::new("KXBTCPERP1").unwrap()
    );
    assert_eq!(
        funding.reference_price,
        dto::parse_perp_price("6.3676").unwrap()
    );
}

#[tokio::test]
async fn a_fetch_failure_is_alert_and_continue_not_a_panic() {
    let fetch = FakeFetch::outage();
    let report = poll_perp_ticks_once(&fetch, "KXBTCPERP1").await;

    assert!(
        report.fetch_failed,
        "a fetch error must be flagged: {report:?}"
    );
    assert!(report.tick.is_none(), "no tick on a fetch failure");
    assert!(
        report.fetch_alert.is_some(),
        "a fetch-failure alert reason is recorded"
    );
    assert!(!report.quarantined);
}

#[tokio::test]
async fn a_market_missing_the_brti_reference_is_quarantined() {
    // The market is well-fetched but MALFORMED for basis-v2: the A6 BRTI
    // reference is absent → fail closed (quarantine), never an anchorless tick.
    let mut market = recorded_market();
    market.reference_price = None;
    let fetch = FakeFetch::ok(market, recorded_estimate());
    let report = poll_perp_ticks_once(&fetch, "KXBTCPERP1").await;

    assert!(
        report.quarantined,
        "an absent BRTI anchor must quarantine: {report:?}"
    );
    assert!(report.tick.is_none(), "no tick on a malformed payload");
    let alert = report
        .quarantine_alert
        .expect("a quarantine alert reason is recorded");
    assert!(
        alert.contains("malformed"),
        "the quarantine alert names the malformed venue data: {alert:?}"
    );
    assert!(!report.fetch_failed);
}

#[test]
fn the_public_base_url_is_the_pinned_prod_host() {
    assert_eq!(
        fortuna_live::funding_poller::KINETICS_PUBLIC_REST_BASE_URL,
        "https://external-api.kalshi.com/trade-api/v2"
    );
}
