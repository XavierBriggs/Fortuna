//! `KalshiVenue`: the `Venue` implementation over the V2 REST surface.
//!
//! Doc-derived (research doc 2026-06-10); Sim-development clearance only —
//! see the module docs in `kalshi/mod.rs`. Design decisions that encode
//! researched venue behavior:
//!
//! - **Series-scoped catalog.** Kalshi `Market` objects do not carry their
//!   series ticker; the documented links are `GET /markets?series_ticker=`
//!   and market -> `event_ticker` -> `GET /events/{e}` -> `series_ticker`.
//!   The adapter is configured with the series tickers FORTUNA trades and
//!   lists markets per series (fee fields live on the Series). An empty
//!   series list yields an empty catalog.
//! - **Structure filter.** Only `market_type == "binary"` with
//!   `price_level_structure == "linear_cent"` pass `markets()`;
//!   `deci_cent`/`tapered_deci_cent` (sub-cent ticks) are excluded entirely
//!   (GAPS rule: integer-cent core). Any sub-cent dollar string on a market
//!   we DO process is a hard error.
//! - **Cancel-then-confirm.** The V2 cancel response body can describe the
//!   wrong order (documented bug, changelog 2026-05-21); cancel() therefore
//!   ignores the DELETE body and confirms via `GET /portfolio/orders/{id}`.
//! - **Duplicate place.** Any 409 from order create triggers a lookup of the
//!   existing order by client_order_id (the 409 body's `code` string is
//!   undocumented — fixture checklist #7 — so nothing branches on it).
//! - **Fees.** `fee_model()` is the researched base schedule (quadratic
//!   taker 0.07 / maker 0.0175, ceil-to-cent, effective 2026-02-05);
//!   `reconcile_fee` applies the per-series `fee_type`/`fee_multiplier`
//!   cache and flags discrepancies for the caller to record.

