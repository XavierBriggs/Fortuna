> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Rate Limits

> API rate limits and best practices for integration

The Polymarket US API enforces rate limits to ensure fair usage and system stability. All limits are **per participant firm** unless stated otherwise.

## REST API

### Trading Endpoints

REST API traffic is subject to a **firm-wide cap of 100 requests per second per firm**, averaged over a 1-minute window. This means short bursts above 100 req/sec are permitted as long as the average stays within budget. Some REST endpoints may also enforce additional lower limits, such as the query/report endpoints listed below.

### Query / Report Endpoints

In addition to the firm-wide REST cap above, these read-heavy endpoints have lower per-firm limits. Cache responses where noted.

| Endpoint                 | Limit         | Notes                                      |
| ------------------------ | ------------- | ------------------------------------------ |
| `GetTradeStats`          | 60 req/min    | Heavy aggregation query                    |
| `ListInstruments`        | 6 req/min     | Static data - cache client-side            |
| `ListSymbols`            | 6 req/min     | Static data - cache client-side            |
| `GetOrderBook`           | 12 req/min    | Prefer streaming for real-time data        |
| `GetBBO`                 | 12 req/min    | Prefer streaming for real-time data        |
| `SearchOrders`           | 12 req/min    | Use filters to narrow results              |
| `SearchExecutions`       | 12 req/min    | Use filters to narrow results              |
| `SearchTrades`           | 12 req/min    | Use filters to narrow results              |
| `ListPositionValuations` | \~0.5 req/min | Mark-to-market calculation; call sparingly |

### Public (Unauthenticated) Endpoints

| Limit                   | Value     |
| ----------------------- | --------- |
| Max requests per second | 20 per IP |

## gRPC Streaming

| Setting                                             | Value       |
| --------------------------------------------------- | ----------- |
| Max concurrent streams per firm                     | 20          |
| Ingress message rate (per firm, across all streams) | 100 msg/sec |
| Egress (server to client)                           | Unlimited   |

Ingress rate is averaged over a 1-minute window, allowing short bursts. Exceeding the average limit will result in throttled or rejected messages. This limit applies to all participants.

## FIX Protocol

| Setting              | Value                   |
| -------------------- | ----------------------- |
| Ingress message rate | 150 msg/sec per session |

FIX rate limiting is enforced at the FIX gateway level. This limit applies to all participants.

## Summary

| Protocol                 | Scope                  | Limit                            |
| ------------------------ | ---------------------- | -------------------------------- |
| REST - trading (orders)  | Per firm               | 100 req/sec (1-min avg)          |
| REST - query endpoints   | Per firm, per endpoint | 0.5–60 req/min (see table above) |
| REST - public/unauth     | Per IP                 | 20 req/sec                       |
| gRPC streaming (ingress) | Per firm (all streams) | 100 msg/sec (1-min avg)          |
| gRPC streaming (egress)  | Per firm               | Unlimited                        |
| FIX                      | Per session            | 150 msg/sec                      |

## Rate Limit Response

When rate limited, the REST API returns:

```json theme={null}
{
  "code": 8,
  "message": "rate limit exceeded",
  "details": []
}
```

**HTTP Status:** `429 Too Many Requests`

### Retry Strategy

When receiving a 429 response:

1. Stop making requests immediately
2. Wait 1 second before retrying
3. Implement exponential backoff for repeated 429s
4. Consider reducing your request rate

```python theme={null}
import time

def make_request_with_retry(url, headers, max_retries=3):
    for attempt in range(max_retries):
        response = requests.get(url, headers=headers)

        if response.status_code == 429:
            wait_time = 2 ** attempt  # 1, 2, 4 seconds
            print(f"Rate limited. Waiting {wait_time}s...")
            time.sleep(wait_time)
            continue

        return response

    raise Exception("Max retries exceeded")
```

## Best Practices

### Use Streaming Instead of Polling

The API is designed as a **streaming-first** system. Instead of repeatedly polling for updates, subscribe to real-time streams:

