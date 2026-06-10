# Kalshi Trading API v2 — Venue Research (retrieved 2026-06-10)

Scope: REST + WebSocket surface needed for the FORTUNA Kalshi adapter (spec 5.x venue
adapter, Phase 1 = REST polling). All shapes below are taken verbatim from Kalshi's
official documentation and the official OpenAPI/AsyncAPI specs published at
docs.kalshi.com. Nothing here is invented; where the docs are silent the gap is listed
in the Uncertainties section. Per repo rules, no behavior is trusted for live use until
an operator-recorded fixture confirms it.

Spec snapshot used: **OpenAPI `3.21.0`** ("Kalshi Trade API Manual Endpoints"),
downloaded 2026-06-10 from `https://docs.kalshi.com/openapi.yaml`, archived in this
directory at `raw/openapi.yaml`. WebSocket: **AsyncAPI 2.0.0** ("Kalshi Market Data
WebSocket API") from `https://docs.kalshi.com/asyncapi.yaml`, archived at
`raw/asyncapi.yaml`. All cited docs pages were fetched 2026-06-10 as the Mintlify `.md`
renders (append `.md` to any page URL) and archived under `raw/pages/`.

---

## Sources

All retrieved 2026-06-10.

| # | URL | Used for |
|---|-----|----------|
| S1 | https://docs.kalshi.com/openapi.yaml | Authoritative REST schemas (v3.21.0) |
| S2 | https://docs.kalshi.com/asyncapi.yaml | Authoritative WS schemas (v2.0.0) |
| S3 | https://docs.kalshi.com/getting_started/api_keys | Auth: headers, signing payload, RSA-PSS code |
| S4 | https://docs.kalshi.com/getting_started/quick_start_authenticated_requests | Auth: signing walkthrough, header table |
| S5 | https://docs.kalshi.com/getting_started/api_environments | Base URLs (REST + WS, prod + demo) |
| S6 | https://docs.kalshi.com/getting_started/demo_env | Demo environment |
| S7 | https://docs.kalshi.com/getting_started/rate_limits | Token buckets, tiers, 429 body |
| S8 | https://docs.kalshi.com/getting_started/fixed_point_migration | `_dollars` / `_fp` representation, price level structures |
| S9 | https://docs.kalshi.com/getting_started/order_direction | `outcome_side` / `book_side` migration |
| S10 | https://docs.kalshi.com/getting_started/quick_start_create_order | V2 order example, client_order_id dedupe, error list |
| S11 | https://docs.kalshi.com/getting_started/quick_start_websockets | WS auth, subscribe examples, WS error table |
| S12 | https://docs.kalshi.com/api-reference/orders/create-order | Legacy POST /portfolio/orders |
| S13 | https://docs.kalshi.com/api-reference/orders/create-order-v2 | POST /portfolio/events/orders (canonical) |
| S14 | https://docs.kalshi.com/api-reference/orders/cancel-order | Legacy DELETE /portfolio/orders/{order_id} |
| S15 | https://docs.kalshi.com/api-reference/orders/cancel-order-v2 | DELETE /portfolio/events/orders/{order_id} |
| S16 | https://docs.kalshi.com/api-reference/orders/get-orders | GET /portfolio/orders |
| S17 | https://docs.kalshi.com/api-reference/market/get-markets | GET /markets |
| S18 | https://docs.kalshi.com/api-reference/market/get-market | GET /markets/{ticker} |
| S19 | https://docs.kalshi.com/api-reference/market/get-market-orderbook | GET /markets/{ticker}/orderbook |
| S20 | https://docs.kalshi.com/api-reference/market/get-series | Series schema (fee_type, fee_multiplier) |
| S21 | https://docs.kalshi.com/api-reference/portfolio/get-balance | GET /portfolio/balance |
| S22 | https://docs.kalshi.com/api-reference/portfolio/get-fills | GET /portfolio/fills |
| S23 | https://docs.kalshi.com/api-reference/portfolio/get-positions | GET /portfolio/positions |
| S24 | https://docs.kalshi.com/api-reference/portfolio/get-settlements | GET /portfolio/settlements |
| S25 | https://docs.kalshi.com/websockets/websocket-connection | WS commands, responses, error codes |
| S26 | https://docs.kalshi.com/websockets/orderbook-updates | orderbook_snapshot / orderbook_delta |
| S27 | https://docs.kalshi.com/websockets/market-ticker | ticker channel |
| S28 | https://docs.kalshi.com/websockets/user-fills | fill channel |
| S29 | https://docs.kalshi.com/websockets/connection-keep-alive | ping/pong heartbeat |
| S30 | https://docs.kalshi.com/changelog/index | Deprecations, cost changes, migration dates |
| S31 | https://docs.kalshi.com/getting_started/historical_data | Historical cutoff (3-month live window) |
| S32 | https://docs.kalshi.com/api-reference/api-keys/generate-api-key | POST /api_keys/generate |
| S33 | https://docs.kalshi.com/api-reference/account/get-account-api-limits | GET /account/limits |
| S34 | https://docs.kalshi.com/llms.txt | Full docs index |
| S35 | https://docs.kalshi.com/getting_started/fee_rounding | Fee rounding / balance precision |

Access note: docs.kalshi.com served all pages and both spec files to plain `curl` with
no blocking on 2026-06-10. The `.md` suffix returns clean markdown for any docs page.

---

## 1. Authentication

**Source: S3, S4, S1 (securitySchemes).**

Current scheme is **API-key + RSA signature per request**. Three required headers on
every authenticated request (S4, verbatim table):

| Header | Description | Example |
| --- | --- | --- |
| `KALSHI-ACCESS-KEY` | Your API Key ID | `a952bcbe-ec3b-4b5b-b8f9-11dae589608c` |
| `KALSHI-ACCESS-TIMESTAMP` | Current time in milliseconds | `1703123456789` |
| `KALSHI-ACCESS-SIGNATURE` | Request signature (see below) | `base64_encoded_signature` |

OpenAPI securitySchemes (S1, verbatim):

```yaml
kalshiAccessKey:    { type: apiKey, in: header, name: KALSHI-ACCESS-KEY,       description: "Your API key ID" }
kalshiAccessSignature: { type: apiKey, in: header, name: KALSHI-ACCESS-SIGNATURE, description: "RSA-PSS signature of the request" }
kalshiAccessTimestamp: { type: apiKey, in: header, name: KALSHI-ACCESS-TIMESTAMP, description: "Request timestamp in milliseconds" }
```

### Signing payload

Message string = `timestamp + HTTP_METHOD + path` — exactly that concatenation, no
separators (S4): example `1703123456789GET/trade-api/v2/portfolio/balance`.

- `timestamp`: Unix epoch **milliseconds**, as a decimal string; the same string goes
  in `KALSHI-ACCESS-TIMESTAMP`.
- `METHOD`: uppercase HTTP method (`GET`, `POST`, `DELETE`, ...).
- `path`: the **full URL path from the API root including the `/trade-api/v2` prefix**,
  **without query parameters**. Verbatim warning (S3): "When signing requests, use the
  path **without query parameters**. For example, if your request is to
  `/trade-api/v2/portfolio/orders?limit=5`, sign only `/trade-api/v2/portfolio/orders`
  (strip the `?` and everything after it)."
- The host never enters the signature: "The host does not change the signature
  payload. Sign the full request path from the API root, without query parameters."
  (S5).

### Algorithm

**RSA-PSS** (not PKCS#1 v1.5) over the UTF-8 message bytes:

- Hash: **SHA-256**
- MGF: **MGF1 with SHA-256**
- Salt length: **equal to digest length** (32 bytes) — Python reference uses
  `padding.PSS(mgf=padding.MGF1(hashes.SHA256()), salt_length=padding.PSS.DIGEST_LENGTH)`;
  JS reference uses `crypto.constants.RSA_PKCS1_PSS_PADDING` with
  `saltLength: crypto.constants.RSA_PSS_SALTLEN_DIGEST` (S3, both verbatim from sample code)
- Output encoding: **standard base64** (`base64.b64encode(...)`) into
  `KALSHI-ACCESS-SIGNATURE`.

Rust mapping: `rsa` crate `Pss::new::<Sha256>()` (salt len = digest len) or ring's
`RSA_PSS_SHA256`, then `base64::engine::general_purpose::STANDARD`.

### Timestamp skew tolerance

**Not documented anywhere** in the current docs, spec, or changelog (searched all
fetched pages for skew/tolerance/window/expired). Troubleshooting table only says
"Signature error → Ensure timestamp is in milliseconds (not seconds)" (S4). Treat the
allowed clock skew as unknown → fixture-confirm (see Uncertainties).

### Key acquisition and scopes

- UI: log in → Account & security → API Keys → Create Key; private key is downloaded
  as a `.key` PEM file once and never retrievable again; Key ID shown on screen (S3, S4).
  Same process for demo and production (S3: "This process is the same for the demo or
  production environment").
- API: `POST /trade-api/v2/api_keys/generate` body `{"name": "...", "scopes": [...]}` →
  response `{"api_key_id": "...", "private_key": "<PEM>"}` (S1, S32).
  `POST /api_keys` (bring-your-own public key) is restricted to "Premier or Market
  Maker API usage levels" (S34 index description).
- API-key scopes exist: `read`, `write`, and child scopes such as `write::transfer`
  (S1 `GenerateApiKeyRequest.scopes`; changelog June 2, 2026). Default when omitted:
  full access (`read`, `write`).

### Old email/password session auth

The previous `POST /login` / member-session auth **does not exist anywhere in the
current docs, OpenAPI spec, or changelog** — there are no login/password/session
endpoints in `openapi.yaml` (S1) or the docs index (S34); the API-keys docs describe
keys as the way to access the API "without requiring username/password authentication".
Treat it as removed. Do not build it.

---

## 2. Base URLs

**Source: S5 (verbatim tables), confirmed by S1 `servers:` block.**

REST:

| Environment | Recommended base URL | Also supported |
| --- | --- | --- |
| Production | `https://external-api.kalshi.com/trade-api/v2` | `https://api.elections.kalshi.com/trade-api/v2` |
| Demo | `https://external-api.demo.kalshi.co/trade-api/v2` | `https://demo-api.kalshi.co/trade-api/v2` |

WebSocket:

| Environment | Recommended URL | Also supported |
| --- | --- | --- |
| Production | `wss://external-api-ws.kalshi.com/trade-api/ws/v2` | `wss://api.elections.kalshi.com/trade-api/ws/v2` |
| Demo | `wss://external-api-ws.demo.kalshi.co/trade-api/ws/v2` | `wss://demo-api.kalshi.co/trade-api/ws/v2` |

Notes (S5): "Despite the `elections` subdomain, the production Trade API provides
access to all Kalshi markets." The `external-api` hosts are "dedicated to the external
Trade API and are the recommended hosts for API traders" (added to docs May 7, 2026 per
changelog S30). Credentials are environment-specific: demo keys only work on demo (S5).

---

## 3. GET /markets and GET /markets/{ticker}

**Source: S1 (paths `/markets`, `/markets/{ticker}`; schemas `Market`,
`GetMarketsResponse`, `GetMarketResponse`), S17, S18.**

### GET /trade-api/v2/markets — query params (S1, verbatim descriptions)

| Param | Type | Notes |
| --- | --- | --- |
| `limit` | int64, 1–1000, default 100 | "Number of results per page. Defaults to 100. Maximum value is 1000." |
| `cursor` | string | "Use the cursor value returned from the previous response to get the next page. Leave empty for the first page." |
| `event_ticker` | string | "Only a single event ticker is supported." |
| `series_ticker` | string | Filter by series ticker |
| `min_created_ts` / `max_created_ts` | int64 Unix s | creation-time window |
| `min_updated_ts` | int64 Unix s | "Incompatible with all filters besides `mve_filter=exclude`" |
| `min_close_ts` / `max_close_ts` | int64 Unix s | close-time window |
| `min_settled_ts` / `max_settled_ts` | int64 Unix s | settled-time window |
| `status` | string enum | **`unopened`, `open`, `paused`, `closed`, `settled`** (query-filter vocabulary; only one at a time) |
| `tickers` | string | "Comma-separated list of market tickers to retrieve." |
| `mve_filter` | enum `only` \| `exclude` | multivariate (combo) filter |

Endpoint description constraints (S1): only one `status` filter at a time; timestamp
filters are mutually exclusive with each other and with certain statuses
(`min/max_created_ts` ↔ `unopened`/`open`/empty; `min/max_close_ts` ↔ `closed`/empty;
`min/max_settled_ts` ↔ `settled`/empty). Markets settled before the historical cutoff
move to `GET /historical/markets` — the live window target is 3 months (S31).

Response: `{"markets": [Market, ...], "cursor": "..."}` — both fields required (S1).

Auth: `GET /markets` and `GET /markets/{ticker}` carry **no security requirement in
the spec**, and the quick start fetches them with plain unauthenticated
`requests.get(...)` (S10). Single market: `GET /trade-api/v2/markets/{ticker}` →
`{"market": Market}`; 404 if unknown.

### Market object (S1 schema `Market` — full field list)

Required fields: `ticker`, `event_ticker`, `market_type`, `yes_sub_title`,
`no_sub_title`, `created_time`, `updated_time`, `open_time`, `close_time`,
`latest_expiration_time`, `settlement_timer_seconds`, `status`,
`notional_value_dollars`, `yes_bid_dollars`, `yes_ask_dollars`, `no_bid_dollars`,
`no_ask_dollars`, `yes_bid_size_fp`, `yes_ask_size_fp`, `last_price_dollars`,
`previous_yes_bid_dollars`, `previous_yes_ask_dollars`, `previous_price_dollars`,
`volume_fp`, `volume_24h_fp`, `liquidity_dollars`, `open_interest_fp`, `result`,
`can_close_early`, `fractional_trading_enabled`, `expiration_value`, `rules_primary`,
`rules_secondary`, `price_level_structure`, `price_ranges`.

Key fields:

- `market_type`: enum `binary` | `scalar`.
- `status` (RESPONSE lifecycle enum — different vocabulary from the query filter):
  **`initialized`, `inactive`, `active`, `closed`, `determined`, `disputed`,
  `amended`, `finalized`**.
- **Prices are now fixed-point dollar strings** (`FixedPointDollars`: "US dollar amount
  as a fixed-point decimal string with up to 6 decimal places of precision",
  example `"0.5600"`). Quantity fields are `FixedPointCount` strings ("2 decimals,
  e.g., \"10.00\" ... requests accept 0–2 decimal places; responses always emit 2
  decimals; minimum granularity is 0.01 contracts").
- `price_level_structure` (string) + `price_ranges` (array of
  `{start, end, step}` dollar strings) define valid tick sizes. Documented structures
  (S8, verbatim table):

  | Structure | Ranges | Tick Size |
  | --- | --- | --- |
  | `linear_cent` | $0.00 – $1.00 | $0.01 |
  | `tapered_deci_cent` | $0.00 – $0.10 / $0.10 – $0.90 / $0.90 – $1.00 | $0.001 / $0.01 / $0.001 |
  | `deci_cent` | $0.00 – $1.00 | $0.001 |

  The old `tick_size` field was removed from Market on **May 7, 2026** (S30):
  "Use `price_level_structure` and `price_ranges[].step`".
- Settlement fields: `result` (enum `yes`,`no`,`scalar`,`""`),
  `settlement_value_dollars` (nullable, "Only filled after determination"),
  `settlement_ts` (nullable date-time, "Only filled for settled markets"),
  `expiration_value` (string, "value that was considered for the settlement"),
  `settlement_timer_seconds` (int), `occurrence_datetime` (nullable),
  `early_close_condition` (nullable), `can_close_early` (bool).
- Strike fields: `strike_type` enum (`greater`, `greater_or_equal`, `less`,
  `less_or_equal`, `between`, `functional`, `custom`, `structured`), `floor_strike` /
  `cap_strike` (double, nullable), `functional_strike`, `custom_strike`.
- Deprecated-but-present: `title`, `subtitle`, `expiration_time`,
  `response_price_units` (enum `usd_cent`, "DEPRECATED: Use price_level_structure and
  price_ranges instead"), `liquidity_dollars` ("always returns \"0.0000\""),
  `fractional_trading_enabled` ("always `true`").
- MVE fields: `mve_collection_ticker`, `mve_selected_legs`, `primary_participant_key`,
  `is_provisional`, `exchange_index`.

### Fee-relevant fields live on the SERIES, not the market

`GET /trade-api/v2/series/{series_ticker}` → `Series` (S1, S20). Required fields
include **`fee_type`** and **`fee_multiplier`**:

- `fee_type`: enum **`quadratic`, `quadratic_with_maker_fees`, `flat`** — "Fee
  structures can be found at https://kalshi.com/docs/kalshi-fee-schedule.pdf.
  'quadratic' is described by the General Trading Fees Table,
  'quadratic_with_maker_fees' ... with maker fees described in the Maker Fees section,
  'flat' is described by the Specific Trading Fees Table." (S1, verbatim)
- `fee_multiplier`: double — "floating point multiplier applied to the fee
  calculations."

Scheduled fee changes: `GET /series/fee_changes` → `{"series_fee_change_arr":
[{id, series_ticker, fee_type, fee_multiplier, scheduled_ts}]}` and
`GET /events/fee_changes` → `{"event_fee_changes": [{id, event_ticker, series_ticker,
fee_type_override (nullable), fee_multiplier_override (nullable), scheduled_ts}],
"cursor"}` (S1). Fee rounding mechanics: S35.

---

## 4. GET /markets/{ticker}/orderbook

**Source: S1 (path + schemas `GetMarketOrderbookResponse`, `OrderbookCountFp`,
`PriceLevelDollarsCountFp`), S19.**

`GET /trade-api/v2/markets/{ticker}/orderbook` — **requires auth** (security listed in
spec, unlike GET /markets).

Param: `depth` — int, 0–100, default 0; "Depth of the orderbook to retrieve (0 or
negative means all levels, 1-100 for specific depth)".

Semantics (S1/S19 description, verbatim): "The order book shows all active bid orders
for both yes and no sides of a binary market. It returns yes bids and no bids only (no
asks are returned). This is because in binary markets, a bid for yes at price X is
equivalent to an ask for no at price (100-X). For example, a yes bid at 7¢ is the same
as a no ask at 93¢, with identical contract sizes."

Response shape:

```json
{
  "orderbook_fp": {
    "yes_dollars": [["0.1500", "100.00"], ...],
    "no_dollars":  [["0.1500", "100.00"], ...]
  }
}
```

Each level is `PriceLevelDollarsCountFp`: a 2-element array
`[dollars_string, fp]` — "where dollars_string is like \"0.1500\" and fp is a
FixedPointCount string (fixed-point contract count). The second element is the
contract quantity (not price)." Both `yes_dollars` and `no_dollars` are required.
Levels are "organized from best to worst prices" (S19).

Batch variant: `GET /trade-api/v2/markets/orderbooks` →
`{"orderbooks": [{"ticker": "...", "orderbook_fp": {...}}, ...]}` (S1;
https://docs.kalshi.com/api-reference/market/get-multiple-market-orderbooks).

Note: the old integer-cent `orderbook.yes`/`orderbook.no` arrays are gone from the
spec; only the `_fp`/dollars form is documented.

---

## 5. POST order (create)

Two generations coexist. **Build the adapter against V2**; record legacy only for
reference. Changelog (S30): "The legacy `/portfolio/orders` endpoint will be
deprecated no earlier than May 6, 2026 — clients should migrate to this path" (we are
past that date) and as of June 4, 2026 legacy order-mutation rate-limit costs are
**10x** the V2 costs (legacy create = 100 tokens vs V2 = default 10).

### 5a. V2 (canonical): POST /trade-api/v2/portfolio/events/orders

**Source: S13, S1 (`CreateOrderV2Request/Response`).** Status 201 on success.

Required: `ticker`, `side`, `count`, `price`, `time_in_force`,
`self_trade_prevention_type`.

Request example (S13, verbatim):

```json
{
  "ticker": "HIGHNY-24JAN01-T60",
  "client_order_id": "8c35ecb3-328f-4f52-8c7c-0f4b9862f8d1",
  "side": "bid",
  "count": "10.00",
  "price": "0.5600",
  "time_in_force": "good_till_canceled",
  "self_trade_prevention_type": "taker_at_cross",
  "post_only": false,
  "cancel_order_on_pause": false,
  "reduce_only": false,
  "subaccount": 0,
  "exchange_index": 0
}
```

Field semantics (S1/S13):

- `side`: `BookSide` enum **`bid` | `ask`** — "For event markets, this refers to the
  YES leg only: `bid` means buy YES, `ask` means sell YES. (Selling YES is economically
  equivalent to buying NO at `1 - price`, but this endpoint quotes everything from the
  YES side.)"
- `count`: FixedPointCount string. `price`: FixedPointDollars string (yes-leg price).
- `time_in_force`: enum **`fill_or_kill` | `good_till_canceled` |
  `immediate_or_cancel`** (required). "Use `good_till_canceled` with `expiration_time`
  for an order that should rest until a specific expiration time; without
  `expiration_time`, `good_till_canceled` is a true good-till-canceled order. `GTT` is
  not a valid API value." `immediate_or_cancel` cannot be combined with
  `expiration_time`.
- `expiration_time`: optional int64 **Unix seconds** expiry (note: legacy endpoint
  calls this `expiration_ts`).
- `self_trade_prevention_type` (required): enum `taker_at_cross` | `maker` —
  "`taker_at_cross` cancels the taker order when it would trade against another order
  from the same user; execution stops and any partial fills already matched are
  executed. `maker` cancels the resting maker order and continues matching."
- `post_only`: bool. (Post-only orders that would cross are canceled; order updates
  report `last_update_reason = PostOnlyCrossCancel` since June 4, 2026 — S30.)
- `reduce_only`: bool — "whether the order place count should be capped by the
  member's current position."
- `client_order_id`: optional string, dedupe key (see below).
- `order_group_id`, `cancel_order_on_pause`, `subaccount` (0 = primary, 1–63),
  `exchange_index` (currently only 0).
- **No `type` field and no `buy_max_cost` in V2.** Order type `market` was deprecated
  Sep 25, 2025 ("only `limit` type orders will be supported") and `type` was removed
  from the legacy create request Feb 11, 2026 (S30). Marketable-limit emulation per
  S30: `{"yes_price": 99, "side": "yes"}` etc.

Response 201 example (S13, verbatim):

```json
{
  "order_id": "3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d",
  "client_order_id": "8c35ecb3-328f-4f52-8c7c-0f4b9862f8d1",
  "fill_count": "0.00",
  "remaining_count": "10.00",
  "ts_ms": 1715793600123
}
```

Required response fields: `order_id`, `fill_count`, `remaining_count`, `ts_ms`.
Optional: `client_order_id`, `average_fill_price` ("Only present when fill_count >
0"), `average_fee_paid` (same). `ts_ms` = "Matching engine timestamp at which the
order was processed, as Unix epoch milliseconds." For IOC, `remaining_count`
"reflects the final state after unfilled contracts are canceled."

Errors declared: 400, 401, 409, 429, 500 → all `ErrorResponse` (below). Each user is
limited to **200,000 open orders** (S12 description on legacy; presumed shared).

### 5b. Legacy: POST /trade-api/v2/portfolio/orders

**Source: S12, S1 (`CreateOrderRequest`).** 201 → `{"order": Order}`. Rate-limit note
on page: "**Rate limit:** 100 tokens per request."

Required: `ticker`, `side` (`yes`|`no`), `action` (`buy`|`sell`). Optional: `count`
(int ≥ 1) or `count_fp` (string; if both, must match), `yes_price`/`no_price`
(int cents 1–99) or `yes_price_dollars`/`no_price_dollars` (strings), `expiration_ts`
(int64 Unix s), `time_in_force` (same enum, optional here), `buy_max_cost`
(int — "Maximum cost in cents. When specified, the order will automatically have
Fill-or-Kill (FoK) behavior."), `post_only`, `reduce_only`, `sell_position_floor`
(deprecated, only 0), `self_trade_prevention_type`, `order_group_id`,
`cancel_order_on_pause`, `subaccount`, `exchange_index`. **There is no `type` field**
(removed Feb 11, 2026 — S30).

### Order object (returned by legacy create, GET orders, GET order — S1 `Order`)

Required: `order_id`, `user_id`, `client_order_id`, `ticker`, `side`, `action`,
`outcome_side`, `book_side`, `type`, `status`, `yes_price_dollars`,
`no_price_dollars`, `fill_count_fp`, `remaining_count_fp`, `initial_count_fp`,
`taker_fees_dollars`, `maker_fees_dollars`, `taker_fill_cost_dollars`,
`maker_fill_cost_dollars`.

- `status`: **`resting` | `canceled` | `executed`** (`OrderStatus`).
- `type`: `limit` | `market` (response-only vestige; creation is always limit).
- `side`/`action` are **deprecated** ("will not be removed before May 14, 2026") in
  favor of `outcome_side` (`yes`|`no`) and `book_side` (`bid`|`ask`); equivalence
  table in S9: buy+yes→yes/bid, sell+no→yes/bid, buy+no→no/ask, sell+yes→no/ask.
- Optional/nullable: `expiration_time`, `created_time`, `last_update_time`,
  `self_trade_prevention_type`, `order_group_id`, `cancel_order_on_pause`,
  `subaccount_number`, `exchange_index`.

### client_order_id idempotency (S10, verbatim bullets)

- "If network issues occur, you can resubmit with the same `client_order_id`"
- "The API will reject duplicate submissions with the same `client_order_id`,
  preventing accidental double orders"
- Error mapping: "`409 Conflict`: Order with this `client_order_id` already exists"

### Error body shape (S1 `ErrorResponse`, used by 400/401/403/404/409/429/500)

```json
{ "code": "string", "message": "string", "details": "string", "service": "string" }
```

("Error code" / "Human-readable error message" / "Additional details, if available" /
"name of the service that generated the error".) **The docs do not publish the
catalog of `code` string values** (e.g. for duplicate client_order_id or insufficient
balance) — fixture-confirm exact strings (see Uncertainties). Specific legacy error
codes like `order_already_exists` / `insufficient_balance` from the pre-2026 docs no
longer appear anywhere in current official docs.

---

## 6. DELETE order (cancel)

### V2 (canonical): DELETE /trade-api/v2/portfolio/events/orders/{order_id}

**Source: S15, S1.** Page note: "**Rate limit:** 2 tokens per request." Query params:
`subaccount` (default 0), `exchange_index`. 200 response (S1 example, verbatim):

```json
{
  "order_id": "3b23c1c7-f4ef-4f0d-8b9a-9e53c61f1a0d",
  "client_order_id": "8c35ecb3-328f-4f52-8c7c-0f4b9862f8d1",
  "reduced_by": "10.00",
  "ts_ms": 1715793660456
}
```

`reduced_by` = "Number of contracts that were canceled (i.e. the remaining count at
time of cancellation)." Errors: 401, 404, 500 (`ErrorResponse`).

Known caveat (changelog May 21, 2026, S30): "In certain uncommon cases, responses from
DELETE/amend on `/portfolio/events/orders/...` do not describe the order that was
cancelled or amended — the `order_id`, `client_order_id`, and quantity fields in the
response may not correspond to your request. The cancel or amend itself executes
correctly against the intended order; only the response body is affected." → the
adapter must treat the cancel response body as advisory and reconcile via GET order /
fills.

### Legacy: DELETE /trade-api/v2/portfolio/orders/{order_id}

**Source: S14, S1.** Description (verbatim): "we can't completely delete the order, as
it may be partially filled already. Instead, the DeleteOrder endpoint reduce the order
completely, essentially zeroing the remaining resting contracts on it. The zeroed
order is returned on the response payload as a form of validation."
200 → `{"order": Order, "reduced_by_fp": "<FixedPointCount>"}`. Errors 401/404/500.
Token cost: page note says 2, but the June 4 changelog raised legacy DELETE to 20 —
conflict, see Uncertainties.

Related (V2 family, same auth, for completeness): amend
`POST /portfolio/events/orders/{order_id}/amend`, decrease
`POST /portfolio/events/orders/{order_id}/decrease` (accepts `reduce_to` or
`reduce_by`, exactly one — S30 May 12), batch create/cancel
`POST|DELETE /portfolio/events/orders/batched`. Batch requests bill per item; "the
maximum batch size scales with your tier's write budget" (S34 descriptions).

---

## 7. GET fills

**Source: S1 (path `/portfolio/fills`, schemas `GetFillsResponse`, `Fill`), S22.**

`GET /trade-api/v2/portfolio/fills` — auth required. Query params: `ticker` (market
filter), `order_id`, `min_ts` / `max_ts` ("Filter items after/before this Unix
timestamp"), `limit` (1–1000, default 100), `cursor`, `subaccount` (omitted = all
subaccounts). Fills older than the historical cutoff only via `GET /historical/fills`
(S31: live window target 3 months).

Response: `{"fills": [Fill, ...], "cursor": "..."}` (both required). Cursor semantics
(S1, positions wording, same convention): "An empty value of this field indicates
there is no next page."

`Fill` required fields: `fill_id`, `trade_id` ("legacy field name, same as fill_id"),
`order_id`, `ticker`, `market_ticker` ("legacy field name, same as ticker"), `side`,
`action`, `outcome_side`, `book_side`, `count_fp`, `yes_price_dollars`,
`no_price_dollars`, `is_taker`, `fee_cost`.

- `is_taker`: "If true, this fill was a taker (removed liquidity from the order book)"
- `fee_cost`: **FixedPointDollars string** — "Fee cost in fixed-point dollars" (NOT
  cents)
- `count_fp`: FixedPointCount string
- `side`/`action` deprecated (same migration as Order; use `outcome_side`/`book_side`)
- Optional: `created_time` (date-time), `ts` (int64 Unix s, "legacy field name"),
  `subaccount_number`

---

## 8. GET orders (listing)

**Source: S1 (path `/portfolio/orders` get), S16.**

`GET /trade-api/v2/portfolio/orders` — auth required. Query params: `ticker`,
`event_ticker` ("Event tickers to filter by, as a comma-separated list (maximum 10)"),
`min_ts` / `max_ts`, `status` ("resting, canceled, or executed"), `limit` (1–1000,
default 100), `cursor`, `subaccount`. Orders canceled/executed before the historical
cutoff only via `GET /historical/orders`; "Resting orders will always be available
through this endpoint."

Response: `{"orders": [Order, ...], "cursor": "..."}` (both required; Order as in §5).

Single order: `GET /trade-api/v2/portfolio/orders/{order_id}` → `{"order": Order}`;
page note "**Rate limit:** 2 tokens per request"; 404 if unknown.

---

## 9. Portfolio balance + positions

### GET /trade-api/v2/portfolio/balance

**Source: S1 (`GetBalanceResponse`), S21.** "Endpoint for getting the balance and
portfolio value of a member. Both values are returned in cents." Query param:
`subaccount` (default 0).

Response (required: `balance`, `balance_dollars`, `portfolio_value`, `updated_ts`):

- `balance`: int64 — "Member's available balance **in cents**."
- `balance_dollars`: FixedPointDollars string (added May 28, 2026 — S30; direct-member
  balances align to centi-cent `$0.0001` precision; "The legacy `balance` field
  truncates any sub-cent amount, so use `balance_dollars` for exact values.")
- `portfolio_value`: int64 — "Member's portfolio value in cents. This is the current
  value of all positions held."
- `updated_ts`: int64 Unix — "last update to the balance."
- `balance_breakdown`: optional array of per-`exchange_index` balances.

### GET /trade-api/v2/portfolio/positions

**Source: S1 (`GetPositionsResponse`, `MarketPosition`, `EventPosition`), S23.**
Query params: `cursor`, `limit` (1–1000, default 100), `count_filter` ("Restricts the
positions to those with any of following fields with non-zero values, as a comma
separated list. The following values are accepted - position, total_traded"),
`ticker`, `event_ticker`, `subaccount` (default 0).

Response: `{"cursor": "...", "market_positions": [...], "event_positions": [...]}`
(arrays required). "An empty value of this field indicates there is no next page."

`MarketPosition` (required: ticker, total_traded_dollars, position_fp,
market_exposure_dollars, realized_pnl_dollars, resting_orders_count,
fees_paid_dollars, last_updated_ts):

- `position_fp`: FixedPointCount string — "number of contracts bought in this market.
  **Negative means NO contracts and positive means YES contracts**"
- `market_exposure_dollars`: "Cost of the aggregate market position in dollars"
- `total_traded_dollars`, `realized_pnl_dollars` ("Locked in profit and loss"),
  `fees_paid_dollars` — all FixedPointDollars strings (no cent fields remain)
- `resting_orders_count`: int32, "[DEPRECATED] Aggregate size of resting orders"
- `last_updated_ts`: date-time string

`EventPosition` (required): `event_ticker`, `total_cost_dollars`,
`total_cost_shares_fp`, `event_exposure_dollars`, `realized_pnl_dollars`,
`fees_paid_dollars`.

---

## 10. Settlements

**Source: S1 (path `/portfolio/settlements`, schema `Settlement`), S24.**

`GET /trade-api/v2/portfolio/settlements` — "member's settlements historical track."
Query params: `limit` (1–1000, default 100), `cursor`, `ticker`, `event_ticker`,
`min_ts`, `max_ts`, `subaccount`.

Response: `{"settlements": [Settlement, ...], "cursor": "..."}` (`settlements`
required).

`Settlement` (required: ticker, event_ticker, market_result, yes_count_fp,
yes_total_cost_dollars, no_count_fp, no_total_cost_dollars, revenue, settled_time,
fee_cost):

- `market_result`: enum `yes` | `no` | `scalar`
- `yes_count_fp` / `no_count_fp`: FixedPointCount strings — contracts owned at
  settlement
- `yes_total_cost_dollars` / `no_total_cost_dollars`: FixedPointDollars strings
- `revenue`: **integer cents** — "Total revenue earned from this settlement in cents
  (winning contracts pay out 100 cents each)."
- `value`: nullable **integer cents** — "Payout of a single yes contract in cents."
- `fee_cost`: FixedPointDollars string (example `"0.3400"`) — "Total fees paid in
  fixed point dollars."
- `settled_time`: date-time

Note the units mix (revenue/value in cents; fee_cost in dollar strings).

There is no push "settlement notice"; market settlement is visible via the
`market_lifecycle_v2` WS channel (determined/settled events, S25/S34) and the Market
fields `result` / `settlement_value_dollars` / `settlement_ts` (§3).

---

## 11. WebSocket API

**Source: S2, S25, S26, S27, S28, S29, S11.** (Phase 1 polls REST; this records the
contract.)

- Single connection at `wss://external-api-ws.kalshi.com/trade-api/ws/v2` (prod) /
  `wss://external-api-ws.demo.kalshi.co/trade-api/ws/v2` (demo).
- **Auth during the HTTP upgrade handshake** with the same three headers as REST; the
  signed message is `timestamp + "GET" + "/trade-api/ws/v2"` (S11, verbatim).
  "Authentication is required to establish the connection ... Some channels carry only
  public market data, but the connection itself still requires authentication." (S25)
- Channel groups (S11): private — `orderbook_delta`, `fill`, `market_positions`,
  `communications`, `order_group_updates` (and `user_orders`); public-data —
  `ticker`, `trade`, `market_lifecycle_v2`, `multivariate_market_lifecycle`,
  `multivariate`. Channel enum in subscribe schema (S2): `orderbook_delta`, `ticker`,
  `trade`, `fill`, `market_positions`, `market_lifecycle_v2`,
  `multivariate_market_lifecycle`, `multivariate`, `communications`,
  `order_group_updates`, `user_orders`, `cfbenchmarks_value`.
- Keep-alive (S29, verbatim): "Kalshi sends Ping frames (`0x9`) every 10 seconds with
  body `heartbeat` ... Clients should respond with Pong frames (`0xA`)."
- Session limit: "WebSocket connections per user are limited by usage tier. The
  default limit begins at 200" (S30, Sep 18, 2025).

### Commands (client → server). All carry client-chosen integer `id`.

Subscribe (S25 example, verbatim):

```json
{ "id": 1, "cmd": "subscribe",
  "params": { "channels": ["orderbook_delta"], "market_ticker": "CPI-22DEC-TN0.1" } }
```

`params`: `channels` (required, min 1), `market_ticker` XOR `market_tickers` (pattern
`^[A-Z0-9-]+$`), or `market_id(s)` (UUIDs, ticker channel only); options:
`send_initial_snapshot` (ticker channel), `skip_ticker_ack`, `use_yes_price`
(orderbook channel; see below), `shard_factor`/`shard_key` (communications),
`index_ids` (cfbenchmarks).

Unsubscribe / list / update (S25 examples, verbatim):

```json
{ "id": 124, "cmd": "unsubscribe", "params": { "sids": [1, 2] } }
{ "id": 3, "cmd": "list_subscriptions" }
{ "id": 124, "cmd": "update_subscription",
  "params": { "sids": [456], "market_tickers": ["NEW-MARKET-1", "NEW-MARKET-2"],
              "action": "add_markets" } }
```

(`update_subscription` actions: `add_markets` / `delete_markets`.)

### Server responses (verbatim examples, S25)

```json
{ "id": 1, "type": "subscribed", "msg": { "channel": "orderbook_delta", "sid": 1 } }
{ "id": 102, "sid": 2, "seq": 7, "type": "unsubscribed" }
{ "id": 3, "type": "ok", "msg": [ { "channel": "orderbook_delta", "sid": 1 },
                                   { "channel": "ticker", "sid": 2 },
                                   { "channel": "fill", "sid": 3 } ] }
{ "id": 123, "type": "error", "msg": { "code": 6, "msg": "Already subscribed" } }
```

`sid` = server-generated subscription id; data messages carry `sid` and (orderbook)
`seq` — "Sequential number that should be checked if you want to guarantee you
received all the messages. Used for snapshot/delta consistency" (S26).

### orderbook_delta channel (S26, verbatim examples)

Snapshot on subscribe, then deltas:

```json
{ "type": "orderbook_snapshot", "sid": 2, "seq": 2,
  "msg": { "market_ticker": "FED-23DEC-T3.00",
           "market_id": "9b0f6b43-5b68-4f9f-9f02-9a2d1b8ac1a1",
           "yes_dollars_fp": [["0.0800", "300.00"], ["0.2200", "333.00"]],
           "no_dollars_fp":  [["0.5400", "20.00"],  ["0.5600", "146.00"]] } }

{ "type": "orderbook_delta", "sid": 2, "seq": 3,
  "msg": { "market_ticker": "FED-23DEC-T3.00",
           "market_id": "9b0f6b43-5b68-4f9f-9f02-9a2d1b8ac1a1",
           "price_dollars": "0.960", "delta_fp": "-54.00", "side": "yes",
           "ts": "2022-11-22T20:44:01Z", "ts_ms": 1669149841000 } }
```

Delta fields: `price_dollars` (string), `delta_fp` (signed fixed-point contract
delta), `side` (`yes`|`no`), optional `client_order_id` + `subaccount` ("Present only
when you caused this orderbook change"), `ts` (deprecated RFC3339), `ts_ms`.
**Pricing convention (S9):** by default no-side levels are in **no-leg pricing** (a
no-side delta at `0.30` = "no at 30c", matching "yes at 70c"); subscribing with
`use_yes_price: true` reports both sides on the yes-leg scale. The default "will be
flipped to `true` in a future release, and the flag will then be removed" (S2/S25
migration plan).

### ticker channel (S27, verbatim example)

```json
{ "type": "ticker", "sid": 11,
  "msg": { "market_ticker": "FED-23DEC-T3.00",
           "market_id": "9b0f6b43-5b68-4f9f-9f02-9a2d1b8ac1a1",
           "price_dollars": "0.480", "yes_bid_dollars": "0.450",
           "yes_ask_dollars": "0.530", "volume_fp": "33896.00",
           "open_interest_fp": "20422.00", "dollar_volume": 16948,
           "dollar_open_interest": 10211, "yes_bid_size_fp": "300.00",
           "yes_ask_size_fp": "150.00", "last_trade_size_fp": "25.00",
           "ts": 1669149841, "ts_ms": 1669149841000,
           "time": "2022-11-22T20:44:01Z" } }
```

### fill channel (private fills; S28, verbatim example)

```json
{ "type": "fill", "sid": 13,
  "msg": { "trade_id": "d91bc706-ee49-470d-82d8-11418bda6fed",
           "order_id": "ee587a1c-8b87-4dcf-b721-9f6f790619fa",
           "market_ticker": "HIGHNY-22DEC23-B53.5", "is_taker": true,
           "side": "yes", "yes_price_dollars": "0.750", "count_fp": "278.00",
           "action": "buy", "ts": 1671899397, "ts_ms": 1671899397000,
           "post_position_fp": "500.00", "purchased_side": "yes",
           "subaccount": 3 } }
```

(`purchased_side` deprecated → `outcome_side`/`book_side` per S9. The WS fill example
carries no fee field; REST GET /portfolio/fills is the fee source of truth.)

Other channels: `trade` (public prints, taker_outcome_side/taker_book_side),
`user_orders` (private order created/updated), `market_positions`,
`market_lifecycle_v2` (market created/activated/determined/settled +
`metadata_updated` incl. `price_level_structure_updated` with values like
`"linear_cent"`, `"deci_cent"`, `"tapered_deci_cent"`), `order_group_updates`,
`communications`, `multivariate*`, `cfbenchmarks_value` (S25/S34).

### WS error codes (S11 table; codes 1–22 + 25)

1 Unable to process message · 2 Params required · 3 Channels required ·
4 Subscription IDs required · 5 Unknown command · 6 Already subscribed ·
7 Unknown subscription ID · 8 Unknown channel name · 9 Authentication required ·
10 Channel error · 11 Invalid parameter · 12 Exactly one subscription ID is required ·
13 Unsupported action · 14 Market Ticker required · 15 Action required ·
16 Market not found · 17 Internal error · 18 Command timeout ·
19/20/21/22 shard_factor/shard_key validation · 25 Subscription buffer overflow
("Subscribe to a smaller subset of data, or ensure that your connection read
throughput is optimized" — added May 12, 2026, S30).

---

## 12. Rate limits

**Source: S7 (verbatim quotes), S30, S33.**

- **Token system**: "Every authenticated request costs tokens. Your tier defines how
  many tokens you can spend per second. Most requests cost the default of **10
  tokens**". Effective rate = `budget ÷ cost`. Per-endpoint costs listed on each API
  page; live list via `GET /trade-api/v2/account/endpoint_costs`.
- **Two independent buckets**: Read ("GET endpoints and anything not explicitly routed
  elsewhere") and Write ("Order placement, amends, cancels, order groups, and the RFQ
  quote flow"). (Perps meter separately — 4 buckets total.)
- **Tiers** (per-second budgets, Read/Write): Basic 200/100, Advanced 300/300,
  Premier 1,000/1,000, Paragon 2,000/2,000, Prime 4,000/4,000.
  Qualification: Basic = signup; Advanced = self-serve
  `POST /account/api_usage_level/upgrade`; Premier+ by trailing 30-day volume share
  (earn/keep: Premier 0.25%/0.20%, Paragon 0.50%/0.40%, Prime 1.00%/0.80%) or manual
  grant. Grants visible via `GET /trade-api/v2/account/limits`, example (S7,
  verbatim):

```json
{
  "usage_tier": "premier",
  "read":  { "refill_rate": 1000, "bucket_capacity": 1000 },
  "write": { "refill_rate": 1000, "bucket_capacity": 2000 },
  "grants": [
    { "exchange_instance": "event_contract", "level": "premier", "expires_ts": 1751558400, "source": "volume" },
    { "exchange_instance": "event_contract", "level": "advanced", "source": "manual" }
  ]
}
```

- **Bursting**: "Your Write bucket holds **two seconds** of your per-second budget" —
  refills continuously; "Basic tier is the exception: its Write bucket holds one
  second of budget, with no accumulated headroom."
- **429 response** (S7, verbatim): "A rate-limited request returns
  `429 Too Many Requests` with the body:"

```json
{"error": "too many requests"}
```

  "429 responses don't currently include `Retry-After` or `X-RateLimit-*` headers.
  Apply exponential backoff on 429 until your bucket refills." (Note: this body shape
  differs from the OpenAPI `ErrorResponse` — both shapes must be tolerated.)
- **Known per-endpoint costs** (pages + June 4 changelog): legacy
  `POST /portfolio/orders` = 100; legacy amend/decrease/batch-create = 100; legacy
  DELETE = 20 (changelog) though the endpoint page still prints 2 (see Uncertainties);
  V2 `/portfolio/events/orders` family unchanged (create ≈ default 10, cancel = 2);
  `GET /portfolio/orders/{order_id}` = 2. "Batch endpoints don't save tokens" — each
  item billed separately.

---

## 13. Demo environment (for operator fixture recording)

**Source: S6, S5, S3.**

- Demo web app: **https://demo.kalshi.co/** — separate account with **mock funds**;
  "For safety, credentials are not shared between this environment and production."
  Step-by-step signup tutorial linked from S6 (Google Slides deck).
- API keys: created in the demo UI exactly like prod ("This process is the same for
  the demo or production environment" — S3): Account & security → API Keys → Create
  Key → download `.key` PEM + Key ID. (Or `POST /api_keys/generate` once one key
  exists.)
- Demo REST root: `https://external-api.demo.kalshi.co/trade-api/v2` (also
  `https://demo-api.kalshi.co/trade-api/v2`); WS:
  `wss://external-api-ws.demo.kalshi.co/trade-api/ws/v2` (also
  `wss://demo-api.kalshi.co/trade-api/ws/v2`).
- All quick-start examples in the official docs run against demo first (S4, S10).

Adjacent endpoints useful to the adapter (same auth): `GET /exchange/status` →
`{"exchange_active": bool, "trading_active": bool, "exchange_estimated_resume_time":
date-time|null}` (S1) — venue-level halt detection; `GET /exchange/schedule`;
`GET /exchange/user_data_timestamp` ("approximate indication" of data freshness —
combine write responses with WS data, S34).

---

## Uncertainties / fixture-confirmation checklist

Repo rule: an operator-recorded fixture must confirm each behavior below before any
live use. Record fixtures against **demo** first; flag any prod divergence.

**Auth round-trip**
1. Happy-path signed GET `/portfolio/balance` (headers exactly as §1) — confirm 200.
2. **Timestamp skew tolerance is undocumented.** Record requests with timestamps
   skewed -5s, -30s, -5min, +5s, +5min; capture the exact 401 body. (Pre-2026
   community lore said ±a few seconds; nothing official today.)
3. Exact 401 body for: bad signature, unknown key id, missing header, expired
   timestamp — needed to map to adapter error enums. Docs only give the generic
   `ErrorResponse` shape; the rate-limit page shows a different `{"error": ...}` shape,
   so the real key of the auth-error body must be observed.
4. Whether the signature `path` must include `/trade-api/v2` for **both** hosts and
   for WS (`/trade-api/ws/v2`) — documented yes (§1, §11) but confirm once per host.
5. Whether unauthenticated GET /markets, /markets/{ticker} works in prod and demo
   (docs imply yes; spec marks orderbook as auth-required) and what rate limit applies
   to unauthenticated traffic (undocumented).

**Orders**
6. V2 create: 201 body matches §5a; IOC `remaining_count` semantics; `average_fill_price`
   present only when filled.
7. **Duplicate `client_order_id` → capture the exact 409 ErrorResponse `code` string**
   (docs promise rejection but not the code value; old `order_already_exists` constant
   is no longer documented). Also confirm: does a *canceled* order's client_order_id
   free up for reuse? (Undocumented.)
8. **Insufficient balance → capture exact 400/4xx `code`/`message`** (undocumented).
9. Invalid price for the market's `price_level_structure` (e.g. `0.515` on a
   `linear_cent` market) → exact error body.
10. `post_only` crossing the book: confirm order is canceled, response shape, and
    `last_update_reason = PostOnlyCrossCancel` on the user_orders/GET order surface.
11. `self_trade_prevention_type` both modes — observed cancel behavior.
12. Legacy POST /portfolio/orders still accepts cent-integer `yes_price`/`no_price`
    (and that we should NOT use it: 10x token cost, deprecation announced).
13. Whether V2 create rejects `count`/`price` given as numbers instead of strings.

**Cancel**
14. V2 cancel 200 body; cancel of already-canceled / already-executed / unknown order
    → which of 404 vs 200-with-zero `reduced_by`; exact bodies.
15. The May 21 changelog caveat: V2 cancel/amend response may describe the wrong
    order — adapter must reconcile via `GET /portfolio/orders/{order_id}`; fixture the
    normal case and code defensively for the mismatch case.
16. Legacy DELETE token cost: page says 2, changelog says 20 → read
    `GET /account/endpoint_costs` and record actuals for both order families.

**Pagination**
17. Cursor semantics on /markets, /portfolio/orders, /portfolio/fills,
    /portfolio/settlements, /portfolio/positions: empty-string vs absent `cursor` on
    last page; cursor stability across inserts; behavior when `cursor` is garbage
    (error body) or expired.
18. `limit` over max (e.g. 1001) → 400 body or silent clamp?

**Data/units**
19. Fill `fee_cost` is a dollars **string** (§7) while Settlement `revenue`/`value`
    are cent **integers** and Balance has both cents int + dollars string — fixture
    one of each and lock parsers to these exact types.
20. REST orderbook: confirm `no_dollars` levels are quoted in no-leg pricing (docs
    describe equivalence but the REST page does not state the leg explicitly the way
    the WS docs do).
21. `Market.status` lifecycle values observed in the wild vs the spec enum
    (`initialized`...`finalized`) — and which map to "tradeable" (filter-vocabulary
    `open` ↔ response `active`: confirm).
22. Series `fee_type`/`fee_multiplier` for the series we trade + fee math vs the PDF
    fee schedule (quadratic vs flat) — confirm with a real demo fill's `fee_cost`.

**WebSocket (Phase 2, record now if convenient)**
23. Handshake auth with signed `/trade-api/ws/v2`; capture `subscribed`, snapshot,
    delta sequence; verify `seq` gap detection and re-subscribe flow.
24. `use_yes_price: true` no-side pricing transform — and watch for the announced
    default flip (S2 migration plan).
25. Ping/pong: server pings every 10s with body `heartbeat`; confirm client lib pongs.

**Environment**
26. Demo/prod parity: demo may run newer/older builds; re-record any
    fixture-confirmed behavior against prod read-only endpoints before first live use.
27. `GET /exchange/status` shapes during a real maintenance window (halt detection
    for the I2/I3 rails).

**Deprecation watch (re-verify before adapter freeze)**
- Legacy `side`/`action` on Order/Fill: "will not be removed before May 14, 2026" —
  date has passed; fields were still in the spec on 2026-06-10. Adapter must read
  `outcome_side`/`book_side` only.
- Legacy `/portfolio/orders` family: "deprecated no earlier than May 6, 2026"; cost
  already 10x. Build on `/portfolio/events/orders`.
- `fractional_trading_enabled`, `liquidity_dollars`, `response_price_units`,
  `tick_size` (already removed), `resting_orders_count`: do not depend on these.
- Integer `count` and cent `yes_price`/`no_price` request fields: "Integer contract
  count fields are legacy and will be deprecated" (S1) — use `_fp`/`_dollars` forms.
