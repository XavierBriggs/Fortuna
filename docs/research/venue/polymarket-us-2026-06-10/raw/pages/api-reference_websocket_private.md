> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Private WebSocket

> Real-time orders, positions, and balance updates

# Private WebSocket

The Private WebSocket endpoint provides real-time updates for user-specific data including orders, positions, and account balances.

<Warning>
  **Authentication Required**

  This WebSocket endpoint requires API key authentication in the connection handshake. See the [Authentication guide](/api/authentication) for details.
</Warning>

## Endpoint

```
wss://api.polymarket.us/v1/ws/private
```

## Subscription Types

| Value                               | Description                           |
| ----------------------------------- | ------------------------------------- |
| `SUBSCRIPTION_TYPE_ORDER`           | Order updates (new, filled, canceled) |
| `SUBSCRIPTION_TYPE_ORDER_SNAPSHOT`  | Initial snapshot of open orders       |
| `SUBSCRIPTION_TYPE_POSITION`        | Position changes                      |
| `SUBSCRIPTION_TYPE_ACCOUNT_BALANCE` | Account balance changes               |

## Order Subscriptions

### Subscribe to Orders

```json theme={null}
{
  "subscribe": {
    "requestId": "order-sub-1",
    "subscriptionType": "SUBSCRIPTION_TYPE_ORDER",
    "marketSlugs": ["market-slug-1"]
  }
}
```

Leave `marketSlugs` empty to subscribe to all markets.

### Order Snapshot Response

Initial snapshot of open orders:

```json theme={null}
{
  "requestId": "order-sub-1",
  "subscriptionType": "SUBSCRIPTION_TYPE_ORDER",
  "orderSubscriptionSnapshot": {
    "orders": [
      {
        "id": "order-123",
        "marketSlug": "market-slug-1",
        "side": "ORDER_SIDE_BUY",
        "type": "ORDER_TYPE_LIMIT",
        "price": {"value": "0.555", "currency": "USD"},
        "quantity": 0.5,
        "leavesQuantity": 0.5,
        "state": "ORDER_STATE_PENDING_NEW",
        "intent": "ORDER_INTENT_BUY_LONG",
        "tif": "TIME_IN_FORCE_GOOD_TILL_CANCEL"
      }
    ],
    "eof": true
  }
}
```

### Order Update Response

Real-time order execution updates:

```json theme={null}
{
  "requestId": "order-sub-1",
  "subscriptionType": "SUBSCRIPTION_TYPE_ORDER",
  "orderSubscriptionUpdate": {
    "execution": {
      "id": "exec-456",
      "order": {...},
      "lastShares": "0.25",
      "lastPx": {"value": "0.555", "currency": "USD"},
      "type": "EXECUTION_TYPE_PARTIAL_FILL",
      "tradeId": "trade-789"
    }
  }
}
```

Order quantities can contain decimals for partial-contract markets. `lastShares` is a string and may also contain a decimal quantity.

## Position Subscriptions

### Subscribe to Positions

```json theme={null}
{
  "subscribe": {
    "requestId": "pos-sub-1",
    "subscriptionType": "SUBSCRIPTION_TYPE_POSITION",
    "marketSlugs": ["market-slug-1"]
  }
}
```

### Position Update Response

```json theme={null}
{
  "requestId": "pos-sub-1",
  "subscriptionType": "SUBSCRIPTION_TYPE_POSITION",
  "positionSubscription": {
    "beforePosition": {
      "netPosition": "1",
      "netPositionDecimal": "1.0000",
      "cost": {"value": "55.00", "currency": "USD"}
    },
    "afterPosition": {
      "netPosition": "2",
      "netPositionDecimal": "1.5000",
      "cost": {"value": "82.50", "currency": "USD"}
    },
    "updateTime": "2024-01-15T10:30:00Z",
    "entryType": "LEDGER_ENTRY_TYPE_ORDER_EXECUTION",
    "tradeId": "trade-789"
  }
}
```

Position messages can include `netPositionDecimal`, `qtyBoughtDecimal`, `qtySoldDecimal`, `bodPositionDecimal`, and `qtyAvailableDecimal`, matching `GET /v1/portfolio/positions`. Use those fields when present; the older integer fields are rounded and remain for backward compatibility.

## Account Balance Subscriptions

