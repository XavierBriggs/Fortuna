//! Typed REST client for the Kinetics margin surface (T5.B4 slice 2).
//!
//! Rides the SAME transport/signing machinery as the event API
//! (`KalshiTransport` / `KalshiSigner` — same RSA-PSS recipe, same hosts,
//! paths under `/margin/*`; fixture item 1 confirmed the recipe is
//! accepted on the margin surface). "Own credentials" (spec 5.15) means a
//! SEPARATE KEY PAIR at composition time, not a different algorithm: the
//! kinetics client is constructed with its own signer-backed transport.
//!
//! Every request this client issues is FIXTURES-GATED: the test suite
//! replays each method against the recorded `.meta.json` request
//! (method/path/query/body must match the capture byte-for-byte as JSON).
//! Request shapes are transcribed from the recordings; nothing invented:
//!
//! - Order prices are 4dp dollar strings; counts are PLAIN integer
//!   strings ("1", not "1.00") — exactly as recorded.
//! - `post_only` / `reduce_only` / `order_group_id` are OMITTED when
//!   unset (the IOC recording carries no post_only field at all).
//! - `GET /margin/funding_history` REQUIRES start_date + end_date
//!   (fixture finding 6 — undocumented in the OpenAPI spec).
//! - `POST /portfolio/margin/subaccounts` always sends `{}` (finding 7:
//!   a body-less POST is rejected with invalid_content_type).
//!
//! Error mapping: status >= 400 parses the three-shape
//! `KineticsApiError`; `not_found` maps to `VenueError::NotFound`,
//! everything else to `VenueError::Rejected` with the RAW code preserved
//! at the front of the reason (codes are dynamic, finding 8 — the
//! adapter prefix-matches them; it never keys on HTTP status).

use crate::kalshi::client::KalshiTransport;
use crate::kinetics::dto;
use crate::VenueError;
use fortuna_core::perp::PerpPrice;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::sync::Arc;

/// Margin order book side (NOT the event API's yes/no).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookSide {
    Bid,
    Ask,
}

impl BookSide {
    pub fn as_str(self) -> &'static str {
        match self {
            BookSide::Bid => "bid",
            BookSide::Ask => "ask",
        }
    }
}

/// Time in force, venue vocabulary (recorded: good_till_canceled,
/// immediate_or_cancel; fill_or_kill per the OpenAPI spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    GoodTillCanceled,
    ImmediateOrCancel,
    FillOrKill,
}

impl TimeInForce {
    pub fn as_str(self) -> &'static str {
        match self {
            TimeInForce::GoodTillCanceled => "good_till_canceled",
            TimeInForce::ImmediateOrCancel => "immediate_or_cancel",
            TimeInForce::FillOrKill => "fill_or_kill",
        }
    }
}

/// One order creation request, shaped exactly as recorded.
#[derive(Debug, Clone)]
pub struct CreateOrderRequest {
    pub ticker: String,
    pub side: BookSide,
    pub price: PerpPrice,
    pub count: i64,
    pub client_order_id: String,
    pub time_in_force: TimeInForce,
    /// Omitted from the body when None (the IOC recording has no
    /// post_only field).
    pub post_only: Option<bool>,
    /// Omitted when None; the venue requires IOC/FOK with reduce_only.
    pub reduce_only: Option<bool>,
    pub order_group_id: Option<String>,
}

/// Render a `PerpPrice` as the venue's 4dp dollar string ("5.3829").
pub fn price_string(price: PerpPrice) -> String {
    Decimal::new(price.raw(), 4).to_string()
}

/// Typed margin-surface client over the shared signed transport.
pub struct KineticsClient {
    transport: Arc<dyn KalshiTransport>,
}

impl KineticsClient {
    /// The transport must be constructed with the KINETICS credential
    /// pair (own key id + PEM via env), not the event-API pair.
    pub fn new(transport: Arc<dyn KalshiTransport>) -> Self {
        KineticsClient { transport }
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: Option<&str>,
    ) -> Result<T, VenueError> {
        let (status, body) = self.transport.request("GET", path, query, None).await?;
        decode(status, body, "GET", path)
    }

    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, VenueError> {
        let (status, resp) = self.transport.request(method, path, None, body).await?;
        decode(status, resp, method, path)
    }

