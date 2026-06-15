//! The venue-side half of a `PerpTick`, built VERBATIM from a kinetics WS
//! `ticker` frame.
//!
//! The two perp strategies (`funding_forecast`, `perp_event_basis`) consume
//! `EventPayload::PerpTick { venue, market, marks, funding }` off the bus, but
//! the venue crate is deliberately BUS-FREE (it imports only `fortuna_core`
//! domain types, never the bus / `EventPayload`). So this module builds the
//! perp-DOMAIN components — `MarketId` + [`PerpMarks`] + [`FundingObservation`]
//! — from a recorded `ticker` frame; the producer (in the runner/daemon, the
//! bus layer) wraps them in the `PerpTick` event by adding the `venue` id.
//!
//! Every field is a VERBATIM recorded frame field — nothing is derived or
//! invented (design docs/design/perp-strategies-and-scalar-claims.md §2.1, §4;
//! the [`FundingObservation`] field docs name each source). The `ticker` frame
//! is the single richest source: it carries the settlement mark, the funding
//! rate + `next_funding_time`, the CF-Benchmarks reference price, and the
//! capture `ts_ms` in one frame (dto `WsTickerMsg`).

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::MarketId;
use fortuna_core::perp::{FundingObservation, PerpMarks};
use rust_decimal::Decimal;

use super::dto::{self, WsTickerMsg};
use crate::VenueError;

/// The perp-domain observation parsed from a WS `ticker` frame: the `market`,
/// the [`PerpMarks`] (venue settlement mark; no independent conservative mark
/// at the venue boundary → `None`), and the [`FundingObservation`]. The bus
/// `PerpTick` is `{ venue: <kinetics>, market, marks, funding }` — the producer
/// supplies the `venue` id.
#[derive(Debug, Clone, PartialEq)]
pub struct KineticsPerpObservation {
    /// The perp market the frame is for (`market_ticker`).
    pub market: MarketId,
    /// The marks: `venue_settlement` from `settlement_mark_price`; the
    /// conservative mark is unavailable at the venue boundary (`None`).
    pub marks: PerpMarks,
    /// The funding observation: estimate (rate), `next_funding_time`,
    /// `reference_price`, and the capture `obs_at`.
    pub funding: FundingObservation,
}

impl KineticsPerpObservation {
    /// Build the observation from a WS `ticker` frame. Field-by-field VERBATIM
    /// (no derivation):
    ///
    /// - `market`            ← `market_ticker`
    /// - `marks.venue_settlement` ← `settlement_mark_price.price` (ten-thousandths)
    /// - `marks.conservative`     ← `None` (no independent venue-boundary mark)
    /// - `funding.estimate`       ← `funding_rate.rate` (`f64` rate → `Decimal`)
    /// - `funding.next_funding_time` ← `funding_rate.next_funding_time_ms`
    /// - `funding.reference_price`   ← `reference_price.price` (ten-thousandths)
    /// - `funding.obs_at`            ← the frame `ts_ms`
    ///
    /// Returns [`VenueError::Invalid`] (never panics) on a malformed price
    /// string, a non-exact `f64` rate, or an out-of-range timestamp.
    pub fn from_ws_ticker(t: &WsTickerMsg) -> Result<Self, VenueError> {
        let market = MarketId::new(&t.market_ticker).map_err(|e| VenueError::Invalid {
            reason: format!("perp ticker {:?}: {e}", t.market_ticker),
        })?;
        let venue_settlement = dto::parse_perp_price(&t.settlement_mark_price.price)?;
        let reference_price = dto::parse_perp_price(&t.reference_price.price)?;
        // The funding rate is a small dimensionless fraction per window (the
        // venue payload's `f64` rate domain, mirroring the `FundingEstimate`
        // surface); convert at this boundary to the `Decimal` the bus carries.
        let estimate = Decimal::try_from(t.funding_rate.rate).map_err(|e| VenueError::Invalid {
            reason: format!(
                "funding rate {} not representable: {e}",
                t.funding_rate.rate
            ),
        })?;
        let next_funding_time =
            UtcTimestamp::from_epoch_millis(t.funding_rate.next_funding_time_ms).map_err(|e| {
                VenueError::Invalid {
                    reason: format!(
                        "next_funding_time_ms {}: {e}",
                        t.funding_rate.next_funding_time_ms
                    ),
                }
            })?;
        let obs_at = UtcTimestamp::from_epoch_millis(t.ts_ms).map_err(|e| VenueError::Invalid {
            reason: format!("ticker ts_ms {}: {e}", t.ts_ms),
        })?;

        Ok(Self {
            market,
            marks: PerpMarks {
                venue_settlement,
                conservative: None,
            },
            funding: FundingObservation {
                estimate,
                next_funding_time,
                reference_price,
                obs_at,
            },
        })
    }

