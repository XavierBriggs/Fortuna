> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Market Data Streaming

> Real-time market data streaming via gRPC with Python

Subscribe to real-time market data updates including order book depth, instrument states, and trade statistics using Python and gRPC.

<Info>
  **No Participant ID Required**

  This streaming endpoint only requires Auth0 JWT authentication with `read:marketdata` scope. You do not need to provide the `x-participant-id` header or complete KYC onboarding to access market data streams.
</Info>

## Service Definition

**Service:** `polymarket.v1.MarketDataSubscriptionAPI`

The service provides two streaming methods:

1. **`CreateMarketDataSubscription`** - Server-side streaming with fixed symbols at subscription time
2. **`BiDirectionalStreamMarketData`** - Bidirectional streaming with dynamic symbol management

```protobuf theme={null}
service MarketDataSubscriptionAPI {
    // Server-side streaming - symbols fixed at subscription time
    rpc CreateMarketDataSubscription(CreateMarketDataSubscriptionRequest)
        returns (stream CreateMarketDataSubscriptionResponse);

    // Bidirectional streaming - dynamically add/remove symbols
    rpc BiDirectionalStreamMarketData(stream BiDirectionalStreamMarketDataRequest)
        returns (stream BiDirectionalStreamMarketDataResponse);
}
```

<Tip>
  **When to use which method:**

  * Use `CreateMarketDataSubscription` for simple subscriptions where symbols are known upfront
  * Use `BiDirectionalStreamMarketData` when you need to dynamically add/remove symbols without reconnecting
</Tip>

## Request Parameters

### CreateMarketDataSubscriptionRequest

| Field           | Type        | Required | Description                                                                                                   |
| --------------- | ----------- | -------- | ------------------------------------------------------------------------------------------------------------- |
| `symbols`       | `list[str]` | No       | List of symbols to subscribe to. Empty list subscribes to all instruments.                                    |
| `unaggregated`  | `bool`      | No       | If `True`, receive raw order book. If `False` (default), receive aggregated book by price level.              |
| `depth`         | `int`       | No       | Number of price levels to include in order book. Default: `10`                                                |
| `snapshot_only` | `bool`      | No       | If `True`, receive only initial snapshot then close stream. If `False` (default), receive continuous updates. |

<Warning>
  **Symbol Limit**: Each gRPC stream is limited to **1000 symbols maximum**. If you need to subscribe to more than 1000 symbols, create multiple streams.
</Warning>

### Example Request

```python theme={null}
from polymarket.v1 import marketdatasubscription_pb2

# Subscribe to specific symbols
request = marketdatasubscription_pb2.CreateMarketDataSubscriptionRequest(
    symbols=["tec-nfl-sbw-2026-02-08-kc", "tec-nfl-sbw-2026-02-08-phi"],
    unaggregated=False,
    depth=10,
    snapshot_only=False
)

# Subscribe to all symbols
request = marketdatasubscription_pb2.CreateMarketDataSubscriptionRequest(
    symbols=[],  # Empty = all symbols
    depth=20
)
```

## Response Messages

The stream returns `CreateMarketDataSubscriptionResponse` messages with two possible event types:

### 1. Heartbeat Messages

Keep-alive messages to confirm connection is active.

```python theme={null}
if response.HasField('heartbeat'):
    print(f"[{datetime.now().strftime('%H:%M:%S')}] Heartbeat received")
```

<Info>
  If you stop receiving heartbeats, the connection may be stale. Consider reconnecting.
</Info>

### 2. Market Data Updates

Real-time market data changes.

```python theme={null}
if response.HasField('update'):
    update = response.update
    print(f"Symbol: {update.symbol}")
    print(f"State: {update.state}")
    print(f"Bids: {len(update.bids)}")
    print(f"Offers: {len(update.offers)}")
```

## Update Message Structure

### Fields