    // ---- status / enablement / balance ----

    pub async fn exchange_status(&self) -> Result<dto::ExchangeStatusResponse, VenueError> {
        self.get("/margin/exchange/status", None).await
    }

    pub async fn margin_enabled(&self) -> Result<dto::MarginEnabledResponse, VenueError> {
        self.get("/margin/enabled", None).await
    }

    pub async fn balance(
        &self,
        compute_available: bool,
    ) -> Result<dto::BalanceResponse, VenueError> {
        let query = compute_available.then_some("compute_available_balance=true");
        self.get("/margin/balance", query).await
    }

    pub async fn account_limits_perps(&self) -> Result<dto::AccountLimitsResponse, VenueError> {
        self.get("/account/limits/perps", None).await
    }

    // ---- markets / books ----

    pub async fn markets(&self) -> Result<dto::MarketsResponse, VenueError> {
        self.get("/margin/markets", None).await
    }

    pub async fn market(&self, ticker: &str) -> Result<dto::MarketResponse, VenueError> {
        self.get(&format!("/margin/markets/{ticker}"), None).await
    }

    /// `depth=0` returns the full book (recorded); aggregation buckets are
    /// display-only (finding 17) and label by floor — never executable.
    pub async fn orderbook(
        &self,
        ticker: &str,
        depth: i64,
        aggregation_tick_size: Option<&str>,
    ) -> Result<dto::OrderbookResponse, VenueError> {
        let query = match aggregation_tick_size {
            Some(agg) => format!("depth={depth}&aggregation_tick_size={agg}"),
            None => format!("depth={depth}"),
        };
        self.get(&format!("/margin/markets/{ticker}/orderbook"), Some(&query))
            .await
    }

    // ---- orders ----

    pub async fn create_order(
        &self,
        req: &CreateOrderRequest,
    ) -> Result<dto::CreateOrderResponse, VenueError> {
        let mut body = json!({
            "ticker": req.ticker,
            "side": req.side.as_str(),
            "price": price_string(req.price),
            "count": req.count.to_string(),
            "client_order_id": req.client_order_id,
            "time_in_force": req.time_in_force.as_str(),
            "self_trade_prevention_type": "taker_at_cross",
        });
        if let (Some(map), Some(post_only)) = (body.as_object_mut(), req.post_only) {
            map.insert("post_only".into(), Value::Bool(post_only));
        }
        if let (Some(map), Some(reduce_only)) = (body.as_object_mut(), req.reduce_only) {
            map.insert("reduce_only".into(), Value::Bool(reduce_only));
        }
        if let (Some(map), Some(group)) = (body.as_object_mut(), req.order_group_id.as_ref()) {
            map.insert("order_group_id".into(), Value::String(group.clone()));
        }
        self.send("POST", "/margin/orders", Some(body)).await
    }

    pub async fn get_order(&self, order_id: &str) -> Result<dto::OrderResponse, VenueError> {
        self.get(&format!("/margin/orders/{order_id}"), None).await
    }

    /// Status filter caveat (item 14): garbage status values are NOT
    /// rejected by the venue; the vocabulary is unconfirmed. Callers
    /// must treat filtered lists as advisory until a populated capture
    /// confirms the filter binds.
    pub async fn list_orders(
        &self,
        status: Option<&str>,
        limit: Option<i64>,
    ) -> Result<dto::OrdersResponse, VenueError> {
        let mut parts = Vec::new();
        if let Some(s) = status {
            parts.push(format!("status={s}"));
        }
        if let Some(l) = limit {
            parts.push(format!("limit={l}"));
        }
        let query = (!parts.is_empty()).then(|| parts.join("&"));
        self.get("/margin/orders", query.as_deref()).await
    }

