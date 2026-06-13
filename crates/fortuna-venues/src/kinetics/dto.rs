//! Serde DTOs for the Kinetics (Kalshi margin) perps REST + WS surface.
//! Every shape is transcribed from the OPERATOR-RECORDED fixtures
//! (fixtures/kinetics-perps/, demo environment, 2026-06-12) and the
//! archived specs in docs/research/venue/kinetics-perps-2026-06-10/;
//! nothing is invented. T5.B4 slice 1 (spec 5.15).
//!
//! Reading rules baked in here (fixture findings):
//!
//! - Prices are fixed-point DOLLAR STRINGS at the $0.0001 tick -> they
//!   parse into `PerpPrice` EXACTLY (a sub-tick remainder is a hard
//!   error). FUNDING RATES are JSON NUMBERS (floats), never strings
//!   (finding 10). Dollar notional/equity strings can carry six decimals
//!   -> they stay `Decimal` until an explicit rounding boundary.
//! - Counts are fixed-point strings with fractional trading DISABLED
//!   (research §3): whole-contract parses only; positions are SIGNED.
//! - `tick_size` can be the EMPTY STRING on demo BTC (finding 12):
//!   parse to `Option<PerpPrice>`.
//! - Error bodies arrive in THREE shapes (finding 5): nested
//!   `{"error":{...}}`, flat `{"code","message"}`, bare `{"msg":...}` —
//!   `KineticsApiError` parses all three. Error `code` strings can be
//!   DYNAMIC (`user_not_found:_<uuid>`, finding 8): match with
//!   `code_matches`, never by equality or HTTP status.
//! - REST orderbooks order worst->best on BOTH sides, best at array END
//!   (finding 1, settles research §11.1): `best_bid`/`best_ask` sort
//!   defensively and never assume.
//! - WS frames dispatch on `type`; unknown types degrade to
//!   `WsFrame::Unknown` (preserved, never guessed). The `user_orders`
//!   channel is not guaranteed to emit during a lifecycle (session
//!   notes): nothing here assumes any channel's presence.

use crate::VenueError;
use fortuna_core::market::Contracts;
use fortuna_core::perp::PerpPrice;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// Fixed-point parsing primitives (the typed boundary)
// ---------------------------------------------------------------------------

fn parse_decimal(raw: &str, what: &str) -> Result<Decimal, VenueError> {
    Decimal::from_str(raw).map_err(|e| VenueError::Invalid {
        reason: format!("kinetics {what} {raw:?} is not a decimal string: {e}"),
    })
}

/// Parse a fixed-point dollar string into a `PerpPrice` (ten-thousandths),
/// exactly. A sub-tick remainder is a HARD error: the venue tick is
/// $0.0001 and every recorded price is whole ticks.
pub fn parse_perp_price(raw: &str) -> Result<PerpPrice, VenueError> {
    let d = parse_decimal(raw, "price")?;
    PerpPrice::from_dollars_exact(d).map_err(|e| VenueError::Invalid {
        reason: format!("kinetics price {raw:?} not whole ticks: {e}"),
    })
}

/// `tick_size` tolerance (finding 12): empty string -> None.
pub fn parse_perp_price_opt(raw: &str) -> Result<Option<PerpPrice>, VenueError> {
    if raw.is_empty() {
        return Ok(None);
    }
    parse_perp_price(raw).map(Some)
}

/// Parse a fixed-point count string into WHOLE contracts (fractional
/// trading is disabled; a fractional count in a payload is a hard error).
/// Signed: positions report shorts negative.
pub fn parse_whole_count(raw: &str) -> Result<Contracts, VenueError> {
    let d = parse_decimal(raw, "count")?;
    if !d.fract().is_zero() {
        return Err(VenueError::Invalid {
            reason: format!("kinetics count {raw:?} is fractional (fractional trading disabled)"),
        });
    }
    let n = d.trunc().mantissa() / 10i128.pow(d.trunc().scale());
    i64::try_from(n)
        .map(Contracts::new)
        .map_err(|_| VenueError::Invalid {
            reason: format!("kinetics count {raw:?} out of i64 range"),
        })
}

/// Dollar amounts (equity, notional, fees) as exact decimals; rounding to
/// cents happens at explicit adapter boundaries, not here.
pub fn parse_dollars(raw: &str) -> Result<Decimal, VenueError> {
    parse_decimal(raw, "dollar amount")
}

