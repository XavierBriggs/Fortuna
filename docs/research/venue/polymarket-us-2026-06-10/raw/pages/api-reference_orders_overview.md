> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Orders API Overview

> Create, cancel, and manage orders

# Orders API

The Orders API provides order entry and management capabilities for trading on markets.

<Warning>
  **Authentication Required**

  All Orders API endpoints require API key authentication. See the [Authentication guide](/api/authentication) for details on signing requests.
</Warning>

## Base URL

```
https://api.polymarket.us
```

## Endpoints

### Order Entry

| Method | Endpoint                   | Description                     |
| ------ | -------------------------- | ------------------------------- |
| `POST` | `/v1/orders`               | Create a new order              |
| `POST` | `/v1/order/preview`        | Preview order before submission |
| `POST` | `/v1/order/close-position` | Close an existing position      |

### Order Query

| Method | Endpoint              | Description                |
| ------ | --------------------- | -------------------------- |
| `GET`  | `/v1/orders/open`     | Get all open orders        |
| `GET`  | `/v1/order/{orderId}` | Get a specific order by ID |

### Order Management

| Method | Endpoint                     | Description              |
| ------ | ---------------------------- | ------------------------ |
| `POST` | `/v1/order/{orderId}/modify` | Modify an existing order |
| `POST` | `/v1/order/{orderId}/cancel` | Cancel a specific order  |
| `POST` | `/v1/orders/open/cancel`     | Cancel all open orders   |

### Batched Operations

Up to 20 orders per call.

| Method | Endpoint                    | Description                                                                          |
| ------ | --------------------------- | ------------------------------------------------------------------------------------ |
| `POST` | `/v1/orders/batched`        | Create up to 20 orders in a single request                                           |
| `POST` | `/v1/orders/batched/cancel` | Cancel up to 20 specific orders by ID                                                |
| `POST` | `/v1/orders/batched/modify` | Modify up to 20 existing orders (each forwarded to the exchange as a cancel-replace) |

Note: `/v1/orders/open/cancel` cancels **all** of your open orders (optionally filtered by market). Use `/v1/orders/batched/cancel` when you want to cancel a specific list of order IDs.

<Warning>
  **Batched responses do not confirm per-entry success.**

  * Gateway validation is atomic: if any entry fails request-shape checks (missing `orderId`, batch > 20, etc.), the whole batch is rejected with `400`.
  * The exchange processes entries independently. An unknown `orderId` in a batched cancel or modify is silently ignored.
  * `canceledOrderIds` and `modifiedOrderIds` echo the request; they do not certify that each ID was acted on. For real outcomes, subscribe to the [Private WebSocket order stream](/api-reference/websocket/private) and watch for `EXECUTION_TYPE_REPLACE`, `EXECUTION_TYPE_CANCELED`, and `EXECUTION_TYPE_REJECTED`.
  * `createdOrderIds` from `/v1/orders/batched` are real exchange-assigned IDs, but per-order accept/fill/reject events still come via the order stream.
</Warning>

## Order Types

All enum values are passed as **strings** in the request body:

| Value               | Description                                   |
| ------------------- | --------------------------------------------- |
| `ORDER_TYPE_LIMIT`  | Limit order at specified price                |
| `ORDER_TYPE_MARKET` | Market order executed at best available price |

**Example:**

```json theme={null}
{
  "type": "ORDER_TYPE_LIMIT"
}
```

## Order Intent

Orders require an intent indicating position direction. Pass these as **string** values:

| Value                     | Description                                  |
| ------------------------- | -------------------------------------------- |
| `ORDER_INTENT_BUY_LONG`   | Buy YES contracts (go long on Yes outcome)   |
| `ORDER_INTENT_SELL_LONG`  | Sell YES contracts (close long Yes position) |
| `ORDER_INTENT_BUY_SHORT`  | Buy NO contracts (go long on No outcome)     |
| `ORDER_INTENT_SELL_SHORT` | Sell NO contracts (close long No position)   |

**Example - Buy NO contracts:**

```json theme={null}
{
  "marketSlug": "your-market-slug",
  "type": "ORDER_TYPE_LIMIT",
  "price": { "value": "0.45", "currency": "USD" },
  "quantity": 10,
  "tif": "TIME_IN_FORCE_GOOD_TILL_CANCEL",
  "intent": "ORDER_INTENT_BUY_SHORT"
}
```