    /// Amend price/size in place (recorded body: count/price/side/ticker).
    pub async fn amend_order(
        &self,
        order_id: &str,
        ticker: &str,
        side: BookSide,
        price: PerpPrice,
        count: i64,
    ) -> Result<dto::OrderIdResponse, VenueError> {
        let body = json!({
            "ticker": ticker,
            "side": side.as_str(),
            "price": price_string(price),
            "count": count.to_string(),
        });
        self.send(
            "POST",
            &format!("/margin/orders/{order_id}/amend"),
            Some(body),
        )
        .await
    }

    pub async fn decrease_order(
        &self,
        order_id: &str,
        reduce_by: i64,
    ) -> Result<dto::DecreaseOrderResponse, VenueError> {
        let body = json!({ "reduce_by": reduce_by.to_string() });
        self.send(
            "POST",
            &format!("/margin/orders/{order_id}/decrease"),
            Some(body),
        )
        .await
    }

    pub async fn cancel_order(
        &self,
        order_id: &str,
    ) -> Result<dto::CancelOrderResponse, VenueError> {
        self.send("DELETE", &format!("/margin/orders/{order_id}"), None)
            .await
    }

    // ---- fills / positions / risk ----

    pub async fn fills(
        &self,
        ticker: Option<&str>,
        limit: Option<i64>,
    ) -> Result<dto::FillsResponse, VenueError> {
        let mut parts = Vec::new();
        if let Some(t) = ticker {
            parts.push(format!("ticker={t}"));
        }
        if let Some(l) = limit {
            parts.push(format!("limit={l}"));
        }
        let query = (!parts.is_empty()).then(|| parts.join("&"));
        self.get("/margin/fills", query.as_deref()).await
    }

    pub async fn positions(&self) -> Result<dto::PositionsResponse, VenueError> {
        self.get("/margin/positions", None).await
    }

    pub async fn risk_account(&self) -> Result<dto::RiskAccountResponse, VenueError> {
        self.get("/margin/risk", None).await
    }

    pub async fn risk_parameters(&self) -> Result<dto::RiskParametersResponse, VenueError> {
        self.get("/margin/risk_parameters", None).await
    }

    pub async fn notional_risk_limit(&self) -> Result<dto::NotionalRiskLimitResponse, VenueError> {
        self.get("/margin/notional_risk_limit", None).await
    }

    pub async fn fee_tiers(&self) -> Result<dto::FeeTiersResponse, VenueError> {
        self.get("/margin/fee_tiers", None).await
    }

    // ---- funding ----

    pub async fn funding_estimate(
        &self,
        ticker: &str,
    ) -> Result<dto::FundingEstimateResponse, VenueError> {
        self.get(
            "/margin/funding_rates/estimate",
            Some(&format!("ticker={ticker}")),
        )
        .await
    }

    pub async fn funding_rates_historical(
        &self,
        ticker: Option<&str>,
        limit: Option<i64>,
    ) -> Result<dto::FundingRatesHistoricalResponse, VenueError> {
        let mut parts = Vec::new();
        if let Some(t) = ticker {
            parts.push(format!("ticker={t}"));
        }
        if let Some(l) = limit {
            parts.push(format!("limit={l}"));
        }
        let query = (!parts.is_empty()).then(|| parts.join("&"));
        self.get("/margin/funding_rates/historical", query.as_deref())
            .await
    }