use crate::fees::{FeeSchedule, FormulaKind, ScheduleFeeModel};
use crate::kalshi::client::KalshiTransport;
use crate::kalshi::dto::{
    self, error_reason, from_direction, to_book_side, CreateOrderV2Request, CreateOrderV2Response,
    GetBalanceResponse, GetEventResponse, GetFillsResponse, GetMarketResponse, GetMarketsResponse,
    GetOrderResponse, GetOrderbookResponse, GetOrdersResponse, GetPositionsResponse, KalshiFeeType,
    KalshiMarket, KalshiMarketStatus, KalshiOrderStatus, KalshiSeries, KalshiStp,
    KalshiTimeInForce,
};
use crate::{
    Cursor, Fill, FillPage, Market, MarketFilter, MarketStatus, OpenOrder, SettlementMeta, Venue,
    VenueError, VenuePosition,
};
use async_trait::async_trait;
use fortuna_core::book::{FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_core::market::{ClientOrderId, MarketId, VenueId, VenueOrderId};
use fortuna_core::money::Cents;
use fortuna_gates::GatedOrder;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Researched fee constants (fee schedule PDF, "Last updated and effective:
/// Feb 5, 2026"; docs/research/venue/kalshi-fees-2026-06-09/research.md):
/// taker = ceil_to_cent(m x 0.07 x C x P x (1-P)),
/// maker = ceil_to_cent(m x 0.0175 x C x P x (1-P)) on maker-fee series.
const KALSHI_TAKER_COEFF: &str = "0.07";
const KALSHI_MAKER_COEFF: &str = "0.0175";
const KALSHI_FEE_EFFECTIVE: &str = "2026-02-05";

/// Page size for catalog/portfolio listings (documented max 1000).
const PAGE_LIMIT: &str = "1000";
/// Hard guard against a venue cursor that never terminates.
const MAX_PAGES: usize = 1_000;
/// Page cap per status bucket when resolving a duplicate client_order_id.
const MAX_LOOKUP_PAGES: usize = 10;

/// Modeled-vs-charged fee comparison for one fill. `matches` tolerates the
/// venue charging up to one cent LESS than the model (documented per-order
/// rounding rebates); any overcharge or larger drift is a discrepancy the
/// caller must record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeReconciliation {
    pub modeled: Cents,
    pub charged: Cents,
    pub matches: bool,
}

/// Cached per-series fee + metadata (research §3: fees live on the Series).
struct SeriesFees {
    category: String,
    resolution_source: String,
    fee_type: KalshiFeeType,
    /// Fee model with the series multiplier folded into the coefficients.
    /// `None` for `flat`/unknown fee types: those are refused, never
    /// guessed (fees research uncertainty #1).
    model: Option<ScheduleFeeModel>,
}

#[derive(Default)]
struct AdapterState {
    /// series ticker -> cached fees/metadata.
    series: BTreeMap<String, Arc<SeriesFees>>,
    /// market ticker -> series ticker (populated by markets() and
    /// ensure_series_for_market()).
    market_series: BTreeMap<String, String>,
    /// venue order id -> client order id (fills carry no client_order_id;
    /// populated by place(), open_orders(), and on-demand order lookups).
    coid_by_order: BTreeMap<String, ClientOrderId>,
}

/// The Kalshi venue adapter. See module docs for clearance status.
pub struct KalshiVenue {
    venue_id: VenueId,
    transport: Arc<dyn KalshiTransport>,
    clock: Arc<dyn Clock>,
    series_tickers: Vec<String>,
    fees: ScheduleFeeModel,
    state: Mutex<AdapterState>,
}

impl KalshiVenue {
    /// `series_tickers` is the configured trading universe; `markets()` is
    /// scoped to it (empty list => empty catalog).
    pub fn new(
        venue_id: VenueId,
        transport: Arc<dyn KalshiTransport>,
        clock: Arc<dyn Clock>,
        series_tickers: Vec<String>,
    ) -> Result<Self, VenueError> {
        let fees = base_fee_model(Decimal::ONE, /* maker fees */ true)?;
        Ok(KalshiVenue {
            venue_id,
            transport,
            clock,
            series_tickers,
            fees,
            state: Mutex::new(AdapterState::default()),
        })
    }

    fn lock(&self) -> MutexGuard<'_, AdapterState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: Option<&str>,
        what: &str,
    ) -> Result<T, VenueError> {
        let (status, body) = self.transport.request("GET", path, query, None).await?;
        if (200..300).contains(&status) {
            decode(body, what)
        } else {
            Err(fail_status(status, &body, what, false))
        }
    }

    /// Fetch-and-cache the series fee/metadata record.
    async fn series_info(&self, series_ticker: &str) -> Result<Arc<SeriesFees>, VenueError> {
        if let Some(cached) = self.lock().series.get(series_ticker).cloned() {
            return Ok(cached);
        }
        let path = format!("/series/{series_ticker}");
        let what = format!("GET /series/{series_ticker}");
        let resp: dto::GetSeriesResponse = self.get_json(&path, None, &what).await?;
        let fees = Arc::new(build_series_fees(&resp.series)?);
        self.lock()
            .series
            .insert(series_ticker.to_string(), Arc::clone(&fees));
        Ok(fees)
    }

    /// Resolve and cache a market's series via the documented chain
    /// market -> event_ticker -> event.series_ticker, then warm the series
    /// fee cache. Lets `reconcile_fee` (sync, cache-only) work for fills on
    /// markets this process has not listed via `markets()`.
    pub async fn ensure_series_for_market(&self, market: &MarketId) -> Result<(), VenueError> {
        if self.lock().market_series.contains_key(market.as_str()) {
            return Ok(());
        }
        let ticker = market.as_str();
        let market_resp: GetMarketResponse = self
            .get_json(
                &format!("/markets/{ticker}"),
                None,
                &format!("GET /markets/{ticker}"),
            )
            .await?;
        let event_ticker = market_resp.market.event_ticker;
        let event_resp: GetEventResponse = self
            .get_json(
                &format!("/events/{event_ticker}"),
                None,
                &format!("GET /events/{event_ticker}"),
            )
            .await?;
        let series_ticker = event_resp.event.series_ticker;
        self.series_info(&series_ticker).await?;
        self.lock()
            .market_series
            .insert(ticker.to_string(), series_ticker);
        Ok(())
    }

    /// Modeled-vs-charged fee for one of OUR fills, using the per-series
    /// multiplier cache (sync: callers warm the cache via `markets()` or
    /// `ensure_series_for_market`). The fill's own-side price feeds the
    /// quadratic directly: P(1-P) is symmetric, so YES- and NO-space prices
    /// model the same fee.
    pub fn reconcile_fee(&self, fill: &Fill) -> Result<FeeReconciliation, VenueError> {
        let (series_ticker, series) = {
            let st = self.lock();
            let ticker = st
                .market_series
                .get(fill.market.as_str())
                .cloned()
                .ok_or_else(|| VenueError::Invalid {
                    reason: format!(
                        "series unknown for market {}; call markets() or \
                         ensure_series_for_market() before reconciling fees",
                        fill.market
                    ),
                })?;
            let series = st
                .series
                .get(&ticker)
                .cloned()
                .ok_or_else(|| VenueError::Invalid {
                    reason: format!("series {ticker} missing from fee cache (internal)"),
                })?;
            (ticker, series)
        };
        let model = series.model.as_ref().ok_or_else(|| VenueError::Invalid {
            reason: format!(
                "series {series_ticker} has fee_type {:?} which has no documented fee math; \
                 refusing to model (fees research uncertainty #1)",
                series.fee_type
            ),
        })?;
        let role = if fill.is_maker {
            FillRole::Maker
        } else {
            FillRole::Taker
        };
        let modeled = model.fee(role, fill.price, fill.qty, None, fill.at)?;
        let charged = fill.fee;
        let diff = modeled.raw() - charged.raw();
        // The venue may legitimately charge up to one cent less per order
        // than the ceil-per-fill model (documented rounding rebates); it
        // must never charge more.
        let matches = (0..=1).contains(&diff);
        Ok(FeeReconciliation {
            modeled,
            charged,
            matches,
        })
    }

    /// Locate an existing order by client_order_id (no documented direct
    /// query exists). Scans resting first, then executed, then canceled,
    /// with a page cap per bucket.
    async fn find_order_by_coid(&self, coid: &str) -> Result<Option<dto::KalshiOrder>, VenueError> {
        for status in ["resting", "executed", "canceled"] {
            let mut cursor = String::new();
            for _ in 0..MAX_LOOKUP_PAGES {
                let mut query = format!("limit={PAGE_LIMIT}&status={status}");
                if !cursor.is_empty() {
                    query.push_str(&format!("&cursor={cursor}"));
                }
                let resp: GetOrdersResponse = self
                    .get_json("/portfolio/orders", Some(&query), "GET /portfolio/orders")
                    .await?;
                if let Some(order) = resp.orders.into_iter().find(|o| o.client_order_id == coid) {
                    return Ok(Some(order));
                }
                cursor = resp.cursor;
                if cursor.is_empty() {
                    break;
                }
            }
        }
        Ok(None)
    }

    /// venue order id -> client order id, via cache then a single-order GET.
    async fn resolve_coid(&self, order_id: &str) -> Result<ClientOrderId, VenueError> {
        if let Some(coid) = self.lock().coid_by_order.get(order_id).cloned() {
            return Ok(coid);
        }
        let resp: GetOrderResponse = self
            .get_json(
                &format!("/portfolio/orders/{order_id}"),
                None,
                &format!("GET /portfolio/orders/{order_id}"),
            )
            .await?;
        let coid =
            ClientOrderId::new(resp.order.client_order_id).map_err(|e| VenueError::Invalid {
                reason: format!("venue returned an empty client_order_id for {order_id}: {e}"),
            })?;
        self.lock()
            .coid_by_order
            .insert(order_id.to_string(), coid.clone());
        Ok(coid)
    }

    fn remember_coid(&self, order_id: &str, coid: &ClientOrderId) {
        self.lock()
            .coid_by_order
            .insert(order_id.to_string(), coid.clone());
    }
}

