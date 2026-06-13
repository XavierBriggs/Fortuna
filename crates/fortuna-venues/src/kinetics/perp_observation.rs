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
}
