> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Markets WebSocket

> Real-time market data, order book, and trades

# Markets WebSocket

The Markets WebSocket endpoint provides real-time market data including order book updates, price changes, and trade notifications.

<Warning>
  **Authentication Required**

  This WebSocket endpoint requires API key authentication in the connection handshake. See the [Authentication guide](/api/authentication) for details.
</Warning>

## Endpoint

```
wss://api.polymarket.us/v1/ws/markets
```

## Subscription Types

| Value                                | Description                      |
| ------------------------------------ | -------------------------------- |
| `SUBSCRIPTION_TYPE_MARKET_DATA`      | Full order book and market stats |
| `SUBSCRIPTION_TYPE_MARKET_DATA_LITE` | Lightweight price data only      |
| `SUBSCRIPTION_TYPE_TRADE`            | Real-time trade notifications    |

## Market Data Subscription

### Subscribe to Full Market Data

```json theme={null}
{
  "subscribe": {
    "requestId": "md-sub-1",
    "subscriptionType": "SUBSCRIPTION_TYPE_MARKET_DATA",
    "marketSlugs": ["market-slug-1", "market-slug-2"]
  }
}
```

### Market Data Response

Full order book with market statistics:

```json theme={null}
{
  "requestId": "md-sub-1",
  "subscriptionType": "SUBSCRIPTION_TYPE_MARKET_DATA",
  "marketData": {
    "marketSlug": "market-slug-1",
    "bids": [
      {"px": {"value": "0.555", "currency": "USD"}, "qty": "0.50"},
      {"px": {"value": "0.550", "currency": "USD"}, "qty": "2.50"}
    ],
    "offers": [
      {"px": {"value": "0.560", "currency": "USD"}, "qty": "0.80"},
      {"px": {"value": "0.565", "currency": "USD"}, "qty": "1.50"}
    ],
    "state": "MARKET_STATE_OPEN",
    "stats": {
      "lastTradePx": {"value": "0.55", "currency": "USD"},
      "sharesTraded": "150000",
      "openInterest": "500000",
      "highPx": {"value": "0.58", "currency": "USD"},
      "lowPx": {"value": "0.52", "currency": "USD"}
    },
    "transactTime": "2024-01-15T10:30:00Z"
  }
}
```

## Market Data Lite Subscription

### Subscribe to Lightweight Data

For reduced bandwidth, use the lite subscription:

```json theme={null}
{
  "subscribe": {
    "requestId": "mdl-sub-1",
    "subscriptionType": "SUBSCRIPTION_TYPE_MARKET_DATA_LITE",
    "marketSlugs": ["market-slug-1"]
  }
}
```

### Market Data Lite Response

```json theme={null}
{
  "requestId": "mdl-sub-1",
  "subscriptionType": "SUBSCRIPTION_TYPE_MARKET_DATA_LITE",
  "marketDataLite": {
    "marketSlug": "market-slug-1",
    "currentPx": {"value": "0.55", "currency": "USD"},
    "lastTradePx": {"value": "0.55", "currency": "USD"},
    "bestBid": {"value": "0.54", "currency": "USD"},
    "bestAsk": {"value": "0.56", "currency": "USD"},
    "bidDepth": 5,
    "askDepth": 4,
    "sharesTraded": "150000",
    "openInterest": "500000"
  }
}
```

## Trade Subscription

### Subscribe to Trades

```json theme={null}
{
  "subscribe": {
    "requestId": "trade-sub-1",
    "subscriptionType": "SUBSCRIPTION_TYPE_TRADE",
    "marketSlugs": ["market-slug-1"]
  }
}
```

### Trade Response

```json theme={null}
{
  "requestId": "trade-sub-1",
  "subscriptionType": "SUBSCRIPTION_TYPE_TRADE",
  "trade": {
    "marketSlug": "market-slug-1",
    "price": {"value": "0.555", "currency": "USD"},
    "quantity": {"value": "0.50", "currency": "USD"},
    "tradeTime": "2024-01-15T10:30:00Z",
    "maker": {
      "side": "ORDER_SIDE_BUY",
      "intent": "ORDER_INTENT_BUY_LONG"
    },
    "taker": {
      "side": "ORDER_SIDE_SELL",
      "intent": "ORDER_INTENT_SELL_LONG"
    }
  }
}
```

## Market States

| Value                                  | Description                   |
| -------------------------------------- | ----------------------------- |
| `MARKET_STATE_OPEN`                    | Market open for trading       |
| `MARKET_STATE_PREOPEN`                 | Market in pre-open phase      |
| `MARKET_STATE_SUSPENDED`               | Trading temporarily suspended |
| `MARKET_STATE_HALTED`                  | Trading halted                |
| `MARKET_STATE_EXPIRED`                 | Market has expired            |
| `MARKET_STATE_TERMINATED`              | Market terminated             |
| `MARKET_STATE_MATCH_AND_CLOSE_AUCTION` | Market in closing auction     |

## Order Side

| Value             | Description |
| ----------------- | ----------- |
| `ORDER_SIDE_BUY`  | Buy order   |
| `ORDER_SIDE_SELL` | Sell order  |

## Order Intent

| Value                     | Description        |
| ------------------------- | ------------------ |
| `ORDER_INTENT_BUY_LONG`   | Buy YES contracts  |
| `ORDER_INTENT_SELL_LONG`  | Sell YES contracts |
| `ORDER_INTENT_BUY_SHORT`  | Buy NO contracts   |
| `ORDER_INTENT_SELL_SHORT` | Sell NO contracts  |

## Debouncing

For high-frequency markets, enable response debouncing to reduce message volume:

```json theme={null}
{
  "subscribe": {
    "requestId": "md-sub-1",
    "subscriptionType": "SUBSCRIPTION_TYPE_MARKET_DATA",
    "marketSlugs": ["market-slug-1"],
    "responsesDebounced": true
  }
}
```

When debouncing is enabled, updates are batched and sent at regular intervals rather than on every change.

<Tip>
  **Subscription Limits**

  You can subscribe to a maximum of 100 markets per subscription. Use multiple subscriptions if you need more.
</Tip>

## Order Book Depth

The full market data subscription includes the top levels of the order book. Each level shows:

| Field | Description                                                                      |
| ----- | -------------------------------------------------------------------------------- |
| `px`  | Price level                                                                      |
| `qty` | Total quantity at this price. May contain decimals for partial-contract markets. |

Order book levels are sorted best-to-worst (highest bid first, lowest ask first).