| Field           | Type              | Description                                           |
| --------------- | ----------------- | ----------------------------------------------------- |
| `symbol`        | `str`             | Instrument symbol (e.g., "tec-nfl-sbw-2026-02-08-kc") |
| `bids`          | `list[BookEntry]` | Bid side of order book (buy orders)                   |
| `offers`        | `list[BookEntry]` | Offer/ask side of order book (sell orders)            |
| `state`         | `InstrumentState` | Current trading state of instrument (optional)        |
| `stats`         | `InstrumentStats` | Market statistics (optional)                          |
| `transact_time` | `Timestamp`       | Server timestamp of update                            |
| `book_hidden`   | `bool`            | If `True`, order book is hidden                       |

<Tip>
  **Instrument State Tracking:**
  The `state` field in `MarketDataUpdate` is optional and should not be relied upon for tracking instrument state changes. The preferred approach is to use `ListInstruments` to get and cache the initial state for each instrument, then subscribe to the **instrument state change subscription** for real-time state updates.
</Tip>

### BookEntry Structure

Each price level in the order book contains:

| Field | Type    | Description                                   |
| ----- | ------- | --------------------------------------------- |
| `px`  | `int64` | **Price as integer** (divide by price\_scale) |
| `qty` | `int64` | Aggregate quantity at this price level        |

<Warning>
  **Price Representation:**
  All prices are `int64` values. Divide by the instrument's `price_scale` to get the decimal value.

  `price_scale` varies by instrument. Query instrument metadata to get the correct value.

  ```python theme={null}
  px = bid.px / price_scale # Convert from price representation
  print(f"${px:.4f}")
  ```
</Warning>

### InstrumentStats Structure

Market statistics include:

| Field                                 | Type        | Description                                                                                                                                           |
| ------------------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `last_trade_px`                       | `int64`     | Last trade price (÷ price\_scale)                                                                                                                     |
| `last_trade_qty`                      | `int64`     | Quantity of the most recent trade. Populated after any trade executes on the instrument.                                                              |
| `open_px`                             | `int64`     | Opening price (÷ price\_scale)                                                                                                                        |
| `high_px`                             | `int64`     | High price of session (÷ price\_scale)                                                                                                                |
| `low_px`                              | `int64`     | Low price of session (÷ price\_scale)                                                                                                                 |
| `close_px`                            | `int64`     | Closing price (÷ price\_scale)                                                                                                                        |
| `shares_traded`                       | `int64`     | Total volume traded                                                                                                                                   |
| `open_interest`                       | `int64`     | Current open interest                                                                                                                                 |
| `notional_traded`                     | `int64`     | Total notional value traded                                                                                                                           |
| `settlement_px`                       | `int64`     | Settlement/resolution price (÷ price\_scale). Only populated when instrument state is `CLOSED`, `TERMINATED`, or `EXPIRED`.                           |
| `settlement_set_time`                 | `Timestamp` | Timestamp when the settlement price was set. Only populated when instrument is in a settled state (`CLOSED`, `TERMINATED`, or `EXPIRED`).             |
| `settlement_preliminary`              | `bool`      | If `true`, settlement price may still change (awaiting final approval). If `false`, settlement is final and positions will be resolved at this price. |
| `settlement_price_calculation_method` | `string`    | Method used to calculate the settlement price. See below for values.                                                                                  |
| `settlement_price_calculation_text`   | `string`    | Free-form text describing the outcome that determined the settlement (e.g., "Buffalo Bills win", "Kansas City Chiefs win").                           |

<Note>
  Stats fields use protobuf `oneof`, so they may not always be present. Always check with `HasField()` before accessing.

  ```python theme={null}
  if update.HasField('stats') and update.stats.HasField('last_trade_px'):
      last_px = update.stats.last_trade_px / price_scale
  ```
</Note>

#### Settlement Fields

**Settlement Price Calculation Methods:**