#[async_trait]
impl Venue for KalshiVenue {
    fn id(&self) -> VenueId {
        self.venue_id.clone()
    }

    async fn markets(&self, filter: MarketFilter) -> Result<Vec<Market>, VenueError> {
        let mut out = Vec::new();
        for series_ticker in self.series_tickers.clone() {
            let series = self.series_info(&series_ticker).await?;
            if let Some(cat) = &filter.category {
                if &series.category != cat {
                    continue;
                }
            }
            let mut cursor = String::new();
            let mut done = false;
            for _ in 0..MAX_PAGES {
                let mut query = format!("limit={PAGE_LIMIT}&series_ticker={series_ticker}");
                if let Some(s) = filter.status.and_then(status_query_value) {
                    query.push_str(&format!("&status={s}"));
                }
                if !cursor.is_empty() {
                    query.push_str(&format!("&cursor={cursor}"));
                }
                let resp: GetMarketsResponse = self
                    .get_json("/markets", Some(&query), "GET /markets")
                    .await?;
                for km in &resp.markets {
                    // Integer-cent core: binaries on whole-cent ticks only.
                    // Sub-cent structures are excluded from the catalog
                    // entirely (deci_cent / tapered_deci_cent).
                    if km.market_type != "binary" || km.price_level_structure != "linear_cent" {
                        continue;
                    }
                    let market = map_market(km, &series, &self.venue_id)?;
                    self.lock()
                        .market_series
                        .insert(km.ticker.clone(), series_ticker.clone());
                    // Client-side status filter on top of the query-side one:
                    // the response lifecycle vocabulary is broader than the
                    // query vocabulary (fixture checklist #21).
                    if filter.status.is_none_or(|s| market.status == s) {
                        out.push(market);
                    }
                }
                cursor = resp.cursor;
                if cursor.is_empty() {
                    done = true;
                    break;
                }
            }
            if !done {
                return Err(VenueError::Invalid {
                    reason: format!(
                        "GET /markets pagination for series {series_ticker} did not terminate \
                         within {MAX_PAGES} pages"
                    ),
                });
            }
        }
        Ok(out)
    }

    async fn book(&self, market: &MarketId) -> Result<OrderBook, VenueError> {
        let ticker = market.as_str();
        let resp: GetOrderbookResponse = self
            .get_json(
                &format!("/markets/{ticker}/orderbook"),
                None,
                &format!("GET /markets/{ticker}/orderbook"),
            )
            .await?;
        // Research §4 (doc-verbatim): the book returns YES bids and NO bids
        // only; "a bid for yes at price X is equivalent to an ask for no at
        // price (100-X)", levels best-to-worst. Canonical form: NO bid at q
        // becomes a YES ask at 100-q, order preserved (best NO bid = highest
        // q = lowest mirrored ask). NO-leg pricing of `no_dollars` is
        // fixture checklist #20.
        let mut yes_bids = Vec::with_capacity(resp.orderbook_fp.yes_dollars.len());
        for level in &resp.orderbook_fp.yes_dollars {
            yes_bids.push(PriceLevel {
                price: dto::parse_dollars_to_cents_exact(&level[0])?,
                qty: dto::parse_count_integral(&level[1])?,
            });
        }
        let mut yes_asks = Vec::with_capacity(resp.orderbook_fp.no_dollars.len());
        for level in &resp.orderbook_fp.no_dollars {
            let no_price = dto::parse_dollars_to_cents_exact(&level[0])?;
            yes_asks.push(PriceLevel {
                price: Cents::new(100)
                    .checked_sub(no_price)
                    .map_err(VenueError::Money)?,
                qty: dto::parse_count_integral(&level[1])?,
            });
        }
        let book = OrderBook {
            market: market.clone(),
            as_of: self.clock.now(),
            yes_bids,
            yes_asks,
        };
        book.validate().map_err(|e| VenueError::Invalid {
            reason: format!("kalshi book for {market} failed validation: {e}"),
        })?;
        Ok(book)
    }