// ---------------------------------------------------------------------------
// Error envelope (finding 5/8: three shapes, dynamic codes)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct NestedErrorBody {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
}

/// All three recorded error-body shapes.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum KineticsApiError {
    Nested { error: NestedErrorBody },
    Flat { code: String, message: String },
    Bare { msg: String },
}

impl KineticsApiError {
    /// The raw code (nested/flat) or the bare message.
    pub fn raw_code(&self) -> &str {
        match self {
            KineticsApiError::Nested { error } => &error.code,
            KineticsApiError::Flat { code, .. } => code,
            KineticsApiError::Bare { msg } => msg,
        }
    }

    /// Prefix-match a code family: `user_not_found:_<uuid>` matches
    /// `user_not_found` (finding 8 — never key on equality or status).
    pub fn code_matches(&self, family: &str) -> bool {
        let code = self.raw_code();
        code == family
            || code
                .strip_prefix(family)
                .is_some_and(|rest| rest.starts_with(':') || rest.starts_with(":_"))
    }
}

// ---------------------------------------------------------------------------
// Markets
// ---------------------------------------------------------------------------

/// A stamped mark price: `{"price": "6.3760", "ts_ms": 1781237143000}`.
#[derive(Debug, Clone, Deserialize)]
pub struct MarkPriceStamp {
    pub price: String,
    pub ts_ms: i64,
}

