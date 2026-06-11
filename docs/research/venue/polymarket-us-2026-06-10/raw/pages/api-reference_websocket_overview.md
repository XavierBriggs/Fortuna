> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# WebSocket API Overview

> Real-time streaming data via WebSocket

# WebSocket API

The WebSocket API provides real-time streaming data for market information and private user data.

## Endpoints

| Endpoint         | Description                                | Authentication |
| ---------------- | ------------------------------------------ | -------------- |
| `/v1/ws/private` | Orders, positions, account balance updates | API Key        |
| `/v1/ws/markets` | Market data, order book, trades            | API Key        |

## Connection

Connect to the WebSocket endpoints with your API key credentials:

```
wss://api.polymarket.us/v1/ws/private
wss://api.polymarket.us/v1/ws/markets
```

## Authentication

WebSocket connections use the same API key authentication as the REST API.

Include these headers in the WebSocket handshake:

```
X-PM-Access-Key: <your-api-key-id>
X-PM-Timestamp: <timestamp-in-milliseconds>
X-PM-Signature: <base64-encoded-signature>
```

The signature is constructed from: `timestamp + "GET" + path` where path is `/v1/ws/private` or `/v1/ws/markets`.

See [Authentication](/api/authentication) for details on request signing.

## Message Format

All WebSocket messages are JSON formatted with snake\_case field names.

### Request Format

```json theme={null}
{
  "subscribe": {
    "request_id": "unique-request-id",
    "subscription_type": 1,
    "market_slugs": ["market-slug-1", "market-slug-2"]
  }
}
```

### Subscription Types

**Private WebSocket (`/v1/ws/private`):**

| Value | Type             | Description      |
| ----- | ---------------- | ---------------- |
| 1     | ORDER            | Order updates    |
| 3     | POSITION         | Position changes |
| 4     | ACCOUNT\_BALANCE | Balance updates  |

**Markets WebSocket (`/v1/ws/markets`):**

| Value | Type               | Description                      |
| ----- | ------------------ | -------------------------------- |
| 1     | MARKET\_DATA       | Full order book and market stats |
| 2     | MARKET\_DATA\_LITE | Lightweight price data           |
| 3     | TRADE              | Real-time trade notifications    |

### Response Format

```json theme={null}
{
  "request_id": "unique-request-id",
  "subscription_type": 1,
  "order_subscription_snapshot": {
    "orders": [...],
    "eof": true
  }
}
```

## Heartbeats

The server sends periodic heartbeat messages to keep the connection alive:

```json theme={null}
{
  "heartbeat": {}
}
```

Clients should respond to heartbeats or implement their own keep-alive mechanism.

## Error Handling

If a subscription request fails, the response will include an error field:

```json theme={null}
{
  "request_id": "unique-request-id",
  "error": "Error description"
}
```

## Unsubscribing

To unsubscribe from a stream:

```json theme={null}
{
  "unsubscribe": {
    "request_id": "original-request-id"
  }
}
```

## Best Practices

1. **Use unique request IDs** - Track subscriptions with unique identifiers
2. **Handle reconnection** - Implement automatic reconnection with exponential backoff
3. **Process messages in order** - Messages are delivered in sequence
4. **Monitor heartbeats** - Reconnect if heartbeats stop
5. **Limit subscriptions** - Only subscribe to markets you need