    async fn place(&self, order: GatedOrder) -> Result<VenueOrderId, VenueError> {
        if order.qty().raw() <= 0 {
            return Err(VenueError::Invalid {
                reason: format!("order quantity {} must be positive", order.qty()),
            });
        }
        let (book_side, yes_price) =
            to_book_side(order.side(), order.action(), order.limit_price())?;
        if !(1..=99).contains(&yes_price.raw()) {
            return Err(VenueError::Invalid {
                reason: format!(
                    "yes-leg price {yes_price} outside [1c, 99c] (linear_cent valid range)"
                ),
            });
        }
        let coid = order.client_order_id().clone();
        let request = CreateOrderV2Request {
            ticker: order.market().as_str().to_string(),
            client_order_id: coid.as_str().to_string(),
            side: book_side,
            count: dto::format_count(order.qty())?,
            price: dto::format_price_dollars(yes_price)?,
            // The exec layer owns timing (I6); the adapter places plain GTC
            // resting limit orders. taker_at_cross is the conservative STP:
            // OUR aggressing order cancels rather than trading our own book.
            time_in_force: KalshiTimeInForce::GoodTillCanceled,
            self_trade_prevention_type: KalshiStp::TakerAtCross,
            post_only: false,
            cancel_order_on_pause: false,
            reduce_only: false,
            subaccount: 0,
            exchange_index: 0,
        };
        let body = serde_json::to_value(&request).map_err(|e| VenueError::Invalid {
            reason: format!("order serialization failed: {e}"),
        })?;
        let what = "POST /portfolio/events/orders";
        let (status, resp_body) = self
            .transport
            .request("POST", "/portfolio/events/orders", None, Some(body))
            .await?;

        if (200..300).contains(&status) {
            let resp: CreateOrderV2Response = decode(resp_body, what)?;
            let id = VenueOrderId::new(resp.order_id).map_err(|e| VenueError::Invalid {
                reason: format!("venue returned an empty order_id: {e}"),
            })?;
            self.remember_coid(id.as_str(), &coid);
            return Ok(id);
        }

        if status == 409 {
            // Documented: "409 Conflict: Order with this client_order_id
            // already exists". The body's code string is NOT documented
            // (fixture checklist #7), so we branch on the status alone and
            // resolve the existing order by listing.
            let venue_reason = error_reason(&resp_body);
            return match self.find_order_by_coid(coid.as_str()).await? {
                Some(existing) => {
                    let id =
                        VenueOrderId::new(existing.order_id).map_err(|e| VenueError::Invalid {
                            reason: format!("venue returned an empty order_id: {e}"),
                        })?;
                    self.remember_coid(id.as_str(), &coid);
                    Err(VenueError::AlreadyExists { existing: id })
                }
                None => Err(VenueError::Timeout {
                    operation: format!(
                        "place {coid} (venue reported a duplicate client_order_id [{venue_reason}] \
                         but the existing order was not found; reconcile open orders and fills)"
                    ),
                }),
            };
        }

        Err(fail_status(status, &resp_body, what, true))
    }

    /// DELETE then confirm. The V2 cancel response body is ADVISORY ONLY
    /// (documented wrong-order bug, research §6): state is confirmed via
    /// `GET /portfolio/orders/{id}`, never read from the DELETE body.
    async fn cancel(&self, id: &VenueOrderId) -> Result<(), VenueError> {
        let order_id = id.as_str();
        let what = format!("DELETE /portfolio/events/orders/{order_id}");
        let (status, body) = self
            .transport
            .request(
                "DELETE",
                &format!("/portfolio/events/orders/{order_id}"),
                None,
                None,
            )
            .await?;
        if status == 404 {
            return Err(VenueError::NotFound {
                what: format!("order {order_id}"),
            });
        }
        if !(200..300).contains(&status) {
            return Err(fail_status(status, &body, &what, true));
        }

        // Reconcile-after-cancel.
        let confirm: Result<GetOrderResponse, VenueError> = self
            .get_json(
                &format!("/portfolio/orders/{order_id}"),
                None,
                &format!("GET /portfolio/orders/{order_id}"),
            )
            .await;
        match confirm {
            Ok(resp) => match resp.order.status {
                KalshiOrderStatus::Canceled => Ok(()),
                KalshiOrderStatus::Executed => Err(VenueError::Rejected {
                    reason: format!(
                        "cancel of {order_id} had no effect: order already fully executed \
                         (fills will arrive via fills_since)"
                    ),
                }),
                KalshiOrderStatus::Resting | KalshiOrderStatus::Unknown => {
                    Err(VenueError::Timeout {
                        operation: format!(
                            "cancel {order_id} (DELETE acknowledged but the order reads \
                             {:?} on reconcile; effect unknown)",
                            resp.order.status
                        ),
                    })
                }
            },
            Err(e) => Err(VenueError::Timeout {
                operation: format!(
                    "cancel {order_id} (DELETE acknowledged but reconcile GET failed: {e}; \
                     effect unknown)"
                ),
            }),
        }
    }

