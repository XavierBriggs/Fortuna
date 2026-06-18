> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Market Data

> Access real-time and historical market data from the Polymarket Exchange

## Overview

Public market data available to all users:

* **Best Bid/Offer (BBO)** - Current best prices
* **L2 Order Book** - Full depth of market

Private trade data (requires account access):

* **Trades** - Executed transactions for your account

## Required Scopes

| Scope               | Data Access                |
| ------------------- | -------------------------- |
| `read:marketdata`   | BBO, streaming market data |
| `read:l2marketdata` | Full L2 order book depth   |

## REST API Endpoints

### Get Best Bid/Offer

Retrieve the current best bid and offer for a symbol:

```bash theme={null}
curl -X GET "https://api.preprod.polymarketexchange.com/v1/orderbook/{symbol}/bbo" \
  -H "Authorization: Bearer YOUR_ACCESS_TOKEN"
```

**Response:**

```json theme={null}
{
  "symbol": "aec-nfl-buf-nyj-2025-01-15",
  "bestBid": {
    "px": "650",
    "qty": "1000"
  },
  "bestOffer": {
    "px": "670",
    "qty": "500"
  },
  "spread": "20",
  "midPrice": "660",
  "state": "INSTRUMENT_STATE_OPEN",
  "transactTime": "2025-01-15T10:30:00Z"
}
```

### Get L2 Order Book

Retrieve the full order book depth:

```bash theme={null}
curl -X GET "https://api.preprod.polymarketexchange.com/v1/orderbook/{symbol}" \
  -H "Authorization: Bearer YOUR_ACCESS_TOKEN"
```

**Response:**

```json theme={null}
{
  "symbol": "aec-nfl-buf-nyj-2025-01-15",
  "bids": [
    {"px": "650", "qty": "1000"},
    {"px": "640", "qty": "2000"},
    {"px": "630", "qty": "1500"}
  ],
  "offers": [
    {"px": "670", "qty": "500"},
    {"px": "680", "qty": "800"},
    {"px": "690", "qty": "1200"}
  ],
  "state": "INSTRUMENT_STATE_OPEN",
  "transactTime": "2025-01-15T10:30:00Z"
}
```

<Info>
  For OHLC/candlestick data generation, see the [Candlestick Data Guide](/data-guide/candlestick-data).
</Info>

## Streaming Market Data (gRPC)

For real-time updates, use the gRPC [Market Data Stream](/streaming-endpoints/market-data-stream).

### Subscribe to Market Data

```python theme={null}
import grpc
from polymarket_pb2 import MarketDataRequest
from polymarket_pb2_grpc import MarketDataServiceStub

# Create channel with credentials
channel = grpc.secure_channel(
    'grpc.preprod.polymarketexchange.com:443',
    grpc.ssl_channel_credentials()
)

stub = MarketDataServiceStub(channel)

# Subscribe to symbols
request = MarketDataRequest(
    symbols=["aec-nfl-buf-nyj-2025-01-15", "aec-nba-bos-lal-2025-01-20"]
)

# Stream updates
for update in stub.Subscribe(request, metadata=[('authorization', f'Bearer {token}')]):
    print(f"Symbol: {update.symbol}, Bid: {update.bid}, Ask: {update.ask}")
```

## Best Practices

### Rate Limits

* REST endpoints are subject to [rate limits](/trader-guide/rate-limits)
* Use streaming (gRPC) for real-time data to reduce API calls
* Cache reference data locally

### Connection Management

* Implement reconnection logic for streaming connections
* Handle network interruptions gracefully
* Use heartbeats to detect connection issues

### Data Handling

* Validate timestamps to detect stale data
* Handle gaps in sequence numbers appropriately
* Store historical data locally for analysis