| Value                                              | Description                                                                                               |
| -------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_1` | Settlement via event resolution. This is the primary method for Polymarket event markets.                 |
| Other methods                                      | Used for daily settlement of futures-style products (VWAP-based). These can be ignored for event markets. |

<Tip>
  For Polymarket event markets, `settlement_price_calculation_method` will be `SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_1`. If you see a different method, you can treat it as a daily mark rather than final resolution.
</Tip>

**Example: Settled Market (YES wins)**

```json theme={null}
{
  "symbol": "aec-nfl-buf-kc-2026-01-26",
  "state": "INSTRUMENT_STATE_EXPIRED",
  "stats": {
    "settlement_px": 1000,
    "settlement_set_time": "2026-01-27T03:42:11Z",
    "settlement_preliminary": false,
    "settlement_price_calculation_method": "SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_1",
    "settlement_price_calculation_text": "Buffalo Bills win"
  }
}
```

In this example, `settlement_px = 1000` with a `price_scale` of 1000 equals \$1.00 (YES won, Buffalo won the game).

**Example: Settled Market (NO wins)**

```json theme={null}
{
  "symbol": "aec-nfl-buf-kc-2026-01-26",
  "state": "INSTRUMENT_STATE_EXPIRED",
  "stats": {
    "settlement_px": 0,
    "settlement_preliminary": false,
    "settlement_price_calculation_method": "SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_1",
    "settlement_price_calculation_text": "Kansas City Chiefs win"
  }
}
```

In this example, `settlement_px = 0` equals \$0.00 (NO won, Kansas City won so Buffalo did not win).

## Instrument States

<Tip>
  **Preferred approach:** Use `ListInstruments` to get and cache the initial state for each instrument, then subscribe to the instrument state change stream for ongoing state updates. The `state` field on `MarketDataUpdate` is now optional.
</Tip>

### Primary State Flow

| State                                               | Value | Description                                                                                                                                                                                                      |
| --------------------------------------------------- | ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `INSTRUMENT_STATE_PENDING`                          | 8     | Initial state for a newly created instrument which has not yet begun trading.                                                                                                                                    |
| `INSTRUMENT_STATE_OPEN`                             | 1     | In this state, the instrument is open for continuous order entry and matching.                                                                                                                                   |
| `INSTRUMENT_STATE_CLOSED`                           | 0     | In this state, orders can not be entered, modified, or canceled, and no matching occurs. Any existing Day orders will be expired.                                                                                |
| `INSTRUMENT_STATE_EXPIRED`                          | 4     | An instrument moves to this state when its Expiration Date/Time is reached. In this state, any resting orders are expired and no new orders can be entered.                                                      |
| `INSTRUMENT_STATE_TERMINATED`                       | 5     | When an instrument's Termination Date is reached, the order book is removed from the matching engine, orders are canceled, and positions are closed. Historical data will still remain in Polymarket US ledgers. |

### Exception States

| State                                               | Value | Description                                                                                   |
| --------------------------------------------------- | ----- | --------------------------------------------------------------------------------------------- |
| `INSTRUMENT_STATE_SUSPENDED`                        | 3     | Orders can be canceled but no matching occurs, and no order entry or modification is allowed. |
| `INSTRUMENT_STATE_HALTED`                           | 6     | This state is similar to SUSPENDED, with the exception that orders cannot be canceled.        |

### Other Possible States

| State                                               | Value | Description                                                                                                                                                                                                                                                                                       |
| --------------------------------------------------- | ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `INSTRUMENT_STATE_PREOPEN`                          | 2     | Orders can be entered and modified, but no matching occurs. When the instrument transitions to an OPEN state, the orders entered during PREOPEN will match at a single opening price that is automatically determined by an algorithm that is designed to maximize the volume traded at the open. |
| `INSTRUMENT_STATE_MATCH_AND_CLOSE_AUCTION`          | 7     | This state is similar to PREOPEN, with the exception that matching will occur upon the transition of this state to any other state. This state is useful if you want matching to occur at the end of the state, but you don't want the instrument to be open after.                               |

```python theme={null}
from polymarket.v1 import refdata_pb2