    async fn positions(&self) -> Result<Vec<VenuePosition>, VenueError> {
        let mut out = Vec::new();
        let mut cursor = String::new();
        let mut done = false;
        for _ in 0..MAX_PAGES {
            let mut query = format!("limit={PAGE_LIMIT}&count_filter=position");
            if !cursor.is_empty() {
                query.push_str(&format!("&cursor={cursor}"));
            }
            let resp: GetPositionsResponse = self
                .get_json(
                    "/portfolio/positions",
                    Some(&query),
                    "GET /portfolio/positions",
                )
                .await?;
            for p in &resp.market_positions {
                let signed = dto::parse_count_integral(&p.position_fp)?;
                if signed.raw() == 0 {
                    continue;
                }
                // position_fp is SIGNED net: "Negative means NO contracts
                // and positive means YES contracts". Kalshi nets the sides
                // at the venue, so one of the two lots is always zero here.
                let (yes, no) = if signed.raw() > 0 {
                    (signed.raw(), 0)
                } else {
                    (0, -signed.raw())
                };
                // Cost rounds UP (against us) — venue cost can carry
                // sub-cent precision.
                let cost = dto::parse_fee_dollars_ceil(&p.market_exposure_dollars)?;
                out.push(VenuePosition {
                    market: MarketId::new(p.ticker.clone()).map_err(|e| VenueError::Invalid {
                        reason: format!("venue returned an empty position ticker: {e}"),
                    })?,
                    yes,
                    no,
                    cost,
                });
            }
            cursor = resp.cursor.unwrap_or_default();
            if cursor.is_empty() {
                done = true;
                break;
            }
        }
        if !done {
            return Err(VenueError::Invalid {
                reason: format!("positions pagination did not terminate within {MAX_PAGES} pages"),
            });
        }
        Ok(out)
    }

    async fn open_orders(&self) -> Result<Vec<OpenOrder>, VenueError> {
        let mut out = Vec::new();
        let mut cursor = String::new();
        let mut done = false;
        for _ in 0..MAX_PAGES {
            let mut query = format!("limit={PAGE_LIMIT}&status=resting");
            if !cursor.is_empty() {
                query.push_str(&format!("&cursor={cursor}"));
            }
            let resp: GetOrdersResponse = self
                .get_json("/portfolio/orders", Some(&query), "GET /portfolio/orders")
                .await?;
            for o in resp.orders {
                let (side, action) = from_direction(o.outcome_side, o.book_side)?;
                let limit_price = match side {
                    fortuna_core::market::Side::Yes => {
                        dto::parse_dollars_to_cents_exact(&o.yes_price_dollars)?
                    }
                    fortuna_core::market::Side::No => {
                        dto::parse_dollars_to_cents_exact(&o.no_price_dollars)?
                    }
                };
                let coid = ClientOrderId::new(o.client_order_id.clone()).map_err(|e| {
                    VenueError::Invalid {
                        reason: format!(
                            "venue returned an empty client_order_id on order {}: {e}",
                            o.order_id
                        ),
                    }
                })?;
                let venue_order_id =
                    VenueOrderId::new(o.order_id.clone()).map_err(|e| VenueError::Invalid {
                        reason: format!("venue returned an empty order_id: {e}"),
                    })?;
                self.remember_coid(venue_order_id.as_str(), &coid);
                out.push(OpenOrder {
                    venue_order_id,
                    client_order_id: coid,
                    market: MarketId::new(o.ticker).map_err(|e| VenueError::Invalid {
                        reason: format!("venue returned an empty order ticker: {e}"),
                    })?,
                    side,
                    action,
                    limit_price,
                    remaining_qty: dto::parse_count_integral(&o.remaining_count_fp)?,
                });
            }
            cursor = resp.cursor;
            if cursor.is_empty() {
                done = true;
                break;
            }
        }
        if !done {
            return Err(VenueError::Invalid {
                reason: format!(
                    "open-orders pagination did not terminate within {MAX_PAGES} pages"
                ),
            });
        }
        Ok(out)
    }

    async fn balance(&self) -> Result<Cents, VenueError> {
        let resp: GetBalanceResponse = self
            .get_json("/portfolio/balance", None, "GET /portfolio/balance")
            .await?;
        // `balance` is documented integer cents and truncates any sub-cent
        // amount — it never overstates available cash, which is the
        // conservative reading for capital checks.
        Ok(Cents::new(resp.balance))
    }