### Subscribe to Balances

```json theme={null}
{
  "subscribe": {
    "requestId": "balance-sub-1",
    "subscriptionType": "SUBSCRIPTION_TYPE_ACCOUNT_BALANCE"
  }
}
```

### Balance Snapshot Response

```json theme={null}
{
  "requestId": "balance-sub-1",
  "subscriptionType": "SUBSCRIPTION_TYPE_ACCOUNT_BALANCE",
  "accountBalancesSnapshot": {
    "balances": [
      {
        "currentBalance": 1000.00,
        "currency": "USD",
        "buyingPower": 850.00
      }
    ]
  }
}
```

### Balance Update Response

```json theme={null}
{
  "requestId": "balance-sub-1",
  "subscriptionType": "SUBSCRIPTION_TYPE_ACCOUNT_BALANCE",
  "accountBalancesUpdate": {
    "balanceChange": {
      "beforeBalance": {...},
      "afterBalance": {...},
      "description": "Order execution",
      "updateTime": "2024-01-15T10:30:00Z",
      "entryType": "LEDGER_ENTRY_TYPE_ORDER_EXECUTION"
    }
  }
}
```

## Execution Types

| Value                         | Description                    |
| ----------------------------- | ------------------------------ |
| `EXECUTION_TYPE_PARTIAL_FILL` | Order partially filled         |
| `EXECUTION_TYPE_FILL`         | Order fully filled             |
| `EXECUTION_TYPE_CANCELED`     | Order canceled                 |
| `EXECUTION_TYPE_REPLACE`      | Order replaced/modified        |
| `EXECUTION_TYPE_REJECTED`     | Order rejected                 |
| `EXECUTION_TYPE_EXPIRED`      | Order expired                  |
| `EXECUTION_TYPE_DONE_FOR_DAY` | Order done for the trading day |

## Ledger Entry Types

| Value                                   | Description               |
| --------------------------------------- | ------------------------- |
| `LEDGER_ENTRY_TYPE_ORDER_EXECUTION`     | Trade execution           |
| `LEDGER_ENTRY_TYPE_DEPOSIT`             | Account deposit           |
| `LEDGER_ENTRY_TYPE_WITHDRAWAL`          | Account withdrawal        |
| `LEDGER_ENTRY_TYPE_RESOLUTION`          | Market resolution         |
| `LEDGER_ENTRY_TYPE_COMMISSION`          | Commission charge         |
| `LEDGER_ENTRY_TYPE_CORRECTION`          | Balance correction        |
| `LEDGER_ENTRY_TYPE_NETTING`             | Netting adjustment        |
| `LEDGER_ENTRY_TYPE_MANUAL_ADJUSTMENT`   | Manual balance adjustment |
| `LEDGER_ENTRY_TYPE_CONTRACT_EXPIRATION` | Contract expiration       |

## Order States

| Value                          | Description                                |
| ------------------------------ | ------------------------------------------ |
| `ORDER_STATE_PENDING_NEW`      | Order received, not yet processed          |
| `ORDER_STATE_PENDING_REPLACE`  | Modify request received, not yet processed |
| `ORDER_STATE_PENDING_CANCEL`   | Cancel request received, not yet processed |
| `ORDER_STATE_PENDING_RISK`     | Order pending risk approval                |
| `ORDER_STATE_PARTIALLY_FILLED` | Order partially executed                   |
| `ORDER_STATE_FILLED`           | Order fully executed                       |
| `ORDER_STATE_CANCELED`         | Order canceled                             |
| `ORDER_STATE_REPLACED`         | Order replaced                             |
| `ORDER_STATE_REJECTED`         | Order rejected                             |
| `ORDER_STATE_EXPIRED`          | Order expired                              |

## Order Intent

| Value                     | Description        |
| ------------------------- | ------------------ |
| `ORDER_INTENT_BUY_LONG`   | Buy YES contracts  |
| `ORDER_INTENT_SELL_LONG`  | Sell YES contracts |
| `ORDER_INTENT_BUY_SHORT`  | Buy NO contracts   |
| `ORDER_INTENT_SELL_SHORT` | Sell NO contracts  |

<Tip>
  **Subscription Limits**

  You can subscribe to a maximum of 100 markets per subscription. Use multiple subscriptions if you need more.
</Tip>