# Get state name
state_name = refdata_pb2.InstrumentState.Name(update.state)
print(f"State: {state_name}")
```

## Complete Example (from stream.py)

This example matches the implementation from the Python examples repository:

```python theme={null}
import grpc
import requests
from datetime import datetime, timedelta
from typing import Optional
from polymarket.v1 import marketdatasubscription_pb2
from polymarket.v1 import marketdatasubscription_pb2_grpc
from polymarket.v1 import refdata_pb2


class PolymarketStreamer:
    def __init__(self, base_url: str = "https://rest.preprod.polymarketexchange.com",
                 grpc_server: str = "grpc-api.preprod.polymarketexchange.com:443"):
        self.base_url = base_url
        self.grpc_server = grpc_server
        self.access_token: Optional[str] = None
        self.refresh_token: Optional[str] = None
        self.access_expiration: Optional[datetime] = None
        self.price_scales: dict = {}  # symbol -> price_scale cache

    def get_price_scale(self, symbol: str) -> int:
        """Get price_scale for symbol from cache. Populate via list_instruments API."""
        # WARNING: Replace this with actual API lookup. Do not rely on default.
        return self.price_scales.get(symbol, 1000)

    def login(self, auth0_domain: str, client_id: str, private_key_path: str, audience: str) -> dict:
        """Authenticate using Private Key JWT and store the access token."""
        import jwt
        import uuid
        from cryptography.hazmat.primitives import serialization

        # Load private key
        with open(private_key_path, 'rb') as f:
            private_key = serialization.load_pem_private_key(f.read(), password=None)

        # Create JWT assertion
        now = int(datetime.now().timestamp())
        claims = {
            "iss": client_id,
            "sub": client_id,
            "aud": f"https://{auth0_domain}/oauth/token",
            "iat": now,
            "exp": now + 300,
            "jti": str(uuid.uuid4()),
        }
        assertion = jwt.encode(claims, private_key, algorithm="RS256")

        # Exchange for access token
        response = requests.post(
            f"https://{auth0_domain}/oauth/token",
            json={
                "client_id": client_id,
                "client_assertion_type": "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
                "client_assertion": assertion,
                "audience": audience,
                "grant_type": "client_credentials"
            }
        )
        response.raise_for_status()

        token_data = response.json()
        self.access_token = token_data["access_token"]
        # Set expiration with 30-second buffer (tokens expire in 180 seconds)
        expires_in = token_data.get("expires_in", 180)
        self.access_expiration = datetime.now() + timedelta(seconds=expires_in - 30)

        return token_data

    def stream_market_data(self, symbols: list, unaggregated: bool = False,
                          depth: int = 10, snapshot_only: bool = False):
        """Stream market data for the given symbols using gRPC."""
        if not self.access_token:
            raise ValueError("Not authenticated. Please login first.")

        # Create credentials
        credentials = grpc.ssl_channel_credentials()

        # Create channel
        channel = grpc.secure_channel(self.grpc_server, credentials)

        # Create stub
        stub = marketdatasubscription_pb2_grpc.MarketDataSubscriptionAPIStub(channel)

        # Create request
        request = marketdatasubscription_pb2.CreateMarketDataSubscriptionRequest(
            symbols=symbols,
            unaggregated=unaggregated,
            depth=depth,
            snapshot_only=snapshot_only
        )

        # Set up metadata with authorization
        metadata = [
            ('authorization', f'Bearer {self.access_token}')
        ]

        try:
            print(f"Starting market data stream for symbols: {symbols}")
            print(f"Parameters: unaggregated={unaggregated}, depth={depth}, snapshot_only={snapshot_only}")
            print("-" * 60)

            # Start streaming
            response_stream = stub.CreateMarketDataSubscription(request, metadata=metadata)

            for response in response_stream:
                self._process_market_data_response(response)

        except grpc.RpcError as e:
            print(f"gRPC error: {e.code()} - {e.details()}")
            raise
        except KeyboardInterrupt:
            print("\nStream interrupted by user")
        finally:
            channel.close()

    def _process_market_data_response(self, response):
        """Process and display market data response."""
        if response.HasField('heartbeat'):
            print(f"[{datetime.now().strftime('%H:%M:%S')}] Heartbeat received")

        elif response.HasField('update'):
            update = response.update
            print(f"\n[{datetime.now().strftime('%H:%M:%S')}] Market Update for {update.symbol}")

            # Display instrument state
            state_name = refdata_pb2.InstrumentState.Name(update.state)
            print(f"  State: {state_name}")

            # Get price_scale from instrument metadata (via list_instruments API)
            price_scale = self.get_price_scale(update.symbol)

            # Display order book
            if update.bids:
                print("  Bids:")
                for i, bid in enumerate(update.bids[:5]):  # Show top 5 bids
                    px = bid.px / price_scale  # Convert from price representation
                    qty = bid.qty
                    print(f"    [{i+1}] ${px:.4f} x {qty}")

            if update.offers:
                print("  Offers:")
                for i, offer in enumerate(update.offers[:5]):  # Show top 5 offers
                    px = offer.px / price_scale # Convert from price representation
                    qty = offer.qty
                    print(f"    [{i+1}] ${px:.4f} x {qty}")

            # Display stats if available
            if update.HasField('stats'):
                stats = update.stats
                print("  Stats:")
                if stats.HasField('last_trade_px'):
                    last_px = stats.last_trade_px / price_scale
                    print(f"    Last Trade: ${last_px:.4f}")
                if stats.HasField('open_px'):
                    open_px = stats.open_px / price_scale
                    print(f"    Open: ${open_px:.4f}")
                if stats.HasField('high_px'):
                    high_px = stats.high_px / price_scale
                    print(f"    High: ${high_px:.4f}")
                if stats.HasField('low_px'):
                    low_px = stats.low_px / price_scale
                    print(f"    Low: ${low_px:.4f}")
                if stats.HasField('shares_traded'):
                    print(f"    Shares Traded: {stats.shares_traded}")
                if stats.HasField('open_interest'):
                    print(f"    Open Interest: {stats.open_interest}")

            print("-" * 60)