    /// One venue page per call. Delivery is at-least-once: when the venue
    /// cursor is exhausted (empty), `next_cursor` stays at the POLLED
    /// cursor, so the next poll re-reads the tail and picks up new fills.
    /// Consumers dedup on `fill_id`. (Venue cursor stability across inserts
    /// is fixture checklist #17.)
    async fn fills_since(&self, cursor: Cursor) -> Result<FillPage, VenueError> {
        let mut query = format!("limit={PAGE_LIMIT}");
        if !cursor.0.is_empty() {
            query.push_str(&format!("&cursor={}", cursor.0));
        }
        let resp: GetFillsResponse = self
            .get_json("/portfolio/fills", Some(&query), "GET /portfolio/fills")
            .await?;
        let mut fills = Vec::with_capacity(resp.fills.len());
        for f in &resp.fills {
            let (side, action) = from_direction(f.outcome_side, f.book_side)?;
            let price = match side {
                fortuna_core::market::Side::Yes => {
                    dto::parse_dollars_to_cents_exact(&f.yes_price_dollars)?
                }
                fortuna_core::market::Side::No => {
                    dto::parse_dollars_to_cents_exact(&f.no_price_dollars)?
                }
            };
            let client_order_id = self.resolve_coid(&f.order_id).await?;
            fills.push(Fill {
                fill_id: f.fill_id.clone(),
                venue_order_id: VenueOrderId::new(f.order_id.clone()).map_err(|e| {
                    VenueError::Invalid {
                        reason: format!("venue returned an empty fill order_id: {e}"),
                    }
                })?,
                client_order_id,
                market: MarketId::new(f.ticker.clone()).map_err(|e| VenueError::Invalid {
                    reason: format!("venue returned an empty fill ticker: {e}"),
                })?,
                side,
                action,
                price,
                qty: dto::parse_count_integral(&f.count_fp)?,
                // fee_cost is a dollars STRING; ceil = against us.
                fee: dto::parse_fee_dollars_ceil(&f.fee_cost)?,
                is_maker: !f.is_taker,
                at: fill_time(f.created_time.as_deref(), f.ts, self.clock.now())?,
            });
        }
        let next_cursor = if resp.cursor.is_empty() {
            cursor
        } else {
            Cursor(resp.cursor)
        };
        Ok(FillPage { fills, next_cursor })
    }

    fn fee_model(&self) -> &dyn FeeModel {
        &self.fees
    }
}

// ---------------------------------------------------------------------------
// Pure mapping helpers
// ---------------------------------------------------------------------------

fn decode<T: DeserializeOwned>(body: serde_json::Value, what: &str) -> Result<T, VenueError> {
    serde_json::from_value(body).map_err(|e| VenueError::Invalid {
        reason: format!("{what}: malformed venue response: {e}"),
    })
}

/// Map a non-success HTTP status. `mutation` selects the conservative
/// failure semantics: a 5xx on an order mutation means EFFECT UNKNOWN
/// (Timeout: caller must reconcile), while a 5xx on a read is an Outage.
fn fail_status(status: u16, body: &serde_json::Value, what: &str, mutation: bool) -> VenueError {
    match status {
        429 => VenueError::RateLimited,
        404 => VenueError::NotFound {
            what: what.to_string(),
        },
        400 | 401 | 403 | 409 | 422 => VenueError::Rejected {
            reason: format!("{what}: HTTP {status}: {}", error_reason(body)),
        },
        s if s >= 500 => {
            if mutation {
                VenueError::Timeout {
                    operation: format!(
                        "{what} (HTTP {s}; the operation may have executed — reconcile)"
                    ),
                }
            } else {
                VenueError::Outage {
                    venue: "kalshi".to_string(),
                    reason: format!("{what}: HTTP {s}: {}", error_reason(body)),
                }
            }
        }
        s => VenueError::Invalid {
            reason: format!("{what}: unexpected HTTP {s}: {}", error_reason(body)),
        },
    }
}

/// Response lifecycle status -> our MarketStatus. CONSERVATIVE: anything
/// not positively known to be tradeable maps to a non-tradeable state.
/// Observed-vocabulary confirmation is fixture checklist #21.
fn map_market_status(status: KalshiMarketStatus) -> MarketStatus {
    match status {
        KalshiMarketStatus::Initialized => MarketStatus::Listed,
        // `inactive` semantics are undocumented; treat as halted (not
        // tradeable) rather than guessing "pre-open".
        KalshiMarketStatus::Inactive => MarketStatus::Halted,
        KalshiMarketStatus::Active => MarketStatus::Trading,
        KalshiMarketStatus::Closed => MarketStatus::Expired,
        // Post-determination states that are not yet settled — including
        // disputes and amended determinations — read as Determined.
        KalshiMarketStatus::Determined
        | KalshiMarketStatus::Disputed
        | KalshiMarketStatus::Amended => MarketStatus::Determined,
        KalshiMarketStatus::Finalized => MarketStatus::Settled,
        KalshiMarketStatus::Unknown => MarketStatus::Halted,
    }
}

