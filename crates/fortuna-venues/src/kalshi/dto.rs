//! Serde DTOs for the Kalshi Trade API v2 REST surface, plus the
//! direction-mapping and fixed-point parsing primitives. Every shape is
//! transcribed from the archived OpenAPI spec v3.21.0
//! (docs/research/venue/kalshi-api-2026-06-10/raw/openapi.yaml) and the
//! research doc; nothing is invented.
//!
//! Reading rules baked in here:
//!
//! - Prices/fees are fixed-point DOLLAR STRINGS (`FixedPointDollars`),
//!   quantities are fixed-point COUNT STRINGS (`FixedPointCount`). They are
//!   parsed with `rust_decimal::Decimal::from_str`; money goes through
//!   `Cents::from_dollars_exact` (sub-cent remainder on a linear_cent
//!   market = hard error) and fees through a ceil (rounding against us).
//! - Direction is read from `outcome_side`/`book_side` ONLY. The legacy
//!   `side`/`action` fields are past their deprecation window (research
//!   "Deprecation watch") and are not even deserialized.
//! - Unknown enum values degrade to an explicit `Unknown` variant (mapped
//!   conservatively by the adapter) instead of failing the whole response.

use crate::VenueError;
use fortuna_core::market::{Action, Contracts, Side};
use fortuna_core::money::Cents;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

// ---------------------------------------------------------------------------
// Fixed-point parsing / formatting (the integer-cent boundary)
// ---------------------------------------------------------------------------

fn parse_decimal(raw: &str, what: &str) -> Result<Decimal, VenueError> {
    Decimal::from_str(raw).map_err(|e| VenueError::Invalid {
        reason: format!("kalshi {what} {raw:?} is not a decimal string: {e}"),
    })
}

/// Parse a `FixedPointDollars` string into exact integer cents. A sub-cent
/// remainder is a HARD error: the integer-cent core only trades
/// `linear_cent` markets, where every valid price is a whole cent.
pub fn parse_dollars_to_cents_exact(raw: &str) -> Result<Cents, VenueError> {
    let d = parse_decimal(raw, "dollar amount")?;
    Cents::from_dollars_exact(d).map_err(|e| VenueError::Invalid {
        reason: format!("kalshi dollar amount {raw:?} not representable as integer cents: {e}"),
    })
}

/// Parse a `FixedPointDollars` fee into cents, rounding UP (against us).
/// Per-fill fees are assessed at $0.0001 granularity (docs fee_rounding
/// page), so charged fees can carry sub-cent digits; the conservative
/// integer-cent view never understates them.
pub fn parse_fee_dollars_ceil(raw: &str) -> Result<Cents, VenueError> {
    let d = parse_decimal(raw, "fee amount")?;
    Cents::from_dollars_ceil(d).map_err(|e| VenueError::Invalid {
        reason: format!("kalshi fee amount {raw:?} out of range: {e}"),
    })
}

/// Parse a `FixedPointCount` string into a whole contract count. Fractional
/// contracts (granularity 0.01 per the spec) cannot be represented by the
/// integer core and are a HARD error; FORTUNA never sends fractional counts.
pub fn parse_count_integral(raw: &str) -> Result<Contracts, VenueError> {
    let d = parse_decimal(raw, "contract count")?;
    if !d.fract().is_zero() {
        return Err(VenueError::Invalid {
            reason: format!(
                "kalshi contract count {raw:?} is fractional; the integer-cent core cannot \
                 represent it"
            ),
        });
    }
    let n = d.to_i64().ok_or_else(|| VenueError::Invalid {
        reason: format!("kalshi contract count {raw:?} out of i64 range"),
    })?;
    Ok(Contracts::new(n))
}

