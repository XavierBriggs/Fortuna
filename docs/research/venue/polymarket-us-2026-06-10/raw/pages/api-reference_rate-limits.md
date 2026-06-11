> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Rate Limits

> API rate limits and how to stay within them.

Rate limits are enforced per API key. Exceeding them returns `429 Too Many Requests`.

***

## Limits

The Retail API enforces a global rate limit of **20 requests per second** per API key across all endpoints.

| Limit                                    | Value                              |
| ---------------------------------------- | ---------------------------------- |
| **Global (all authenticated endpoints)** | 20 requests per second per API key |
| **Public (unauthenticated)**             | 20 requests per second per IP      |

***

## When you're rate limited

```json theme={null}
{ "status": 429, "message": "Too Many Requests" }
```

Stop immediately, wait at least 1 second, then retry with exponential backoff:

```python theme={null}
import time

def make_request_with_retry(fn, max_retries=3):
    for attempt in range(max_retries):
        response = fn()
        if response.status_code != 429:
            return response
        time.sleep(2 ** attempt)
    raise Exception("Max retries exceeded")
```

***

## Use WebSocket instead of polling

The single most effective way to stay within limits is to stop polling and use WebSocket streams. One persistent connection replaces hundreds of repeated REST calls.

| Don't poll                               | Use instead                                             |
| ---------------------------------------- | ------------------------------------------------------- |
| `GET /v1/orders/open` repeatedly         | `/v1/ws/private` - `SUBSCRIPTION_TYPE_ORDER`            |
| `GET /v1/portfolio/positions` repeatedly | `/v1/ws/private` - `SUBSCRIPTION_TYPE_POSITION`         |
| `GET /v1/account/balances` repeatedly    | `/v1/ws/private` - `SUBSCRIPTION_TYPE_ACCOUNT_BALANCE`  |
| `GET /v1/markets/{slug}/bbo` repeatedly  | `/v1/ws/markets` - `SUBSCRIPTION_TYPE_MARKET_DATA_LITE` |
| `GET /v1/markets/{slug}/book` repeatedly | `/v1/ws/markets` - `SUBSCRIPTION_TYPE_MARKET_DATA`      |

***

## Cache reference data

Market and event metadata changes infrequently. Fetch it once on startup and refresh periodically rather than on every request.

```python theme={null}
import time

class MarketCache:
    def __init__(self, client, ttl=300):
        self._client = client
        self._cache = {}
        self._ttl = ttl
        self._last_refresh = None

    def get(self, slug):
        if self._needs_refresh():
            self._refresh()
        return self._cache.get(slug)

    def _needs_refresh(self):
        return not self._last_refresh or (time.time() - self._last_refresh) > self._ttl

    def _refresh(self):
        markets = self._client.markets.list({"limit": 100, "active": True})
        self._cache = {m["slug"]: m for m in markets["markets"]}
        self._last_refresh = time.time()
```

***

## For automated systems

If you're running an automated trading system and need higher limits for production:

1. Document your use case and expected request volume
2. Email [support@polymarket.us](mailto:support@polymarket.us)
3. Include which endpoints you need higher limits for