### Alternative: Outcome Side + Action

Instead of `intent`, you can specify the equivalent `outcomeSide` + `action` pair. Both forms are accepted on `CreateOrder` and the batched variants. If both are sent, `outcomeSide`+`action` wins.

| `outcomeSide`      | `action`            | Equivalent `intent`       |
| ------------------ | ------------------- | ------------------------- |
| `OUTCOME_SIDE_YES` | `ORDER_ACTION_BUY`  | `ORDER_INTENT_BUY_LONG`   |
| `OUTCOME_SIDE_YES` | `ORDER_ACTION_SELL` | `ORDER_INTENT_SELL_LONG`  |
| `OUTCOME_SIDE_NO`  | `ORDER_ACTION_BUY`  | `ORDER_INTENT_BUY_SHORT`  |
| `OUTCOME_SIDE_NO`  | `ORDER_ACTION_SELL` | `ORDER_INTENT_SELL_SHORT` |

**Example: same "buy NO contracts" order, expressed with outcomeSide + action:**

```json theme={null}
{
  "marketSlug": "your-market-slug",
  "type": "ORDER_TYPE_LIMIT",
  "price": { "value": "0.45", "currency": "USD" },
  "quantity": 10,
  "tif": "TIME_IN_FORCE_GOOD_TILL_CANCEL",
  "outcomeSide": "OUTCOME_SIDE_NO",
  "action": "ORDER_ACTION_BUY"
}
```

The `Order` returned by `GET /v1/order/{orderId}` and `GET /v1/orders/open` includes both `intent` and `outcomeSide`+`action`, so you can read whichever you prefer. The price-vs-side rules below apply to both forms.

### Understanding Price with Order Intent

Only the long side (YES) is directly tradable. The short side (NO) is synthetic exposure created through positions in the long side. The `price.value` field always represents the long side's price, regardless of which order intent you use. In the market slug, the first team is always the long/YES side and the second team is the short/NO side. A common mistake is attempting to buy both YES at 0.60 and NO at 0.40, which causes a self-match error. Since YES and NO prices must sum to \$1.00, buying NO at 0.40 is equivalent to buying YES at 0.60 - you're placing two buy orders at the same price level on the same instrument. If you want exposure to both sides, use different price levels (e.g., buy YES at 0.55 and buy NO at 0.50).

**Example:** For market `aec-cbb-usc-iowa-2026-01-28`:

* YES (long side) = USC
* NO (short side) = Iowa
* `price.value` always refers to USC's price

**How This Affects Your Orders:**

| You Want To       | Order Intent               | price.value |
| ----------------- | -------------------------- | ----------- |
| Buy USC at 0.83   | ORDER\_INTENT\_BUY\_LONG   | 0.83        |
| Sell USC at 0.83  | ORDER\_INTENT\_SELL\_LONG  | 0.83        |
| Buy Iowa at 0.83  | ORDER\_INTENT\_BUY\_SHORT  | 0.17        |
| Sell Iowa at 0.83 | ORDER\_INTENT\_SELL\_SHORT | 0.17        |

In binary markets, YES and NO are inverses: buying NO at 0.83 is equivalent to buying YES at 0.17 (1.00 - 0.83). Since `price.value` always represents the YES side, you must set it to 0.17 when trading Iowa (NO) at 0.83. To trade the NO side at any price X, set `price.value = 1.00 - X`.

## Price Validation