/// Parse a `FixedPointCount` VOLUME into whole contracts, rounding UP.
/// Volume can legitimately be fractional (0.01-contract granularity,
/// fractional_trading_enabled is always true); ceil over-states it, which
/// keeps sub-volume market filters (mech_extremes' $100k cap) conservative
/// — a market is never admitted because rounding made it look smaller.
pub fn parse_count_ceil(raw: &str) -> Result<i64, VenueError> {
    let d = parse_decimal(raw, "contract count")?;
    if d.is_sign_negative() {
        return Err(VenueError::Invalid {
            reason: format!("kalshi volume {raw:?} is negative"),
        });
    }
    d.ceil().to_i64().ok_or_else(|| VenueError::Invalid {
        reason: format!("kalshi volume {raw:?} out of i64 range"),
    })
}

/// Format cents as the 4-decimal dollar string the doc examples use
/// (`56c -> "0.5600"`).
pub fn format_price_dollars(price: Cents) -> Result<String, VenueError> {
    if price.raw() < 0 {
        return Err(VenueError::Invalid {
            reason: format!("cannot format negative price {price} for kalshi"),
        });
    }
    Ok(format!("{:.4}", price.to_dollars()))
}

/// Format a contract count as the 2-decimal string the doc examples use
/// (`10 -> "10.00"`; requests accept 0-2 decimals, responses emit 2).
pub fn format_count(qty: Contracts) -> Result<String, VenueError> {
    if qty.raw() < 0 {
        return Err(VenueError::Invalid {
            reason: format!("cannot format negative contract count {qty} for kalshi"),
        });
    }
    Ok(format!("{}.00", qty.raw()))
}

// ---------------------------------------------------------------------------
// Direction mapping (research §5a + order-direction page, doc-verbatim)
// ---------------------------------------------------------------------------

/// Kalshi `BookSide`: "For event markets, this refers to the YES leg only:
/// `bid` means buy YES, `ask` means sell YES."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiBookSide {
    Bid,
    Ask,
}

/// Kalshi `outcome_side`: "the outcome side this order/fill positioned the
/// user for. buy-yes and sell-no produce 'yes'; buy-no and sell-yes produce
/// 'no'."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiOutcomeSide {
    Yes,
    No,
}

/// Map our (side, action, own-side limit price) onto Kalshi's V2 order
/// model: a `BookSide` plus the YES-leg price.
///
/// Equivalence table (order-direction page, doc-verbatim):
/// buy+yes -> bid, sell+no -> bid, buy+no -> ask, sell+yes -> ask; and "this
/// endpoint quotes everything from the YES side", so NO-space prices mirror
/// to `100 - p`.
pub fn to_book_side(
    side: Side,
    action: Action,
    limit_own_side: Cents,
) -> Result<(KalshiBookSide, Cents), VenueError> {
    let yes_price = match side {
        Side::Yes => limit_own_side,
        Side::No => Cents::new(100)
            .checked_sub(limit_own_side)
            .map_err(VenueError::Money)?,
    };
    let book_side = match (side, action) {
        (Side::Yes, Action::Buy) | (Side::No, Action::Sell) => KalshiBookSide::Bid,
        (Side::Yes, Action::Sell) | (Side::No, Action::Buy) => KalshiBookSide::Ask,
    };
    Ok((book_side, yes_price))
}

/// Map Kalshi's canonical direction pair back into our vocabulary.
///
/// Kalshi's V2 model deliberately collapses buy-yes/sell-no (and
/// buy-no/sell-yes): the original `action` is NOT recoverable from a V2
/// order or fill. We canonicalize to `Action::Buy` of the outcome side,
/// which is venue-truthful under Kalshi's net-position accounting (a "sell
/// no" IS a "buy yes" there). The pair must be self-consistent ("bid == yes,
/// ask == no, always"); disagreement means we are misreading the venue and
/// is a hard error.
pub fn from_direction(
    outcome: KalshiOutcomeSide,
    book: KalshiBookSide,
) -> Result<(Side, Action), VenueError> {
    match (outcome, book) {
        (KalshiOutcomeSide::Yes, KalshiBookSide::Bid) => Ok((Side::Yes, Action::Buy)),
        (KalshiOutcomeSide::No, KalshiBookSide::Ask) => Ok((Side::No, Action::Buy)),
        (o, b) => Err(VenueError::Invalid {
            reason: format!(
                "kalshi direction fields disagree (outcome_side={o:?}, book_side={b:?}); \
                 the docs state bid==yes / ask==no always"
            ),
        }),
    }
}

