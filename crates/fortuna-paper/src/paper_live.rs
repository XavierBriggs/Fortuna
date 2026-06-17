//! Paper execution on live Kalshi market data.
//!
//! `PaperLiveVenue` composes a read-only Kalshi client with the local
//! `PaperVenue`. Market data reads come from Kalshi; order placement, fills,
//! account state, positions, and settlements remain local paper state.

use crate::{PaperConfig, PaperVenue};
use async_trait::async_trait;
use fortuna_core::book::{Cursor, FeeModel, FillPage, OrderBook};
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_core::market::{MarketId, VenueId, VenueOrderId};
use fortuna_core::money::Cents;
use fortuna_gates::GatedOrder;
use fortuna_venues::fees::ScheduleFeeModel;
use fortuna_venues::kalshi::KalshiReadClient;
use fortuna_venues::{
    Market, MarketFilter, MarketStatus, OpenOrder, SettlementPage, Venue, VenueError, VenuePosition,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Composite venue for `stage = "paper"` with live Kalshi reads.
pub struct PaperLiveVenue {
    read: KalshiReadClient,
    paper: PaperVenue,
    trade_cursors: Mutex<BTreeMap<MarketId, UtcTimestamp>>,
}

impl PaperLiveVenue {
    pub fn new(
        read: KalshiReadClient,
        clock: Arc<dyn Clock>,
        fees: ScheduleFeeModel,
        config: PaperConfig,
        starting_cash: Cents,
    ) -> Result<Self, VenueError> {
        let boot_ms = clock.now().epoch_millis();
        let paper = PaperVenue::new_with_id_prefix(
            VenueId::new("paper-live").map_err(|e| VenueError::Invalid {
                reason: format!("paper-live venue id: {e}"),
            })?,
            clock,
            fees,
            config,
            starting_cash,
            format!("live-{boot_ms}"),
        )?;
        Ok(Self::with_paper(read, paper))
    }

    pub fn with_paper(read: KalshiReadClient, paper: PaperVenue) -> Self {
        Self {
            read,
            paper,
            trade_cursors: Mutex::new(BTreeMap::new()),
        }
    }

    fn lock_cursors(&self) -> MutexGuard<'_, BTreeMap<MarketId, UtcTimestamp>> {
        self.trade_cursors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Pull live market, book, and public-trade data into the local paper
    /// engine. This is deliberately separate from `place`: a caller may run it
    /// once per strategy tick before evaluating and submitting paper orders.
    pub async fn refresh_market_data_for(&self, filter: MarketFilter) -> Result<(), VenueError> {
        let markets = self.read.markets(filter).await?;
        for market in markets {
            self.refresh_one_market(market).await?;
        }
        Ok(())
    }

    /// Pull live data only for the runner's active book-poll universe. The
    /// market catalog read is still broad enough to obtain real metadata, but
    /// orderbook/trade reads are scoped to the requested ids.
    pub async fn refresh_market_data_for_markets(
        &self,
        market_ids: &[MarketId],
    ) -> Result<(), VenueError> {
        let wanted: BTreeSet<MarketId> = market_ids.iter().cloned().collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let markets = self
            .read
            .markets(MarketFilter {
                category: None,
                status: Some(MarketStatus::Trading),
            })
            .await?;
        for market in markets.into_iter().filter(|m| wanted.contains(&m.id)) {
            self.refresh_one_market(market).await?;
        }
        Ok(())
    }

    async fn refresh_one_market(&self, market: Market) -> Result<(), VenueError> {
        self.paper.add_market(market.clone());
        let book = self.read.book(&market.id).await?;
        self.paper
            .apply_book(&market.id, book.yes_bids, book.yes_asks)?;

        let since_ts = self.lock_cursors().get(&market.id).copied();
        let trades = self.read.recent_trades(&market.id, since_ts).await?;
        let mut newest = since_ts;
        for trade in trades {
            self.paper
                .apply_public_trade(trade.market(), trade.yes_price(), trade.qty())?;
            newest = Some(match newest {
                Some(ts) => ts.max(trade.ts()),
                None => trade.ts(),
            });
        }
        if let Some(ts) = newest {
            self.lock_cursors().insert(market.id.clone(), ts);
        }
        Ok(())
    }
}

#[async_trait]
impl Venue for PaperLiveVenue {
    fn id(&self) -> VenueId {
        self.paper.id()
    }

    async fn refresh_market_data(&self) -> Result<(), VenueError> {
        self.refresh_market_data_for(MarketFilter {
            category: None,
            status: Some(MarketStatus::Trading),
        })
        .await
    }

    async fn refresh_market_data_for_markets(
        &self,
        markets: &[MarketId],
    ) -> Result<(), VenueError> {
        PaperLiveVenue::refresh_market_data_for_markets(self, markets).await
    }

    async fn markets(&self, filter: MarketFilter) -> Result<Vec<Market>, VenueError> {
        self.read.markets(filter).await
    }

    async fn book(&self, market: &MarketId) -> Result<OrderBook, VenueError> {
        self.read.book(market).await
    }

    async fn place(&self, order: GatedOrder) -> Result<VenueOrderId, VenueError> {
        self.paper.place(order).await
    }

    async fn cancel(&self, id: &VenueOrderId) -> Result<(), VenueError> {
        self.paper.cancel(id).await
    }

    async fn positions(&self) -> Result<Vec<VenuePosition>, VenueError> {
        self.paper.positions().await
    }

    async fn open_orders(&self) -> Result<Vec<OpenOrder>, VenueError> {
        self.paper.open_orders().await
    }

    async fn balance(&self) -> Result<Cents, VenueError> {
        self.paper.balance().await
    }

    async fn account(&self) -> Result<(Cents, Cents), VenueError> {
        self.paper.account().await
    }

    async fn fills_since(&self, cursor: Cursor) -> Result<FillPage, VenueError> {
        self.paper.fills_since(cursor).await
    }

    async fn settlements_since(&self, cursor: Cursor) -> Result<SettlementPage, VenueError> {
        self.paper.settlements_since(cursor).await
    }

    fn fee_model(&self) -> &dyn FeeModel {
        self.paper.fee_model()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fortuna_core::clock::SimClock;
    use fortuna_core::ids::{IdGen, IntentId};
    use fortuna_core::market::{Action, ClientOrderId, Contracts, Side, StrategyId};
    use fortuna_venues::kalshi::client::{KalshiTransport, MockKalshiTransport};
    use fortuna_venues::{MarketStatus, PriceLevel};
    use std::collections::BTreeSet;

    fn t0() -> UtcTimestamp {
        UtcTimestamp::parse_iso8601("2026-06-16T12:00:00.000Z").unwrap()
    }

    fn clock() -> Arc<SimClock> {
        Arc::new(SimClock::new(t0()))
    }

    fn fee_model() -> ScheduleFeeModel {
        let schedule: fortuna_venues::fees::FeeSchedule = toml::from_str(
            r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
            maker_coeff = "0"
            "#,
        )
        .unwrap();
        ScheduleFeeModel::new(vec![schedule]).unwrap()
    }

    fn market_id() -> MarketId {
        MarketId::new("KXTEST-26JUN16-T50").unwrap()
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

    fn two_markets_body() -> serde_json::Value {
        serde_json::json!({
            "markets": [
                {
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
                },
                {
                    "ticker": "KXTEST-26JUN16-T60",
                    "event_ticker": "KXTEST-26JUN16",
                    "market_type": "binary",
                    "title": "Will the sibling test settle yes?",
                    "yes_sub_title": "Yes",
                    "no_sub_title": "No",
                    "status": "active",
                    "strike_type": "greater",
                    "floor_strike": 60,
                    "close_time": "2026-06-16T17:00:00Z",
                    "settlement_timer_seconds": 3600,
                    "notional_value_dollars": "1.0000",
                    "price_level_structure": "linear_cent",
                    "volume_fp": "10.00"
                }
            ],
            "cursor": ""
        })
    }

    fn book_body() -> serde_json::Value {
        serde_json::json!({
            "orderbook_fp": {
                "yes_dollars": [["0.3000", "4.00"]],
                "no_dollars": [["0.4000", "5.00"]]
            }
        })
    }

    fn empty_trades_body() -> serde_json::Value {
        serde_json::json!({
            "trades": [],
            "cursor": ""
        })
    }

    fn through_trade_body() -> serde_json::Value {
        serde_json::json!({
            "trades": [{
                "trade_id": "trade-1",
                "ticker": "KXTEST-26JUN16-T50",
                "yes_price_dollars": "0.3900",
                "no_price_dollars": "0.6100",
                "count_fp": "2.00",
                "taker_outcome_side": "yes",
                "taker_book_side": "bid",
                "created_time": "2026-06-16T12:00:05.000Z"
            }],
            "cursor": ""
        })
    }

    fn gated_buy(seed: u64, price: i64, qty: i64) -> GatedOrder {
        use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline};

        let cfg: GateConfig = toml::from_str(
            r#"
            [global]
            max_total_exposure_cents = 10000000
            max_daily_loss_cents = 50000
            min_order_contracts = 1
            max_order_contracts = 10000
            price_band_cents = 49
            max_cross_cents = 98
            per_market_exposure_cents = 10000000
            per_event_exposure_cents = 10000000
            require_event_mapping = false

            [per_strategy.paper_live_test]
            max_exposure_cents = 10000000
            max_order_notional_cents = 10000000
            min_net_edge_bps = 0

            [rate."paper-live"]
            burst = 100000
            sustained_per_min = 100000
            market_burst = 100000
            market_sustained_per_min = 100000
            "#,
        )
        .unwrap();
        let mut pipeline = GatePipeline::new(cfg).unwrap();
        let mut ids = IdGen::new(seed);
        let intent = IntentId::new(ids.next(t0()).unwrap());
        let candidate = CandidateOrder {
            intent_id: intent,
            strategy: StrategyId::new("paper_live_test").unwrap(),
            venue: VenueId::new("paper-live").unwrap(),
            market: market_id(),
            side: Side::Yes,
            action: Action::Buy,
            limit_price: Cents::new(price),
            qty: Contracts::new(qty),
            fair_value: Cents::new((price + 5).min(99)),
            client_order_id: ClientOrderId::from_intent(intent),
        };
        let fees = fee_model();
        let recent = BTreeSet::new();
        let inputs = GateInputs {
            now: t0(),
            open_exposure_cents: Cents::ZERO,
            market_exposure_cents: Cents::ZERO,
            strategy_exposure_cents: Cents::ZERO,
            event_exposure_cents: Cents::ZERO,
            event_id: None,
            book: None,
            last_trade_price: Some(Cents::new(50)),
            fee_model: &fees,
            category: None,
            own_resting: &[],
            recent_client_order_ids: &recent,
        };
        pipeline.evaluate(&candidate, &inputs).gated.unwrap()
    }

    fn paper_live_with_clock(
        mock: Arc<MockKalshiTransport>,
        clock: Arc<dyn Clock>,
    ) -> PaperLiveVenue {
        let read = KalshiReadClient::new(
            mock as Arc<dyn KalshiTransport>,
            clock.clone(),
            vec!["KXTEST".to_string()],
        )
        .unwrap();
        PaperLiveVenue::new(
            read,
            clock,
            fee_model(),
            PaperConfig {
                maker_haircut_pct: 100,
            },
            Cents::new(100_000),
        )
        .unwrap()
    }

    fn paper_live(mock: Arc<MockKalshiTransport>) -> PaperLiveVenue {
        paper_live_with_clock(mock, clock())
    }

    #[test]
    fn live_reads_feed_paper_execution_without_live_order_calls() {
        let mock = Arc::new(MockKalshiTransport::new());
        mock.push_ok(200, series_body());
        mock.push_ok(200, markets_body());
        mock.push_ok(200, book_body());
        mock.push_ok(200, empty_trades_body());
        mock.push_ok(200, markets_body());
        mock.push_ok(200, book_body());
        mock.push_ok(200, through_trade_body());

        let venue = paper_live(mock.clone());
        let filter = MarketFilter {
            category: None,
            status: Some(MarketStatus::Trading),
        };

        futures::executor::block_on(venue.refresh_market_data_for(filter.clone()))
            .expect("initial live refresh");
        let order_id =
            futures::executor::block_on(venue.place(gated_buy(1, 40, 1))).expect("paper place");
        assert!(order_id.as_str().starts_with("paper-live-"));
        assert!(
            futures::executor::block_on(venue.fills_since(Cursor::start()))
                .unwrap()
                .fills
                .is_empty(),
            "resting paper order should not fill before a through print"
        );

        futures::executor::block_on(venue.refresh_market_data_for(filter)).expect("trade refresh");
        let fills = futures::executor::block_on(venue.fills_since(Cursor::start()))
            .unwrap()
            .fills;
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].market, market_id());
        assert_eq!(fills[0].price, Cents::new(40));
        assert_eq!(fills[0].qty.raw(), 1);
        assert!(fills[0].is_maker);
        assert_eq!(
            futures::executor::block_on(venue.balance()).unwrap(),
            Cents::new(99_960)
        );

        let calls = mock.calls();
        assert!(calls.iter().all(|call| call.method == "GET"));
        assert!(
            calls
                .iter()
                .all(|call| call.path != "/portfolio/orders" && call.path != "/portfolio/order"),
            "paper-live must not send live order calls: {calls:?}"
        );
    }

    #[test]
    fn paper_live_ids_are_boot_scoped() {
        fn filled_id_at(now: UtcTimestamp) -> String {
            let mock = Arc::new(MockKalshiTransport::new());
            mock.push_ok(200, series_body());
            mock.push_ok(200, markets_body());
            mock.push_ok(200, book_body());
            mock.push_ok(200, empty_trades_body());
            mock.push_ok(200, markets_body());
            mock.push_ok(200, book_body());
            mock.push_ok(200, through_trade_body());

            let clock: Arc<dyn Clock> = Arc::new(SimClock::new(now));
            let venue = paper_live_with_clock(mock, clock);
            let filter = MarketFilter {
                category: None,
                status: Some(MarketStatus::Trading),
            };
            futures::executor::block_on(venue.refresh_market_data_for(filter.clone()))
                .expect("initial live refresh");
            futures::executor::block_on(venue.place(gated_buy(1, 40, 1))).expect("paper place");
            futures::executor::block_on(venue.refresh_market_data_for(filter))
                .expect("trade refresh");
            let fills = futures::executor::block_on(venue.fills_since(Cursor::start()))
                .unwrap()
                .fills;
            assert_eq!(fills.len(), 1);
            fills[0].fill_id.clone()
        }

        let first = filled_id_at(t0());
        let second = filled_id_at(UtcTimestamp::parse_iso8601("2026-06-16T12:00:00.001Z").unwrap());
        assert!(first.starts_with("p-live-"));
        assert!(second.starts_with("p-live-"));
        assert_ne!(
            first, second,
            "paper-live fill ids must not collide across daemon boots"
        );
    }

    #[test]
    fn scoped_refresh_reads_books_only_for_requested_markets() {
        let mock = Arc::new(MockKalshiTransport::new());
        mock.push_ok(200, series_body());
        mock.push_ok(200, two_markets_body());
        mock.push_ok(200, book_body());
        mock.push_ok(200, empty_trades_body());

        let venue = paper_live(mock.clone());
        futures::executor::block_on(
            venue.refresh_market_data_for_markets(&[MarketId::new("KXTEST-26JUN16-T50").unwrap()]),
        )
        .expect("scoped refresh");

        let calls = mock.calls();
        assert_eq!(calls[0].path, "/series/KXTEST");
        assert_eq!(calls[1].path, "/markets");
        assert_eq!(calls[2].path, "/markets/KXTEST-26JUN16-T50/orderbook");
        assert_eq!(calls[3].path, "/markets/trades");
        assert_eq!(
            calls.len(),
            4,
            "unrequested sibling market was not refreshed"
        );
        assert!(
            calls
                .iter()
                .all(|call| !call.path.contains("KXTEST-26JUN16-T60")),
            "scoped refresh must not read the sibling market: {calls:?}"
        );
    }

    #[test]
    fn venue_market_and_book_reads_stay_live() {
        let mock = Arc::new(MockKalshiTransport::new());
        mock.push_ok(200, series_body());
        mock.push_ok(200, markets_body());
        mock.push_ok(200, book_body());

        let venue = paper_live(mock.clone());
        let filter = MarketFilter {
            category: None,
            status: Some(MarketStatus::Trading),
        };
        let markets = futures::executor::block_on(venue.markets(filter)).unwrap();
        assert_eq!(markets.len(), 1);
        assert_eq!(markets[0].id, market_id());

        let book = futures::executor::block_on(venue.book(&market_id())).unwrap();
        assert_eq!(
            book.yes_bids,
            vec![PriceLevel {
                price: Cents::new(30),
                qty: Contracts::new(4)
            }]
        );
        assert_eq!(
            book.yes_asks,
            vec![PriceLevel {
                price: Cents::new(60),
                qty: Contracts::new(5)
            }]
        );
        let calls = mock.calls();
        assert_eq!(calls[0].path, "/series/KXTEST");
        assert_eq!(calls[1].path, "/markets");
        assert_eq!(calls[2].path, "/markets/KXTEST-26JUN16-T50/orderbook");
    }
}