Orders must have `price.value` between 0.01 and 0.99 (the exchange's absolute price limits).

**Invalid prices (below 0.01 or above 0.99) are restricted at the exchange level.** Since the order is sent to the exchange, you will still receive an orderID, but the order will never fill because it gets rejected during validation.

**Example:**

```json theme={null}
{
  "price": {"value": "45", "currency": "USD"}  // Invalid - will receive orderID but order rejected
}
```

Always validate price bounds client-side before submission to avoid unnecessary orderIDs for rejected orders.

### Quantity and Tick Size by Market

Markets can differ in both minimum order quantity and minimum price increment. Read these fields from the market response before submitting or modifying an order:

| Market field            | Use                                                                                          |
| ----------------------- | -------------------------------------------------------------------------------------------- |
| `minimumTradeQty`       | Smallest valid `quantity`, expressed in contracts. A value of `0.01` means 1% of a contract. |
| `orderPriceMinTickSize` | Smallest valid `price.value` increment. A value of `0.005` means half-cent ticks.            |

The `quantity` field on order requests and order responses is a number and can contain decimals for partial-contract markets. Submit `quantity` and `price.value` already aligned to the market's `minimumTradeQty` and `orderPriceMinTickSize`. Extra precision is not part of the public contract and can be normalized to the market precision; for example, on a market with `minimumTradeQty: 0.01` and `orderPriceMinTickSize: 0.01`, `quantity: 0.015` can be accepted and returned as `0.01`, and `price.value: "0.515"` can be returned as `"0.51"`.

## Order Side

The order side indicates buy or sell direction:

| Value             | Description |
| ----------------- | ----------- |
| `ORDER_SIDE_BUY`  | Buy order   |
| `ORDER_SIDE_SELL` | Sell order  |

## Order States

Orders progress through these states:

```
PENDING_NEW → PARTIALLY_FILLED → FILLED
                    ↓
                CANCELED / REJECTED / EXPIRED
```

| Value                          | Description                                               |
| ------------------------------ | --------------------------------------------------------- |
| `ORDER_STATE_PENDING_NEW`      | Order received, not yet processed by matching engine      |
| `ORDER_STATE_NEW`              | Order accepted by matching engine and resting on the book |
| `ORDER_STATE_PENDING_REPLACE`  | Modify request received, not yet processed                |
| `ORDER_STATE_PENDING_CANCEL`   | Cancel request received, not yet processed                |
| `ORDER_STATE_PENDING_RISK`     | Order pending risk approval                               |
| `ORDER_STATE_PARTIALLY_FILLED` | Order partially executed                                  |
| `ORDER_STATE_FILLED`           | Order fully executed                                      |
| `ORDER_STATE_CANCELED`         | Order canceled                                            |
| `ORDER_STATE_REPLACED`         | Order replaced via modify (cancel-replace)                |
| `ORDER_STATE_REJECTED`         | Order rejected by exchange                                |
| `ORDER_STATE_EXPIRED`          | Order expired (GTD orders)                                |

## Time in Force

| Value                               | Description                                              |
| ----------------------------------- | -------------------------------------------------------- |
| `TIME_IN_FORCE_DAY`                 | DAY - Expires at the end of the trading day              |
| `TIME_IN_FORCE_GOOD_TILL_CANCEL`    | GTC - Remains active until filled or canceled            |
| `TIME_IN_FORCE_GOOD_TILL_DATE`      | GTD - Expires at specified `goodTillTime`                |
| `TIME_IN_FORCE_IMMEDIATE_OR_CANCEL` | IOC - Fills immediately available quantity, cancels rest |
| `TIME_IN_FORCE_FILL_OR_KILL`        | FOK - Must fill entirely or cancel completely            |

## Manual Order Indicator

Required to indicate whether the order is placed by a human or automated system:

| Value                              | Description                                 |
| ---------------------------------- | ------------------------------------------- |
| `MANUAL_ORDER_INDICATOR_MANUAL`    | Order placed manually by a user             |
| `MANUAL_ORDER_INDICATOR_AUTOMATIC` | Order placed by an automated trading system |

## Execution Types

Execution events returned in synchronous order responses:

| Value                         | Description                                     |
| ----------------------------- | ----------------------------------------------- |
| `EXECUTION_TYPE_NEW`          | Order accepted (new working order confirmation) |
| `EXECUTION_TYPE_PARTIAL_FILL` | Order partially filled                          |
| `EXECUTION_TYPE_FILL`         | Order fully filled                              |
| `EXECUTION_TYPE_CANCELED`     | Order canceled                                  |
| `EXECUTION_TYPE_REPLACE`      | Order replaced/modified                         |
| `EXECUTION_TYPE_REJECTED`     | Order rejected                                  |
| `EXECUTION_TYPE_EXPIRED`      | Order expired                                   |
| `EXECUTION_TYPE_DONE_FOR_DAY` | Order done for the trading day                  |

## Order Reject Reasons

If an order is rejected, the reason will be one of:

| Value                                       | Description                                                               |
| ------------------------------------------- | ------------------------------------------------------------------------- |
| `ORD_REJECT_REASON_EXCHANGE_OPTION`         | Generic exchange-defined reason (used when no more specific code applies) |
| `ORD_REJECT_REASON_UNKNOWN_MARKET`          | Unknown or invalid market                                                 |
| `ORD_REJECT_REASON_EXCHANGE_CLOSED`         | Exchange/market is closed                                                 |
| `ORD_REJECT_REASON_INCORRECT_QUANTITY`      | Invalid quantity                                                          |
| `ORD_REJECT_REASON_INVALID_PRICE_INCREMENT` | Price not on valid increment                                              |
| `ORD_REJECT_REASON_INCORRECT_ORDER_TYPE`    | Invalid order type for market                                             |
| `ORD_REJECT_REASON_PRICE_OUT_OF_BOUNDS`     | Price outside valid range                                                 |
| `ORD_REJECT_REASON_NO_LIQUIDITY`            | No liquidity for market order                                             |

## Slippage Tolerance

For market orders or close position orders, you can specify slippage tolerance:

```json theme={null}
{
  "slippageTolerance": {
    "currentPrice": { "value": "0.50", "currency": "USD" },
    "ticks": 5
  }
}
```

| Field          | Type    | Description                                                  |
| -------------- | ------- | ------------------------------------------------------------ |
| `currentPrice` | Amount  | Reference price for slippage calculation                     |
| `bips`         | integer | Slippage tolerance in basis points (1 bip = 0.01%)           |
| `ticks`        | integer | Slippage tolerance in price ticks (takes priority over bips) |

### Default Values

`slippageTolerance` is optional and defaults to:

* **Market orders**: Unlimited (no slippage protection by default)
* **Limit orders**: Not applicable (price is fixed)

Slippage tolerance defines the maximum price movement you'll accept. For example, if you submit a market order to buy at current price 0.50 with `ticks: 5`, the order will reject if the best ask moves above 0.55 before execution.

<Tip>
  **Real-Time Order Updates**

  After submitting orders via REST, use the [WebSocket Private Stream](/api-reference/websocket/private) to receive real-time updates on order status, fills, and cancellations.
</Tip>

## Complete Create Order Example

```json theme={null}
{
  "marketSlug": "your-market-slug",
  "type": "ORDER_TYPE_LIMIT",
  "price": {
    "value": "0.555",
    "currency": "USD"
  },
  "quantity": 0.5,
  "tif": "TIME_IN_FORCE_GOOD_TILL_CANCEL",
  "intent": "ORDER_INTENT_BUY_LONG",
  "manualOrderIndicator": "MANUAL_ORDER_INDICATOR_MANUAL",
  "participateDontInitiate": false
}
```

## Rate Limits

The API enforces a global rate limit of **20 requests per second** per API key across all endpoints.

<Warning>
  **Rate Limit Exceeded**

  When rate limits are exceeded, the API returns HTTP status `429 Too Many Requests`.
</Warning>

**Notes:**

* Rate limits are enforced at the edge (Cloudflare) before requests reach the API
* Limits are applied per API key
* Implement exponential backoff and request throttling in your application

## Best Practices

1. **Use string enum values** - All enums are passed as strings (e.g., `"ORDER_TYPE_LIMIT"`, not `1`)
2. **Use WebSocket for updates** - Subscribe to order updates instead of polling
3. **Preview before submit** - Use the preview endpoint for order validation
4. **Handle rejects** - Implement proper error handling for rejected orders
5. **Use asynchronous execution for limit orders** - For market-making and resting limit orders, avoid `synchronousExecution: true` as it waits up to 10 seconds for final order state. Instead, submit orders asynchronously (the default) and poll with `GET /v1/order/{orderId}` to check status (\~100ms). Only use `synchronousExecution: true` for immediately-fillable orders where you need to wait for fill confirmation.
6. **Specify manual order indicator** - Required for regulatory compliance
7. **Respect rate limits** - Implement request throttling to stay within rate limits and avoid 429 errors