// ---------------------------------------------------------------------------
// Enums with conservative Unknown fallbacks
// ---------------------------------------------------------------------------

/// Market lifecycle status (RESPONSE vocabulary — differs from the query
/// filter vocabulary; fixture checklist #21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiMarketStatus {
    Initialized,
    Inactive,
    Active,
    Closed,
    Determined,
    Disputed,
    Amended,
    Finalized,
    /// Any value not in the archived spec enum. Mapped to a non-tradeable
    /// status by the adapter (conservative).
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiOrderStatus {
    Resting,
    Canceled,
    Executed,
    #[serde(other)]
    Unknown,
}

/// Series fee structure. `flat` is defined in the API enum but used by ZERO
/// live series and its semantics are officially ambiguous (fees research
/// uncertainty #1): the adapter refuses to model it rather than guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiFeeType {
    Quadratic,
    QuadraticWithMakerFees,
    Flat,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiTimeInForce {
    FillOrKill,
    GoodTillCanceled,
    ImmediateOrCancel,
}

/// `self_trade_prevention_type` (required on V2 create).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiStp {
    TakerAtCross,
    Maker,
}

// ---------------------------------------------------------------------------
// Market / series / event
// ---------------------------------------------------------------------------

/// Subset of the OpenAPI `Market` schema the adapter consumes. Extra fields
/// in responses are ignored by serde. All title/rules text is UNTRUSTED
/// external data (spec 5.11): carried as data, never interpreted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalshiMarket {
    pub ticker: String,
    pub event_ticker: String,
    /// `binary` | `scalar`. Kept as a string: the adapter trades binaries
    /// only and filters on equality.
    pub market_type: String,
    /// Deprecated in the spec but still served; display-only fallback chain
    /// title -> yes_sub_title.
    #[serde(default)]
    pub title: Option<String>,
    pub yes_sub_title: String,
    pub no_sub_title: String,
    pub status: KalshiMarketStatus,
    pub close_time: String,
    pub settlement_timer_seconds: i64,
    pub notional_value_dollars: String,
    /// `linear_cent` | `deci_cent` | `tapered_deci_cent` (research §3).
    pub price_level_structure: String,
    /// Lifetime contracts traded (`FixedPointCount`). Required in the
    /// OpenAPI schema, kept optional here so its absence degrades to
    /// unknown-volume (volume-capped strategies skip) instead of failing
    /// the whole catalog page.
    #[serde(default)]
    pub volume_fp: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub settlement_value_dollars: Option<String>,
    #[serde(default)]
    pub settlement_ts: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetMarketsResponse {
    pub markets: Vec<KalshiMarket>,
    pub cursor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetMarketResponse {
    pub market: KalshiMarket,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalshiSettlementSource {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// Subset of the OpenAPI `Series` schema. Fee fields live HERE, not on the
/// market (research §3: "Fee-relevant fields live on the SERIES").
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KalshiSeries {
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub fee_type: KalshiFeeType,
    /// JSON number in the spec ("floating point multiplier applied to the
    /// fee calculations"). Converted to `Decimal` at the adapter boundary;
    /// observed live values are exactly 0, 0.5, 1.
    pub fee_multiplier: f64,
    #[serde(default)]
    pub settlement_sources: Option<Vec<KalshiSettlementSource>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GetSeriesResponse {
    pub series: KalshiSeries,
}

/// Subset of `EventData`: the documented market -> series link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalshiEvent {
    pub event_ticker: String,
    pub series_ticker: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetEventResponse {
    pub event: KalshiEvent,
}

// ---------------------------------------------------------------------------
// Orderbook
// ---------------------------------------------------------------------------

/// `OrderbookCountFp`: yes/no BID arrays only ("no asks are returned"),
/// each level a 2-element `[dollars_string, count_fp]` array where "the
/// second element is the contract quantity (not price)". `no_dollars`
/// levels are quoted in NO-leg pricing (fixture checklist #20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalshiOrderbook {
    pub yes_dollars: Vec<[String; 2]>,
    pub no_dollars: Vec<[String; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetOrderbookResponse {
    pub orderbook_fp: KalshiOrderbook,
}

// ---------------------------------------------------------------------------
// Orders (V2 create/cancel + Order object for GET surfaces)
// ---------------------------------------------------------------------------

/// `CreateOrderV2Request` — POST /portfolio/events/orders (research §5a).
/// Field set and serialization match the archived doc example exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateOrderV2Request {
    pub ticker: String,
    pub client_order_id: String,
    pub side: KalshiBookSide,
    /// FixedPointCount string, e.g. "10.00".
    pub count: String,
    /// FixedPointDollars string on the YES leg, e.g. "0.5600".
    pub price: String,
    pub time_in_force: KalshiTimeInForce,
    pub self_trade_prevention_type: KalshiStp,
    pub post_only: bool,
    pub cancel_order_on_pause: bool,
    pub reduce_only: bool,
    pub subaccount: i64,
    pub exchange_index: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateOrderV2Response {
    pub order_id: String,
    #[serde(default)]
    pub client_order_id: Option<String>,
    pub fill_count: String,
    pub remaining_count: String,
    /// "Only present when fill_count > 0."
    #[serde(default)]
    pub average_fill_price: Option<String>,
    #[serde(default)]
    pub average_fee_paid: Option<String>,
    pub ts_ms: i64,
}

/// DELETE /portfolio/events/orders/{order_id} response. ADVISORY ONLY: per
/// the May 21, 2026 changelog the body "may not correspond to your request";
/// the adapter reconciles via GET instead of trusting it (research §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelOrderV2Response {
    pub order_id: String,
    #[serde(default)]
    pub client_order_id: Option<String>,
    pub reduced_by: String,
    pub ts_ms: i64,
}

/// Subset of the OpenAPI `Order` schema. Direction is read from
/// `outcome_side`/`book_side` ONLY (legacy `side`/`action` are past their
/// announced removal window and are not deserialized).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalshiOrder {
    pub order_id: String,
    pub client_order_id: String,
    pub ticker: String,
    pub outcome_side: KalshiOutcomeSide,
    pub book_side: KalshiBookSide,
    pub status: KalshiOrderStatus,
    pub yes_price_dollars: String,
    pub no_price_dollars: String,
    pub fill_count_fp: String,
    pub remaining_count_fp: String,
    pub initial_count_fp: String,
    pub taker_fees_dollars: String,
    pub maker_fees_dollars: String,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub last_update_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetOrdersResponse {
    pub orders: Vec<KalshiOrder>,
    pub cursor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetOrderResponse {
    pub order: KalshiOrder,
}

// ---------------------------------------------------------------------------
// Fills
// ---------------------------------------------------------------------------

/// Subset of the OpenAPI `Fill` schema. NOTE: fills carry NO
/// client_order_id (removed 2026-01-28 per the changelog); the adapter
/// resolves it via the order id. `fee_cost` is a fixed-point DOLLARS string
/// (research §7), unlike Settlement's integer-cent `revenue`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalshiFill {
    pub fill_id: String,
    pub order_id: String,
    pub ticker: String,
    pub outcome_side: KalshiOutcomeSide,
    pub book_side: KalshiBookSide,
    pub count_fp: String,
    pub yes_price_dollars: String,
    pub no_price_dollars: String,
    pub is_taker: bool,
    pub fee_cost: String,
    #[serde(default)]
    pub created_time: Option<String>,
    /// Unix seconds, "legacy field name".
    #[serde(default)]
    pub ts: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetFillsResponse {
    pub fills: Vec<KalshiFill>,
    pub cursor: String,
}

// ---------------------------------------------------------------------------
// Balance / positions / settlements
// ---------------------------------------------------------------------------

/// GET /portfolio/balance. `balance` is INTEGER CENTS ("Member's available
/// balance in cents"); `balance_dollars` may carry sub-cent precision for
/// direct members. The adapter uses the truncating integer field: it never
/// overstates available cash (conservative).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetBalanceResponse {
    pub balance: i64,
    pub balance_dollars: String,
    pub portfolio_value: i64,
    pub updated_ts: i64,
}

/// Subset of `MarketPosition`. `position_fp` is SIGNED: "Negative means NO
/// contracts and positive means YES contracts" — Kalshi nets the two sides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalshiMarketPosition {
    pub ticker: String,
    pub position_fp: String,
    pub market_exposure_dollars: String,
    pub realized_pnl_dollars: String,
    pub fees_paid_dollars: String,
    pub total_traded_dollars: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetPositionsResponse {
    #[serde(default)]
    pub cursor: Option<String>,
    pub market_positions: Vec<KalshiMarketPosition>,
}

/// Subset of `Settlement`. UNITS MIX (documented, fixture checklist #19):
/// `revenue`/`value` are integer CENTS; `fee_cost` and the cost fields are
/// dollar STRINGS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalshiSettlement {
    pub ticker: String,
    pub event_ticker: String,
    pub market_result: String,
    pub yes_count_fp: String,
    pub yes_total_cost_dollars: String,
    pub no_count_fp: String,
    pub no_total_cost_dollars: String,
    pub revenue: i64,
    pub settled_time: String,
    pub fee_cost: String,
    #[serde(default)]
    pub value: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetSettlementsResponse {
    pub settlements: Vec<KalshiSettlement>,
    #[serde(default)]
    pub cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Error envelopes
// ---------------------------------------------------------------------------

/// OpenAPI `ErrorResponse` — all fields optional in practice because the
/// docs publish the SHAPE but not the value catalog (fixture checklist #3,
/// #7, #8), and the 429 body uses a different shape entirely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalshiErrorBody {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    /// The `error` field is polymorphic on the wire: the rate-limit page sends a
    /// STRING (`{"error": "too many requests"}`), while 17/19 recorded 4xx bodies
    /// nest an OBJECT (`{"error":{"code","message","service"}}`, fixture finding
    /// F1). Held as a Value so `error_reason` can unpack BOTH forms.
    #[serde(default)]
    pub error: Option<serde_json::Value>,
}

/// Render whatever error body the venue sent into a diagnostic string.
/// Tolerates the `ErrorResponse` shape, the 429 `{"error": ...}` shape, and
/// anything else (raw JSON, truncated). Never fails: error paths must not
/// produce secondary errors.
pub fn error_reason(body: &serde_json::Value) -> String {
    if let Ok(e) = serde_json::from_value::<KalshiErrorBody>(body.clone()) {
        let mut parts: Vec<String> = Vec::new();
        if let Some(code) = e.code {
            parts.push(format!("code={code}"));
        }
        if let Some(message) = e.message {
            parts.push(message);
        }
        if let Some(details) = e.details {
            parts.push(details);
        }
        match e.error {
            // 429 shape: {"error": "too many requests"}.
            Some(serde_json::Value::String(s)) => parts.push(s),
            // Nested shape (F1, 17/19 4xx): {"error":{"code","message","details"}}
            // — extract the same fields the flat shape exposes at the top level so
            // the venue's code/message reach diagnostics structured, not as raw JSON.
            Some(serde_json::Value::Object(obj)) => {
                if let Some(code) = obj.get("code").and_then(|v| v.as_str()) {
                    parts.push(format!("code={code}"));
                }
                if let Some(message) = obj.get("message").and_then(|v| v.as_str()) {
                    parts.push(message.to_string());
                }
                if let Some(details) = obj.get("details").and_then(|v| v.as_str()) {
                    parts.push(details.to_string());
                }
            }
            _ => {}
        }
        if !parts.is_empty() {
            return parts.join("; ");
        }
    }
    let mut raw = body.to_string();
    if raw.len() > 300 {
        raw.truncate(300);
    }
    if raw.is_empty() || raw == "null" {
        "venue sent no error body".to_string()
    } else {
        raw
    }
}