    /// BOTH dates are REQUIRED (finding 6, undocumented): `YYYY-MM-DD`.
    pub async fn funding_history(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<dto::FundingHistoryResponse, VenueError> {
        self.get(
            "/margin/funding_history",
            Some(&format!("start_date={start_date}&end_date={end_date}")),
        )
        .await
    }

    // ---- order groups ----

    pub async fn create_group(
        &self,
        contracts_limit: i64,
    ) -> Result<dto::GroupCreateResponse, VenueError> {
        self.send(
            "POST",
            "/margin/order_groups/create",
            Some(json!({ "contracts_limit": contracts_limit })),
        )
        .await
    }

    pub async fn get_group(&self, group_id: &str) -> Result<dto::GroupGetResponse, VenueError> {
        self.get(&format!("/margin/order_groups/{group_id}"), None)
            .await
    }

    pub async fn list_groups(&self) -> Result<dto::GroupsListResponse, VenueError> {
        self.get("/margin/order_groups", None).await
    }

    pub async fn update_group_limit(
        &self,
        group_id: &str,
        contracts_limit: i64,
    ) -> Result<dto::EmptyResponse, VenueError> {
        self.send(
            "PUT",
            &format!("/margin/order_groups/{group_id}/limit"),
            Some(json!({ "contracts_limit": contracts_limit })),
        )
        .await
    }

    /// Trigger/reset send `{}` — exactly as recorded (the JSON content
    /// type matters on this surface; finding 7's lesson generalizes).
    pub async fn trigger_group(&self, group_id: &str) -> Result<dto::EmptyResponse, VenueError> {
        self.send(
            "PUT",
            &format!("/margin/order_groups/{group_id}/trigger"),
            Some(json!({})),
        )
        .await
    }

    pub async fn reset_group(&self, group_id: &str) -> Result<dto::EmptyResponse, VenueError> {
        self.send(
            "PUT",
            &format!("/margin/order_groups/{group_id}/reset"),
            Some(json!({})),
        )
        .await
    }

    pub async fn delete_group(&self, group_id: &str) -> Result<dto::EmptyResponse, VenueError> {
        self.send("DELETE", &format!("/margin/order_groups/{group_id}"), None)
            .await
    }

    // ---- subaccounts / transfers ----

    /// Always sends `{}` (finding 7: body-less POST is rejected with
    /// invalid_content_type even though the OpenAPI spec declares no body).
    pub async fn create_subaccount(&self) -> Result<dto::SubaccountCreateResponse, VenueError> {
        self.send("POST", "/portfolio/margin/subaccounts", Some(json!({})))
            .await
    }

    pub async fn subaccount_transfer(
        &self,
        client_transfer_id: &str,
        from_subaccount: i64,
        to_subaccount: i64,
        amount_cents: i64,
    ) -> Result<dto::EmptyResponse, VenueError> {
        let body = json!({
            "client_transfer_id": client_transfer_id,
            "from_subaccount": from_subaccount,
            "to_subaccount": to_subaccount,
            "amount_cents": amount_cents,
        });
        self.send("POST", "/portfolio/margin/subaccounts/transfer", Some(body))
            .await
    }

    /// The event-contract <-> margin funding rail (live on demo; used by
    /// the recorder to fund the margin account). `amount` is integer
    /// CENTICENTS per the recorded KINETICS_FUND_CENTICENTS naming.
    pub async fn intra_exchange_transfer(
        &self,
        source: &str,
        destination: &str,
        amount: i64,
    ) -> Result<dto::TransferResponse, VenueError> {
        let body = json!({
            "source": source,
            "destination": destination,
            "amount": amount,
        });
        self.send(
            "POST",
            "/portfolio/intra_exchange_instance_transfer",
            Some(body),
        )
        .await
    }
}

/// Decode a `(status, body)` pair: 2xx parses the typed DTO; anything
/// else parses the three-shape error envelope and maps it.
fn decode<T: serde::de::DeserializeOwned>(
    status: u16,
    body: Value,
    method: &str,
    path: &str,
) -> Result<T, VenueError> {
    if (200..300).contains(&status) {
        return serde_json::from_value(body).map_err(|e| VenueError::Invalid {
            reason: format!("kinetics {method} {path}: response did not parse: {e}"),
        });
    }
    match serde_json::from_value::<dto::KineticsApiError>(body.clone()) {
        Ok(err) => {
            if err.code_matches("not_found") {
                return Err(VenueError::NotFound {
                    what: format!("kinetics {method} {path}: {}", err.raw_code()),
                });
            }
            // RAW CODE FIRST: codes are dynamic (finding 8); callers
            // prefix-match the front of the reason, never the status.
            Err(VenueError::Rejected {
                reason: format!(
                    "{} (kinetics {method} {path} status {status})",
                    err.raw_code()
                ),
            })
        }
        Err(_) => Err(VenueError::Invalid {
            reason: format!(
                "kinetics {method} {path}: status {status} with unparseable error body: {body}"
            ),
        }),
    }
}