# Usage
if __name__ == "__main__":
    streamer = PolymarketStreamer()

    # Login using Private Key JWT
    streamer.login(
        auth0_domain="pmx-preprod.us.auth0.com",
        client_id="your_client_id",
        private_key_path="private_key.pem",
        audience="https://api.preprod.polymarketexchange.com"
    )

    # Stream market data
    streamer.stream_market_data(
        symbols=["tec-nfl-sbw-2026-02-08-kc"],
        depth=10
    )
```

***

## Bidirectional Streaming

The `BiDirectionalStreamMarketData` RPC allows you to dynamically add and remove symbols during the subscription lifetime without reconnecting.

### Request Messages

Send `BiDirectionalStreamMarketDataRequest` messages to manage your subscription:

```protobuf theme={null}
message BiDirectionalStreamMarketDataRequest {
    oneof command {
        SubscribeCommand subscribe = 1;     // Add symbols
        UnsubscribeCommand unsubscribe = 2; // Remove symbols
        KeepAliveCommand keepalive = 7;     // Application-level keepalive (no-op)
    }
    bool unaggregated = 3;  // Options (read from first request only)
    int32 depth = 4;
    bool snapshot_only = 5;
    bool slow_consumer_skip_to_head = 6;
}

message SubscribeCommand {
    repeated string symbols = 1;
}

message UnsubscribeCommand {
    repeated string symbols = 1;
}