/// Quote/mark/volume fields are ABSENT on inactive markets (recorded:
/// the TEST-EQUITY perps in markets__list carry no bid/ask/marks/
/// leverage at all) — every market-state field is optional; only
/// identity and structural fields are required.
#[derive(Debug, Clone, Deserialize)]
pub struct MarginMarket {
    pub ticker: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub bid: Option<String>,
    #[serde(default)]
    pub ask: Option<String>,
    #[serde(default)]
    pub price: Option<String>,
    pub contract_size: String,
    pub fractional_trading_enabled: bool,
    /// Can be the EMPTY STRING (demo BTC, finding 12).
    pub tick_size: String,
    #[serde(default)]
    pub leverage_estimate: Option<f64>,
    /// Keyed by notional-dollar tier ("1000", "10000", ...) — the recorded
    /// risk curve the gate/sim mm_curve is derived from.
    #[serde(default)]
    pub leverage_estimates: BTreeMap<String, f64>,
    #[serde(default)]
    pub liquidation_mark_price: Option<MarkPriceStamp>,
    #[serde(default)]
    pub reference_price: Option<MarkPriceStamp>,
    #[serde(default)]
    pub settlement_mark_price: Option<MarkPriceStamp>,
    #[serde(default)]
    pub open_interest: Option<String>,
    #[serde(default)]
    pub volume: Option<String>,
    #[serde(default)]
    pub volume_24h: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MarketResponse {
    pub market: MarginMarket,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MarketsResponse {
    pub markets: Vec<MarginMarket>,
    #[serde(default)]
    pub cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Orderbook (finding 1: worst->best both sides; sort defensively)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct OrderbookBody {
    pub bids: Vec<(String, String)>,
    pub asks: Vec<(String, String)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderbookResponse {
    pub orderbook: OrderbookBody,
}

impl OrderbookBody {
    /// Best bid = MAX bid price, defensively (the recorded ordering is
    /// worst->best with best at array END, but never assume).
    pub fn best_bid(&self) -> Result<Option<(PerpPrice, Contracts)>, VenueError> {
        best_level(&self.bids, true)
    }

    /// Best ask = MIN ask price, defensively.
    pub fn best_ask(&self) -> Result<Option<(PerpPrice, Contracts)>, VenueError> {
        best_level(&self.asks, false)
    }
}

fn best_level(
    levels: &[(String, String)],
    max_side: bool,
) -> Result<Option<(PerpPrice, Contracts)>, VenueError> {
    let mut best: Option<(PerpPrice, Contracts)> = None;
    for (price_raw, count_raw) in levels {
        let price = parse_perp_price(price_raw)?;
        let count = parse_whole_count(count_raw)?;
        let better = match &best {
            None => true,
            Some((current, _)) => {
                if max_side {
                    price > *current
                } else {
                    price < *current
                }
            }
        };
        if better {
            best = Some((price, count));
        }
    }
    Ok(best)
}

// ---------------------------------------------------------------------------
// Orders
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct Order {
    pub order_id: String,
    pub client_order_id: String,
    pub user_id: String,
    pub ticker: String,
    /// "bid" / "ask" (margin book side — NOT the event API's yes/no).
    pub side: String,
    pub price: String,
    pub fill_count: String,
    pub remaining_count: String,
    /// "user" | "system" (system = venue-originated, e.g. liquidation).
    #[serde(default)]
    pub order_source: Option<String>,
    pub self_trade_prevention_type: String,
    pub created_time: String,
    pub last_update_time: String,
    pub last_update_reason: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderResponse {
    pub order: Order,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrdersResponse {
    pub cursor: String,
    pub orders: Vec<Order>,
}

/// POST /margin/orders response.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateOrderResponse {
    pub order_id: String,
    pub client_order_id: String,
    pub fill_count: String,
    pub remaining_count: String,
}

/// Amend (price) response: just the order id.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderIdResponse {
    pub order_id: String,
}

/// Amend (decrease via amend) response.
#[derive(Debug, Clone, Deserialize)]
pub struct AmendDecreaseResponse {
    pub order_id: String,
    pub fill_count: String,
    pub remaining_count: String,
}

/// Decrease response.
#[derive(Debug, Clone, Deserialize)]
pub struct DecreaseOrderResponse {
    pub order_id: String,
    pub remaining_count: String,
}

/// Cancel response.
#[derive(Debug, Clone, Deserialize)]
pub struct CancelOrderResponse {
    pub order_id: String,
    pub reduced_by: String,
}

// ---------------------------------------------------------------------------
// Fills / positions / balance / risk
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct Fill {
    pub fill_id: String,
    pub order_id: String,
    pub ticker: String,
    /// "bid" / "ask".
    pub side: String,
    pub price: String,
    pub count: String,
    pub entry_price: String,
    pub fees: String,
    pub realized_pnl: String,
    pub is_taker: bool,
    /// "user" | "system" — system fills are the liquidation class
    /// (spec 5.15: dedicated lifecycle, never silently absorbed).
    pub order_source: String,
    pub created_time: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FillsResponse {
    pub cursor: String,
    pub fills: Vec<Fill>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Position {
    pub market_ticker: String,
    /// SIGNED count string (shorts negative).
    pub position: String,
    pub entry_price: String,
    pub fees: String,
    pub margin_used: String,
    pub unrealized_pnl: String,
    pub roe: f64,
    pub subaccount: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PositionsResponse {
    pub positions: Vec<Position>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubaccountBalance {
    pub subaccount: i64,
    pub account_equity: String,
    pub available_balance: String,
    pub initial_margin: String,
    pub maintenance_margin: String,
    pub position_value: String,
    pub resting_orders_margin: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BalanceResponse {
    pub settled_funds: String,
    pub subaccount_balances: Vec<SubaccountBalance>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RiskAccountPosition {
    pub market_ticker: String,
    pub position: String,
    pub mark_price: String,
    pub maintenance_margin_required: String,
    pub position_leverage: f64,
    pub position_notional: String,
    pub subaccount: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RiskAccountResponse {
    pub account_leverage: f64,
    pub positions: Vec<RiskAccountPosition>,
    pub total_maintenance_margin: String,
    pub total_position_notional: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RiskParametersResponse {
    pub initial_margin_multiplier: BTreeMap<String, f64>,
    pub liquidation_margin_ratio_threshold: f64,
    pub queue_entry_margin_ratio_threshold: f64,
}

/// Values uncaptured (empty map on demo) — kept as raw JSON until a
/// populated capture exists.
#[derive(Debug, Clone, Deserialize)]
pub struct NotionalRiskLimitResponse {
    pub default_notional_value_risk_limit: String,
    pub notional_value_risk_limits_by_market_ticker: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateBucket {
    pub bucket_capacity: i64,
    pub refill_rate: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountLimitsResponse {
    pub usage_tier: String,
    pub grants: Vec<serde_json::Value>,
    pub read: RateBucket,
    pub write: RateBucket,
}

// ---------------------------------------------------------------------------
// Funding / fees / status / enablement
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct FundingRateHistoricalEntry {
    pub market_ticker: String,
    /// JSON NUMBER, never a string (finding 10).
    pub funding_rate: f64,
    pub funding_time: String,
    pub mark_price: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FundingRatesHistoricalResponse {
    pub funding_rates: Vec<FundingRateHistoricalEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FundingEstimateResponse {
    pub market_ticker: String,
    pub funding_rate: f64,
    pub mark_price: String,
    pub computed_time: String,
    pub next_funding_time: String,
}

/// Entry shape UNCAPTURED (item 10 partial: demo funding rate was 0, a
/// zero payment posts no entry). Raw values until a populated capture or
/// the PROD parity sweep provides the shape — never invented.
#[derive(Debug, Clone, Deserialize)]
pub struct FundingHistoryResponse {
    pub funding_history: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeeTiersResponse {
    pub maker_fee_rates: BTreeMap<String, f64>,
    pub taker_fee_rates: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExchangeStatusResponse {
    pub exchange_active: bool,
    pub trading_active: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MarginEnabledResponse {
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Groups / subaccounts / transfers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct GroupCreateResponse {
    pub order_group_id: String,
    pub subaccount: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupGetResponse {
    pub contracts_limit_fp: String,
    pub is_auto_cancel_enabled: bool,
    pub orders: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupSummary {
    pub id: String,
    pub contracts_limit_fp: String,
    pub is_auto_cancel_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupsListResponse {
    /// ABSENT on an empty account (recorded: `{}` in the re-recorded
    /// corpus) — same tolerance pattern as MarketsResponse.cursor.
    #[serde(default)]
    pub order_groups: Vec<GroupSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubaccountCreateResponse {
    pub subaccount_number: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransferResponse {
    pub transfer_id: String,
}

/// `{}` responses (group trigger/reset/update/delete, subaccount
/// transfers). deny_unknown_fields pins emptiness: a payload growing
/// fields would surface here instead of vanishing.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyResponse {}

// ---------------------------------------------------------------------------
// WS frames (dispatch on `type`; unknown preserved, never guessed)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct WsSubscribedMsg {
    pub channel: String,
    pub sid: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WsSnapshotMsg {
    pub market_ticker: String,
    pub bid: Vec<(String, String)>,
    pub ask: Vec<(String, String)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WsDeltaMsg {
    pub market_ticker: String,
    pub price: String,
    pub delta: String,
    pub side: String,
    pub ts_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WsTickerFundingRate {
    pub rate: f64,
    pub next_funding_time_ms: i64,
    pub ts_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WsTickerMsg {
    pub market_ticker: String,
    pub price: String,
    pub bid: String,
    pub ask: String,
    pub bid_size_fp: String,
    pub ask_size_fp: String,
    pub funding_rate: WsTickerFundingRate,
    pub reference_price: MarkPriceStamp,
    pub settlement_mark_price: MarkPriceStamp,
    pub liquidation_mark_price: MarkPriceStamp,
    pub ts_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WsTradeMsg {
    pub trade_id: String,
    pub market_ticker: String,
    pub price: String,
    pub count: String,
    pub taker_side: String,
    pub ts_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WsUserOrderMsg {
    pub order_id: String,
    pub user_id: String,
    pub client_order_id: String,
    pub ticker: String,
    pub side: String,
    pub price: String,
    pub fill_count: String,
    pub remaining_count: String,
    pub self_trade_prevention_type: String,
    #[serde(default)]
    pub order_source: Option<String>,
    pub subaccount_number: i64,
    pub created_ts_ms: i64,
    pub last_updated_ts_ms: i64,
}

/// CAPTURED in the re-recorded private stream (one live taker fill):
/// trade/order/client ids, market_ticker, side, price/count strings,
/// fee_cost, post_position (signed count AFTER the fill), order_source
/// ("user" | "system" — the WS surface of the 5.15 liquidation class).
#[derive(Debug, Clone, Deserialize)]
pub struct WsFillMsg {
    pub trade_id: String,
    pub order_id: String,
    pub client_order_id: String,
    pub market_ticker: String,
    pub is_taker: bool,
    pub side: String,
    pub price: String,
    pub count: String,
    pub fee_cost: String,
    pub post_position: String,
    pub subaccount: i64,
    pub order_source: String,
    pub ts_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WsGroupUpdateMsg {
    pub event_type: String,
    pub order_group_id: String,
    /// Absent on "triggered" events (recorded); present on "created".
    #[serde(default)]
    pub contracts_limit_fp: Option<String>,
    pub ts_ms: i64,
}

/// One parsed WS text frame.
#[derive(Debug, Clone)]
pub enum WsFrame {
    Subscribed {
        id: Option<i64>,
        msg: WsSubscribedMsg,
    },
    OrderbookSnapshot {
        sid: i64,
        seq: i64,
        msg: WsSnapshotMsg,
    },
    OrderbookDelta {
        sid: i64,
        seq: i64,
        msg: WsDeltaMsg,
    },
    Ticker {
        sid: i64,
        msg: WsTickerMsg,
    },
    Trade {
        sid: i64,
        seq: i64,
        msg: WsTradeMsg,
    },
    UserOrder {
        sid: i64,
        msg: WsUserOrderMsg,
    },
    Fill {
        sid: i64,
        msg: WsFillMsg,
    },
    OrderGroupUpdate {
        sid: i64,
        seq: Option<i64>,
        msg: WsGroupUpdateMsg,
    },
    /// A type this build does not know. Preserved verbatim for the
    /// caller; never guessed at.
    Unknown(serde_json::Value),
}

fn field_i64(v: &serde_json::Value, key: &str, frame: &str) -> Result<i64, VenueError> {
    v.get(key)
        .and_then(|x| x.as_i64())
        .ok_or_else(|| VenueError::Invalid {
            reason: format!("kinetics ws {frame} frame missing integer {key}"),
        })
}

fn field_msg<T: serde::de::DeserializeOwned>(
    v: &serde_json::Value,
    frame: &str,
) -> Result<T, VenueError> {
    let msg = v.get("msg").ok_or_else(|| VenueError::Invalid {
        reason: format!("kinetics ws {frame} frame missing msg"),
    })?;
    serde_json::from_value(msg.clone()).map_err(|e| VenueError::Invalid {
        reason: format!("kinetics ws {frame} msg: {e}"),
    })
}

/// Parse one WS text frame. Empty lines (the recorded streams carry
/// trailing-newline blanks) are the CALLER's concern; this expects JSON.
pub fn parse_ws_frame(raw: &str) -> Result<WsFrame, VenueError> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| VenueError::Invalid {
        reason: format!("kinetics ws frame is not JSON: {e}"),
    })?;
    let kind = v
        .get("type")
        .and_then(|t| t.as_str())
        .ok_or_else(|| VenueError::Invalid {
            reason: "kinetics ws frame missing type".into(),
        })?;
    Ok(match kind {
        "subscribed" => WsFrame::Subscribed {
            id: v.get("id").and_then(|x| x.as_i64()),
            msg: field_msg(&v, "subscribed")?,
        },
        "orderbook_snapshot" => WsFrame::OrderbookSnapshot {
            sid: field_i64(&v, "sid", "orderbook_snapshot")?,
            seq: field_i64(&v, "seq", "orderbook_snapshot")?,
            msg: field_msg(&v, "orderbook_snapshot")?,
        },
        "orderbook_delta" => WsFrame::OrderbookDelta {
            sid: field_i64(&v, "sid", "orderbook_delta")?,
            seq: field_i64(&v, "seq", "orderbook_delta")?,
            msg: field_msg(&v, "orderbook_delta")?,
        },
        "ticker" => WsFrame::Ticker {
            sid: field_i64(&v, "sid", "ticker")?,
            msg: field_msg(&v, "ticker")?,
        },
        "trade" => WsFrame::Trade {
            sid: field_i64(&v, "sid", "trade")?,
            seq: field_i64(&v, "seq", "trade")?,
            msg: field_msg(&v, "trade")?,
        },
        "user_order" => WsFrame::UserOrder {
            sid: field_i64(&v, "sid", "user_order")?,
            msg: field_msg(&v, "user_order")?,
        },
        "fill" => WsFrame::Fill {
            sid: field_i64(&v, "sid", "fill")?,
            msg: field_msg(&v, "fill")?,
        },
        "order_group_updates" => WsFrame::OrderGroupUpdate {
            sid: field_i64(&v, "sid", "order_group_updates")?,
            seq: v.get("seq").and_then(|x| x.as_i64()),
            msg: field_msg(&v, "order_group_updates")?,
        },
        _ => WsFrame::Unknown(v),
    })
}