/// Our MarketFilter status -> the query-side vocabulary (research §3:
/// unopened/open/paused/closed/settled). None = not expressible; the
/// client-side filter still applies.
fn status_query_value(status: MarketStatus) -> Option<&'static str> {
    match status {
        MarketStatus::Listed => Some("unopened"),
        MarketStatus::Trading => Some("open"),
        MarketStatus::Halted => Some("paused"),
        MarketStatus::Expired => Some("closed"),
        MarketStatus::Settled => Some("settled"),
        MarketStatus::Determined | MarketStatus::Voided => None,
    }
}

fn map_market(
    km: &KalshiMarket,
    series: &SeriesFees,
    venue: &VenueId,
) -> Result<Market, VenueError> {
    let close_at =
        UtcTimestamp::parse_iso8601(&km.close_time).map_err(|e| VenueError::Invalid {
            reason: format!("market {} close_time {:?}: {e}", km.ticker, km.close_time),
        })?;
    // Whole-hour ceiling of the settlement timer.
    let lag_secs = km.settlement_timer_seconds.max(0);
    let lag_hours = (lag_secs + 3599) / 3600;
    let expected_lag_hours = u32::try_from(lag_hours).unwrap_or(u32::MAX);
    // notional_value_dollars on a linear_cent binary must be whole cents
    // ($1.0000); a sub-cent payout would break integer-cent settlement math.
    let payout = dto::parse_dollars_to_cents_exact(&km.notional_value_dollars)?;
    let title = match &km.title {
        Some(t) if !t.trim().is_empty() => t.clone(),
        _ => km.yes_sub_title.clone(),
    };
    Ok(Market {
        id: MarketId::new(km.ticker.clone()).map_err(|e| VenueError::Invalid {
            reason: format!("venue returned an empty market ticker: {e}"),
        })?,
        venue: venue.clone(),
        // UNTRUSTED text (spec 5.11): data only, never instructions.
        title,
        category: series.category.clone(),
        status: map_market_status(km.status),
        close_at: Some(close_at),
        settlement: SettlementMeta {
            // Kalshi is its own settlement oracle, per series rulebook.
            oracle_type: "kalshi_rulebook".to_string(),
            resolution_source: series.resolution_source.clone(),
            expected_lag_hours,
        },
        payout_per_contract: payout,
        // Ceil: over-stated volume keeps sub-volume filters conservative.
        volume_contracts: km
            .volume_fp
            .as_deref()
            .map(dto::parse_count_ceil)
            .transpose()?,
    })
}

/// Build a quadratic ceil-to-cent schedule with the series multiplier
/// folded into both coefficients. Multiplying the coefficient is identical
/// to multiplying the fee before rounding (the documented multiplier
/// semantics; maker scaling is inferred — fees research uncertainty #2 —
/// and reconciliation will surface any divergence).
fn base_fee_model(multiplier: Decimal, maker_fees: bool) -> Result<ScheduleFeeModel, VenueError> {
    let taker = Decimal::from_str(KALSHI_TAKER_COEFF)
        .ok()
        .and_then(|c| c.checked_mul(multiplier))
        .ok_or_else(|| VenueError::Invalid {
            reason: format!("taker coefficient overflow at multiplier {multiplier}"),
        })?;
    let maker = if maker_fees {
        Some(
            Decimal::from_str(KALSHI_MAKER_COEFF)
                .ok()
                .and_then(|c| c.checked_mul(multiplier))
                .ok_or_else(|| VenueError::Invalid {
                    reason: format!("maker coefficient overflow at multiplier {multiplier}"),
                })?
                .to_string(),
        )
    } else {
        None
    };
    let schedule = FeeSchedule {
        formula: FormulaKind::Quadratic,
        effective_date: KALSHI_FEE_EFFECTIVE.to_string(),
        taker_coeff: Some(taker.to_string()),
        maker_coeff: maker,
        taker_bps: None,
        maker_bps: None,
        taker_tiers: None,
        maker_tiers: None,
        category_multipliers: None,
        rounding: None, // default Up: fees never round in our favor.
    };
    ScheduleFeeModel::new(vec![schedule]).map_err(VenueError::Fee)
}

fn build_series_fees(series: &KalshiSeries) -> Result<SeriesFees, VenueError> {
    let resolution_source = series
        .settlement_sources
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|s| s.name.as_deref())
        .filter(|n| !n.trim().is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let resolution_source = if resolution_source.is_empty() {
        "kalshi".to_string()
    } else {
        resolution_source
    };
    let model = match series.fee_type {
        KalshiFeeType::Quadratic | KalshiFeeType::QuadraticWithMakerFees => {
            let multiplier = multiplier_decimal(series.fee_multiplier, &series.ticker)?;
            Some(base_fee_model(
                multiplier,
                series.fee_type == KalshiFeeType::QuadraticWithMakerFees,
            )?)
        }
        // `flat` is in the enum but used by zero live series and its math is
        // officially ambiguous; unknown values are future fee shapes. Both
        // are refused at reconcile time, never guessed.
        KalshiFeeType::Flat | KalshiFeeType::Unknown => None,
    };
    Ok(SeriesFees {
        category: series.category.clone(),
        resolution_source,
        fee_type: series.fee_type,
        model,
    })
}