// Application-level keepalive for BiDirectionalStreamMarketData. Sending one
// puts a client-to-server frame on the wire without modifying subscription
// state; the server returns no response.
message KeepAliveCommand {}
```

### Commands

| Command       | Field | Description                                                                                                                                         |
| ------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `subscribe`   | 1     | Add the listed symbols to the active subscription.                                                                                                  |
| `unsubscribe` | 2     | Remove the listed symbols from the active subscription.                                                                                             |
| `keepalive`   | 7     | Application-level keepalive. Server handles it as a no-op (no response, no state change). Safe to send any time after the first `SubscribeCommand`. |

<Warning>
  **ALB Idle Timeout & Keepalives**

  Long-lived bidirectional streams that send no client-to-server traffic for \~1 hour will be terminated with `RST_STREAM` by the AWS Application Load Balancer (idle timeout is **3600 seconds** by default).

  To keep an otherwise-quiet stream alive, send a `KeepAliveCommand` every **30–60 minutes**. The server treats it as a no-op: no response, no change to subscription state or symbol list.

  This only applies to `BiDirectionalStreamMarketData`. Server-streaming RPCs like `CreateMarketDataSubscription` are not affected.
</Warning>

### Response Messages

The stream returns `BiDirectionalStreamMarketDataResponse` messages with four possible event types:

| Event Type           | Description                                   |
| -------------------- | --------------------------------------------- |
| `heartbeat`          | Keep-alive message                            |
| `update`             | Market data update (same as server-streaming) |
| `subscription_ack`   | Acknowledgment of subscribe/unsubscribe       |
| `subscription_error` | Error for subscription operations             |

```protobuf theme={null}
message BiDirectionalStreamMarketDataResponse {
    oneof event {
        Heartbeat heartbeat = 1;
        MarketDataUpdate update = 2;
        SubscriptionAck subscription_ack = 3;
        SubscriptionError subscription_error = 4;
    }
}

message SubscriptionAck {
    repeated string symbols_added = 1;    // Symbols added in this operation
    repeated string symbols_removed = 2;  // Symbols removed in this operation
    repeated string active_symbols = 3;   // All currently active symbols
}

message SubscriptionError {
    string error_code = 1;     // e.g., "INVALID_SYMBOL", "ALREADY_SUBSCRIBED"
    string message = 2;        // Human-readable error message
    repeated string symbols = 3; // Symbols that caused the error
}
```

### Error Codes

| Code                 | Description                                                 |
| -------------------- | ----------------------------------------------------------- |
| `INVALID_SYMBOL`     | Symbol does not exist or is not valid                       |
| `ALREADY_SUBSCRIBED` | Already subscribed to the symbol                            |
| `NOT_SUBSCRIBED`     | Trying to unsubscribe from a symbol not in the subscription |

### Python Example

```python theme={null}
import grpc
import threading
import queue
from polymarket.v1 import marketdatasubscription_pb2
from polymarket.v1 import marketdatasubscription_pb2_grpc


def request_generator(request_queue):
    """Generator that yields requests from a queue."""
    while True:
        try:
            request = request_queue.get(timeout=0.1)
            if request is None:
                break
            yield request
        except queue.Empty:
            continue


def stream_bidi_market_data(stub, access_token):
    """Demonstrate bidirectional market data streaming."""
    request_queue = queue.Queue()

    # Set up metadata with authorization
    metadata = [('authorization', f'Bearer {access_token}')]

    # Start bidirectional stream
    response_stream = stub.BiDirectionalStreamMarketData(
        request_generator(request_queue),
        metadata=metadata
    )

    # Subscribe to initial symbol
    request_queue.put(marketdatasubscription_pb2.BiDirectionalStreamMarketDataRequest(
        subscribe=marketdatasubscription_pb2.SubscribeCommand(
            symbols=["tec-nfl-sbw-2026-02-08-kc"]
        ),
        depth=10
    ))

    # Process responses
    for response in response_stream:
        if response.HasField('subscription_ack'):
            ack = response.subscription_ack
            print(f"Subscription ACK - Active: {list(ack.active_symbols)}")
        elif response.HasField('subscription_error'):
            err = response.subscription_error
            print(f"Error: {err.error_code} - {err.message}")
        elif response.HasField('update'):
            update = response.update
            print(f"Update: {update.symbol} - {len(update.bids)} bids")
        elif response.HasField('heartbeat'):
            print("Heartbeat")

        # Dynamically add another symbol after receiving first update
        # request_queue.put(marketdatasubscription_pb2.BiDirectionalStreamMarketDataRequest(
        #     subscribe=marketdatasubscription_pb2.SubscribeCommand(
        #         symbols=["tec-nfl-sbw-2026-02-08-phi"]
        #     )
        # ))


