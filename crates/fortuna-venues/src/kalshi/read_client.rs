//! Read-only Kalshi market-data surface for paper-on-live.
//!
//! This type intentionally does **not** implement [`crate::Venue`] and exposes
//! no `place`/`cancel` methods. Paper-on-live composition can hold signed prod
//! market-data credentials here while execution is delegated to a local paper
//! venue; a `GatedOrder` has no method on this type that can send it to Kalshi.

use crate::kalshi::adapter::{KalshiVenue, PublicTrade};
use crate::kalshi::client::KalshiTransport;
use crate::{Cursor, FeeModel, Market, MarketFilter, OrderBook, SettlementPage, Venue, VenueError};
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_core::market::{MarketId, VenueId};
use std::sync::Arc;

/// Read-only wrapper around the Kalshi adapter's market-data methods.
///
/// The inner adapter is reused for parsing/cache behavior, but only read
/// methods are re-exposed. In particular, this type has no `place` or `cancel`
/// method and does not implement [`Venue`].
pub struct KalshiReadClient {
    inner: KalshiVenue,
}

impl KalshiReadClient {
    /// Build a read-only client scoped to the configured series universe.
    pub fn new(
        transport: Arc<dyn KalshiTransport>,
        clock: Arc<dyn Clock>,
        series_tickers: Vec<String>,
    ) -> Result<Self, VenueError> {
        Ok(Self {
            inner: KalshiVenue::new(
                VenueId::new("kalshi").map_err(|e| VenueError::Invalid {
                    reason: format!("kalshi read client venue id: {e}"),
                })?,
                transport,
                clock,
                series_tickers,
            )?,
        })
    }

    pub fn id(&self) -> VenueId {
        self.inner.id()
    }

    pub async fn markets(&self, filter: MarketFilter) -> Result<Vec<Market>, VenueError> {
        self.inner.markets(filter).await
    }

    pub async fn book(&self, market: &MarketId) -> Result<OrderBook, VenueError> {
        self.inner.book(market).await
    }

    pub async fn recent_trades(
        &self,
        market: &MarketId,
        since_ts: Option<UtcTimestamp>,
    ) -> Result<Vec<PublicTrade>, VenueError> {
        self.inner.recent_trades(market, since_ts).await
    }

    pub async fn settlements_since(&self, cursor: Cursor) -> Result<SettlementPage, VenueError> {
        self.inner.settlements_since(cursor).await
    }

    pub fn fee_model(&self) -> &dyn FeeModel {
        self.inner.fee_model()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kalshi::client::MockKalshiTransport;
    use crate::MarketStatus;
    use fortuna_core::book::Cursor;
    use fortuna_core::clock::SimClock;
    use fortuna_core::money::Cents;

    fn clock() -> Arc<dyn Clock> {
        Arc::new(SimClock::new(
            UtcTimestamp::parse_iso8601("2026-06-15T16:00:00.000Z").unwrap(),
        ))
    }

    fn series_body() -> serde_json::Value {
        serde_json::json!({
            "series": {
                "ticker": "KXTEST",
                "title": "Test series",
                "category": "weather",
                "fee_type": "quadratic",
                "fee_multiplier": 1.0,
                "settlement_sources": [{"name": "test", "url": "https://example.test/rules"}]
            }
        })
    }

    fn markets_body() -> serde_json::Value {
        serde_json::json!({
            "markets": [{
                "ticker": "KXTEST-26JUN16-T50",
                "event_ticker": "KXTEST-26JUN16",
                "market_type": "binary",
                "title": "Will the test settle yes?",
                "yes_sub_title": "Yes",
                "no_sub_title": "No",
                "status": "active",
                "strike_type": "greater",
                "floor_strike": 50,
                "close_time": "2026-06-16T17:00:00Z",
                "settlement_timer_seconds": 3600,
                "notional_value_dollars": "1.0000",
                "price_level_structure": "linear_cent",
                "volume_fp": "10.00"
            }],
            "cursor": ""
        })
    }

    #[test]
    fn read_client_exposes_only_read_paths() {
        let mock = Arc::new(MockKalshiTransport::new());
        mock.push_ok(200, series_body());
        mock.push_ok(200, markets_body());
        mock.push_ok(
            200,
            serde_json::json!({
                "orderbook_fp": {
                    "yes_dollars": [["0.3000", "4.00"]],
                    "no_dollars": [["0.6000", "5.00"]]
                }
            }),
        );
        mock.push_ok(
            200,
            serde_json::json!({
                "trades": [{
                    "trade_id": "trade-1",
                    "ticker": "KXTEST-26JUN16-T50",
                    "yes_price_dollars": "0.4100",
                    "no_price_dollars": "0.5900",
                    "count_fp": "2.00",
                    "taker_outcome_side": "yes",
                    "taker_book_side": "bid",
                    "created_time": "2026-06-15T15:48:42.609Z"
                }],
                "cursor": ""
            }),
        );
        mock.push_ok(
            200,
            serde_json::json!({
                "settlements": [],
                "cursor": ""
            }),
        );

        let client = KalshiReadClient::new(
            mock.clone() as Arc<dyn KalshiTransport>,
            clock(),
            vec!["KXTEST".to_string()],
        )
        .expect("read client constructs");
        let market = MarketId::new("KXTEST-26JUN16-T50").unwrap();

        let listed = futures::executor::block_on(client.markets(MarketFilter {
            category: None,
            status: Some(MarketStatus::Trading),
        }))
        .expect("markets read");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, market);

        let book = futures::executor::block_on(client.book(&market)).expect("book read");
        assert_eq!(book.yes_bids[0].price, Cents::new(30));
        assert_eq!(book.yes_asks[0].price, Cents::new(40));

        let trades =
            futures::executor::block_on(client.recent_trades(&market, None)).expect("trades read");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].yes_price(), Cents::new(41));

        let settlements = futures::executor::block_on(client.settlements_since(Cursor::start()))
            .expect("settlements read");
        assert!(settlements.notices.is_empty());

        let calls = mock.calls();
        assert_eq!(calls[0].path, "/series/KXTEST");
        assert_eq!(calls[1].path, "/markets");
        assert_eq!(calls[2].path, "/markets/KXTEST-26JUN16-T50/orderbook");
        assert_eq!(calls[3].path, "/markets/trades");
        assert_eq!(calls[4].path, "/portfolio/settlements");
        assert!(
            calls.iter().all(|c| c.method == "GET"),
            "read client issued only GET calls: {calls:?}"
        );
    }
}