    /// Build the observation from the two PUBLIC REST surfaces the perp-tick
    /// producer reads: a `GET /margin/markets/{ticker}` market (the settlement
    /// mark + the CF-Benchmarks reference) and a `GET
    /// /margin/funding_rates/estimate` estimate (the funding rate +
    /// `next_funding_time`). Field-by-field VERBATIM (nothing derived/invented),
    /// mirroring [`from_ws_ticker`](Self::from_ws_ticker):
    ///
    /// - `market`                    ← `market.ticker`
    /// - `marks.venue_settlement`    ← `market.settlement_mark_price.price` (ten-thousandths)
    /// - `marks.conservative`        ← `None` (no independent venue-boundary mark)
    /// - `funding.estimate`          ← `estimate.funding_rate` (`f64` rate → `Decimal`)
    /// - `funding.next_funding_time` ← `estimate.next_funding_time` (REST ISO string)
    /// - `funding.reference_price`   ← `market.reference_price.price` (ten-thousandths)
    /// - `funding.obs_at`            ← `market.reference_price.ts_ms` (the BRTI
    ///   anchor's OWN stamp — what the basis-v2 A6 stale-anchor veto measures;
    ///   mirrors `from_ws_ticker` keying `obs_at` off the frame `ts_ms`)
    ///
    /// The two payloads MUST agree on the market: a `market.ticker` /
    /// `estimate.market_ticker` mismatch is rejected (never correlate two
    /// different markets). The `reference_price` is REQUIRED — it is basis-v2's
    /// A6 BRTI anchor; an absent reference fails closed (`Invalid`) rather than
    /// silently emitting an anchorless tick.
    ///
    /// Returns [`VenueError::Invalid`] (never panics) on a ticker mismatch, an
    /// absent settlement/reference stamp, a malformed price string, a non-exact
    /// `f64` rate, or an unparseable `next_funding_time` / `ts_ms`.
    pub fn from_rest(
        market: &crate::kinetics::dto::MarginMarket,
        estimate: &crate::kinetics::dto::FundingEstimateResponse,
    ) -> Result<Self, VenueError> {
        // Cross-check FIRST: the market and the estimate must describe the SAME
        // perp, or these two reads cannot be correlated into one observation.
        if market.ticker != estimate.market_ticker {
            return Err(VenueError::Invalid {
                reason: format!(
                    "perp rest market ticker {:?} != funding estimate market_ticker {:?}",
                    market.ticker, estimate.market_ticker
                ),
            });
        }
        let market_id = MarketId::new(&market.ticker).map_err(|e| VenueError::Invalid {
            reason: format!("perp rest market {:?}: {e}", market.ticker),
        })?;
        let settlement_stamp =
            market
                .settlement_mark_price
                .as_ref()
                .ok_or_else(|| VenueError::Invalid {
                    reason: format!(
                        "perp rest market {:?}: settlement_mark_price absent",
                        market.ticker
                    ),
                })?;
        let venue_settlement = dto::parse_perp_price(&settlement_stamp.price)?;
        // The BRTI reference is the A6 anchor — its absence is a fail-closed
        // Invalid, never a silently anchorless tick (the load-bearing guard).
        let ref_stamp = market
            .reference_price
            .as_ref()
            .ok_or_else(|| VenueError::Invalid {
                reason: format!(
                    "perp rest market {:?}: reference_price absent (A6 BRTI anchor missing)",
                    market.ticker
                ),
            })?;
        let reference_price = dto::parse_perp_price(&ref_stamp.price)?;
        // obs_at is the BRTI anchor's OWN timestamp (the A6 stale-anchor measure),
        // exactly as `from_ws_ticker` keys `obs_at` off the frame `ts_ms`.
        let obs_at =
            UtcTimestamp::from_epoch_millis(ref_stamp.ts_ms).map_err(|e| VenueError::Invalid {
                reason: format!(
                    "perp rest market {:?}: reference_price ts_ms {}: {e}",
                    market.ticker, ref_stamp.ts_ms
                ),
            })?;
        // The funding rate is a small dimensionless per-window fraction (the
        // venue payload's `f64` rate domain); convert at this boundary to the
        // `Decimal` the bus carries.
        let estimate_rate =
            Decimal::try_from(estimate.funding_rate).map_err(|e| VenueError::Invalid {
                reason: format!(
                    "perp rest funding rate {} not representable: {e}",
                    estimate.funding_rate
                ),
            })?;
        // REST gives an ISO-8601 STRING (the WS path used epoch-ms instead).
        let next_funding_time =
            UtcTimestamp::parse_iso8601(&estimate.next_funding_time).map_err(|e| {
                VenueError::Invalid {
                    reason: format!(
                        "perp rest next_funding_time {:?}: {e}",
                        estimate.next_funding_time
                    ),
                }
            })?;

        Ok(Self {
            market: market_id,
            marks: PerpMarks {
                venue_settlement,
                conservative: None,
            },
            funding: FundingObservation {
                estimate: estimate_rate,
                next_funding_time,
                reference_price,
                obs_at,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinetics::dto::{FundingEstimateResponse, MarginMarket, MarketResponse};

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
        // Top level is {"market":{...}} → MarketResponse.
        serde_json::from_str::<MarketResponse>(MARKET_FIXTURE)
            .expect("markets__single fixture parses as MarketResponse")
            .market
    }

    fn recorded_estimate() -> FundingEstimateResponse {
        serde_json::from_str::<FundingEstimateResponse>(ESTIMATE_FIXTURE)
            .expect("funding__rates_estimate fixture parses")
    }

    #[test]
    fn from_rest_maps_every_field_from_the_recorded_market_and_estimate() {
        let market = recorded_market();
        let estimate = recorded_estimate();
        // The recorded reference_price.ts_ms — obs_at must equal exactly this.
        let recorded_ref_ts_ms = market
            .reference_price
            .as_ref()
            .expect("recorded reference_price present")
            .ts_ms;
        assert_eq!(recorded_ref_ts_ms, 1_781_258_941_000);

        let obs = KineticsPerpObservation::from_rest(&market, &estimate).expect("maps cleanly");

        assert_eq!(obs.market, MarketId::new("KXBTCPERP1").unwrap());
        assert_eq!(
            obs.marks.venue_settlement,
            dto::parse_perp_price("6.3332").unwrap()
        );
        assert_eq!(obs.marks.conservative, None);
        assert_eq!(
            obs.funding.reference_price,
            dto::parse_perp_price("6.3676").unwrap()
        );
        assert_eq!(obs.funding.estimate, Decimal::from(0));
        assert_eq!(
            obs.funding.next_funding_time,
            UtcTimestamp::parse_iso8601("2026-06-12T20:00:00Z").unwrap()
        );
        // obs_at is the BRTI anchor's OWN stamp (reference_price.ts_ms).
        assert_eq!(
            obs.funding.obs_at,
            UtcTimestamp::from_epoch_millis(recorded_ref_ts_ms).unwrap()
        );
        assert_eq!(
            obs.funding.obs_at,
            UtcTimestamp::from_epoch_millis(1_781_258_941_000).unwrap()
        );
    }

    #[test]
    fn an_absent_reference_price_fails_closed_invalid() {
        let mut market = recorded_market();
        market.reference_price = None;
        let estimate = recorded_estimate();
        let err = KineticsPerpObservation::from_rest(&market, &estimate)
            .expect_err("absent BRTI anchor must fail closed");
        match err {
            VenueError::Invalid { reason } => {
                assert!(
                    reason.contains("reference"),
                    "reason names the reference price: {reason:?}"
                );
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn an_absent_settlement_mark_fails_closed_invalid() {
        let mut market = recorded_market();
        market.settlement_mark_price = None;
        let estimate = recorded_estimate();
        let err = KineticsPerpObservation::from_rest(&market, &estimate)
            .expect_err("absent settlement mark must fail closed");
        assert!(matches!(err, VenueError::Invalid { .. }), "got {err:?}");
    }

    #[test]
    fn a_market_estimate_ticker_mismatch_is_invalid() {
        let market = recorded_market();
        let mut estimate = recorded_estimate();
        estimate.market_ticker = "KXETHPERP1".to_string();
        let err = KineticsPerpObservation::from_rest(&market, &estimate)
            .expect_err("two different markets must not be correlated");
        match err {
            VenueError::Invalid { reason } => {
                // Names BOTH tickers.
                assert!(reason.contains("KXBTCPERP1"), "names market: {reason:?}");
                assert!(reason.contains("KXETHPERP1"), "names estimate: {reason:?}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn an_unparseable_next_funding_time_is_invalid() {
        let market = recorded_market();
        let mut estimate = recorded_estimate();
        estimate.next_funding_time = "not-a-time".to_string();
        let err = KineticsPerpObservation::from_rest(&market, &estimate)
            .expect_err("a bad next_funding_time must fail closed");
        assert!(matches!(err, VenueError::Invalid { .. }), "got {err:?}");
    }
}