def keepalive_worker(request_queue, interval_seconds: int = 1800):
    """Periodically push a KeepAliveCommand onto the request queue (every 30 minutes
    by default) so the connection stays inside the AWS ALB idle window."""
    import time
    while True:
        time.sleep(interval_seconds)
        request_queue.put(marketdatasubscription_pb2.BiDirectionalStreamMarketDataRequest(
            keepalive=marketdatasubscription_pb2.KeepAliveCommand()
        ))
```

### Go Example

```go theme={null}
import (
    "context"
    polymarketv1 "github.com/polymarket/client-sample-code/go/gen/polymarket/v1"
)

func streamBidiMarketData(client polymarketv1.MarketDataSubscriptionAPIClient) error {
    ctx := context.Background()
    stream, err := client.BiDirectionalStreamMarketData(ctx)
    if err != nil {
        return err
    }

    // Subscribe to initial symbol
    err = stream.Send(&polymarketv1.BiDirectionalStreamMarketDataRequest{
        Command: &polymarketv1.BiDirectionalStreamMarketDataRequest_Subscribe{
            Subscribe: &polymarketv1.SubscribeCommand{
                Symbols: []string{"tec-nfl-sbw-2026-02-08-kc"},
            },
        },
        Depth: 10,
    })
    if err != nil {
        return err
    }

    // Process responses
    for {
        resp, err := stream.Recv()
        if err != nil {
            return err
        }

        switch {
        case resp.GetSubscriptionAck() != nil:
            ack := resp.GetSubscriptionAck()
            fmt.Printf("ACK - Active: %v\n", ack.ActiveSymbols)
        case resp.GetSubscriptionError() != nil:
            subErr := resp.GetSubscriptionError()
            fmt.Printf("Error: %s - %s\n", subErr.ErrorCode, subErr.Message)
        case resp.GetUpdate() != nil:
            update := resp.GetUpdate()
            fmt.Printf("Update: %s - %d bids\n", update.Symbol, len(update.Bids))

            // Dynamically add another symbol
            // stream.Send(&polymarketv1.BiDirectionalStreamMarketDataRequest{
            //     Command: &polymarketv1.BiDirectionalStreamMarketDataRequest_Subscribe{
            //         Subscribe: &polymarketv1.SubscribeCommand{
            //             Symbols: []string{"tec-nfl-sbw-2026-02-08-phi"},
            //         },
            //     },
            // })
        case resp.GetHeartbeat() != nil:
            fmt.Println("Heartbeat")
        }
    }
}

// Run alongside the receive loop above to keep the stream alive when the
// client is not actively (un)subscribing. Send every 30 minutes (1800s) so
// the connection stays inside the AWS ALB 3600s idle window.
func sendKeepalives(stream polymarketv1.MarketDataSubscriptionAPI_BiDirectionalStreamMarketDataClient) {
    ticker := time.NewTicker(30 * time.Minute)
    defer ticker.Stop()
    for range ticker.C {
        _ = stream.Send(&polymarketv1.BiDirectionalStreamMarketDataRequest{
            Command: &polymarketv1.BiDirectionalStreamMarketDataRequest_Keepalive{
                Keepalive: &polymarketv1.KeepAliveCommand{},
            },
        })
    }
}
```

***

## Next Steps

<CardGroup cols={3}>
  <Card title="Order Streaming" icon="file-invoice" href="/streaming-endpoints/order-stream">
    Subscribe to order execution updates
  </Card>

  <Card title="Proto Reference" icon="book" href="/streaming-endpoints/proto-reference">
    Detailed protocol buffer reference
  </Card>

  <Card title="Error Handling" icon="triangle-exclamation" href="/streaming-endpoints/error-handling">
    Handle errors and reconnections
  </Card>
</CardGroup>