/// The spec types fee_multiplier as a JSON number (double). Convert via its
/// shortest decimal rendering; observed live values (0, 0.5, 1) round-trip
/// exactly. Negative/non-finite multipliers are refused.
fn multiplier_decimal(raw: f64, series: &str) -> Result<Decimal, VenueError> {
    if !raw.is_finite() || raw < 0.0 {
        return Err(VenueError::Invalid {
            reason: format!(
                "series {series} fee_multiplier {raw} is not a finite non-negative number"
            ),
        });
    }
    Decimal::from_str(&format!("{raw}")).map_err(|e| VenueError::Invalid {
        reason: format!("series {series} fee_multiplier {raw} not representable: {e}"),
    })
}

/// Fill timestamp: `created_time` (RFC3339) preferred, then legacy `ts`
/// (Unix seconds), then the injected clock (better a coarse timestamp than
/// a dropped fill; both venue fields are optional in the spec).
fn fill_time(
    created_time: Option<&str>,
    ts: Option<i64>,
    now: UtcTimestamp,
) -> Result<UtcTimestamp, VenueError> {
    if let Some(raw) = created_time {
        if let Ok(t) = UtcTimestamp::parse_iso8601(raw) {
            return Ok(t);
        }
    }
    if let Some(secs) = ts {
        if let Some(millis) = secs.checked_mul(1000) {
            if let Ok(t) = UtcTimestamp::from_epoch_millis(millis) {
                return Ok(t);
            }
        }
    }
    Ok(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping_is_conservative() {
        assert_eq!(
            map_market_status(KalshiMarketStatus::Active),
            MarketStatus::Trading
        );
        assert_eq!(
            map_market_status(KalshiMarketStatus::Initialized),
            MarketStatus::Listed
        );
        assert_eq!(
            map_market_status(KalshiMarketStatus::Inactive),
            MarketStatus::Halted
        );
        assert_eq!(
            map_market_status(KalshiMarketStatus::Closed),
            MarketStatus::Expired
        );
        assert_eq!(
            map_market_status(KalshiMarketStatus::Determined),
            MarketStatus::Determined
        );
        assert_eq!(
            map_market_status(KalshiMarketStatus::Disputed),
            MarketStatus::Determined
        );
        assert_eq!(
            map_market_status(KalshiMarketStatus::Amended),
            MarketStatus::Determined
        );
        assert_eq!(
            map_market_status(KalshiMarketStatus::Finalized),
            MarketStatus::Settled
        );
        // Anything we cannot positively identify is NOT tradeable.
        assert_eq!(
            map_market_status(KalshiMarketStatus::Unknown),
            MarketStatus::Halted
        );
    }

    #[test]
    fn status_query_vocabulary_matches_research() {
        assert_eq!(status_query_value(MarketStatus::Trading), Some("open"));
        assert_eq!(status_query_value(MarketStatus::Listed), Some("unopened"));
        assert_eq!(status_query_value(MarketStatus::Halted), Some("paused"));
        assert_eq!(status_query_value(MarketStatus::Expired), Some("closed"));
        assert_eq!(status_query_value(MarketStatus::Settled), Some("settled"));
        assert_eq!(status_query_value(MarketStatus::Determined), None);
        assert_eq!(status_query_value(MarketStatus::Voided), None);
    }

    #[test]
    fn fill_time_prefers_created_time_then_ts_then_clock() {
        let now = UtcTimestamp::parse_iso8601("2026-06-09T12:00:00Z").unwrap();
        let t = fill_time(Some("2024-01-01T12:00:00Z"), Some(1), now);
        assert_eq!(
            t.ok().map(|t| t.to_iso8601()),
            Some("2024-01-01T12:00:00.000Z".to_string())
        );
        let t = fill_time(None, Some(1704110400), now);
        assert_eq!(
            t.ok().map(|t| t.to_iso8601()),
            Some("2024-01-01T12:00:00.000Z".to_string())
        );
        let t = fill_time(None, None, now);
        assert_eq!(t.ok(), Some(now));
        // Garbage created_time falls through to ts.
        let t = fill_time(Some("not-a-time"), Some(1704110400), now);
        assert_eq!(
            t.ok().map(|t| t.to_iso8601()),
            Some("2024-01-01T12:00:00.000Z".to_string())
        );
    }

    #[test]
    fn multiplier_decimal_handles_observed_values_and_rejects_junk() {
        assert_eq!(
            multiplier_decimal(1.0, "S").ok(),
            Decimal::from_str("1").ok()
        );
        assert_eq!(
            multiplier_decimal(0.5, "S").ok(),
            Decimal::from_str("0.5").ok()
        );
        assert_eq!(
            multiplier_decimal(0.0, "S").ok(),
            Decimal::from_str("0").ok()
        );
        assert!(multiplier_decimal(-1.0, "S").is_err());
        assert!(multiplier_decimal(f64::NAN, "S").is_err());
        assert!(multiplier_decimal(f64::INFINITY, "S").is_err());
    }
}