| Don't Poll                                   | Use Streaming Instead                      |
| -------------------------------------------- | ------------------------------------------ |
| Repeated calls to `/v1/report/orders/search` | `CreateOrderSubscription` gRPC stream      |
| Repeated calls to `/v1/positions`            | `CreatePositionSubscription` gRPC stream   |
| Repeated calls to `/v1/orderbook`            | `CreateMarketDataSubscription` gRPC stream |

<Info>
  Streaming connections don't count against the REST rate limit. One streaming connection can replace hundreds of polling requests.
</Info>

### Cache Reference Data

Reference data (instruments, symbols, metadata) changes infrequently - `ListInstruments` and `ListSymbols` are limited to just 6 req/min. Cache responses locally:

```python theme={null}
class InstrumentCache:
    def __init__(self):
        self.instruments = {}
        self.last_refresh = None

    def get_instrument(self, symbol):
        # Refresh cache every 5 minutes
        if self._needs_refresh():
            self._refresh_instruments()
        return self.instruments.get(symbol)

    def _needs_refresh(self):
        if not self.last_refresh:
            return True
        return (time.time() - self.last_refresh) > 300

    def _refresh_instruments(self):
        response = api.list_instruments()
        for inst in response.instruments:
            self.instruments[inst.symbol] = inst
        self.last_refresh = time.time()
```

### Batch Operations

Where possible, batch your operations instead of making individual requests:

* Use `SearchOrders` with filters instead of fetching orders one by one
* Use `ListInstruments` with symbol filters instead of individual lookups
* Subscribe to multiple symbols in a single streaming connection

### Design Around `ListPositionValuations`

`ListPositionValuations` is the most restrictive endpoint at \~0.5 req/min. It performs mark-to-market calculations across all positions and is intended for periodic reporting (e.g., end-of-day P\&L), not real-time monitoring.

## Monitoring Your Usage

Track your request patterns to stay within limits:

```python theme={null}
import time
from collections import deque

class RateLimiter:
    def __init__(self, max_requests=100, window_seconds=1):
        self.max_requests = max_requests
        self.window = window_seconds
        self.requests = deque()

    def can_make_request(self):
        now = time.time()
        # Remove old requests outside the window
        while self.requests and self.requests[0] < now - self.window:
            self.requests.popleft()
        return len(self.requests) < self.max_requests

    def record_request(self):
        self.requests.append(time.time())

    def wait_if_needed(self):
        while not self.can_make_request():
            time.sleep(0.05)  # 50ms
        self.record_request()
```

## Abuse Prevention

Patterns that may result in temporary or permanent restrictions:

* Sustained requests above the rate limit
* Polling for data available via streaming
* Requesting the same unchanged data repeatedly
* Automated retry loops without backoff

<Warning>
  Abuse of the API may result in temporary or permanent restrictions on your API credentials. Contact [onboarding@polymarket.us](mailto:onboarding@polymarket.us) if you need higher limits for legitimate use cases.
</Warning>

## Troubleshooting Rate Limits

### Consistently Hitting Limits

If you're consistently receiving 429 errors:

* Reduce request frequency
* Batch multiple operations where possible
* Cache responses that don't change frequently (reference data, instrument lists)
* Use streaming endpoints instead of polling
* Contact support to discuss higher rate limits for production use

### Need Higher Limits

For production use cases requiring higher limits:

1. Document your use case and expected volume
2. Contact support at [onboarding@polymarket.us](mailto:onboarding@polymarket.us)
3. Provide environment (dev, preprod, prod)
4. Specify which endpoints you need higher limits for

## Next Steps

<CardGroup cols={2}>
  <Card title="gRPC Streaming" icon="bolt" href="/streaming-endpoints/grpc-overview">
    Replace polling with real-time streams
  </Card>

  <Card title="Authentication" icon="key" href="/trader-guide/authentication-troubleshooting">
    Set up API authentication
  </Card>
</CardGroup>
