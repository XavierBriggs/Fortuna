> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Order Execution Streaming

> Real-time order and execution updates via gRPC with Python

Subscribe to real-time order updates, execution reports, fills, and cancellations for your trading activity using Python and gRPC.

## Service Definition

**Service:** `polymarket.v1.OrderEntryAPI`
**RPC:** `CreateOrderSubscription`
**Type:** Server-side streaming

```protobuf theme={null}
service OrderEntryAPI {
    rpc CreateOrderSubscription(CreateOrderSubscriptionRequest)
        returns (stream CreateOrderSubscriptionResponse);
}
```

## Request Parameters

### CreateOrderSubscriptionRequest

| Field           | Type        | Required | Description                                                                                                        |
| --------------- | ----------- | -------- | ------------------------------------------------------------------------------------------------------------------ |
| `symbols`       | `list[str]` | No       | Filter by symbols. Empty list = all symbols.                                                                       |
| `accounts`      | `list[str]` | No       | Filter by trading accounts. Empty list = all accounts for authenticated user.                                      |
| `snapshot_only` | `bool`      | No       | If `True`, receive snapshot of current orders then close stream. If `False` (default), receive continuous updates. |

### Example Request

```python theme={null}
from polymarket.v1 import trading_pb2

# Subscribe to all orders for your accounts
request = trading_pb2.CreateOrderSubscriptionRequest(
    symbols=[],
    accounts=[],
    snapshot_only=False
)

# Subscribe to specific symbol
request = trading_pb2.CreateOrderSubscriptionRequest(
    symbols=["tec-nfl-sbw-2026-02-08-kc"],
    accounts=[],
    snapshot_only=False
)

# Get snapshot only (current state)
request = trading_pb2.CreateOrderSubscriptionRequest(
    symbols=[],
    accounts=[],
    snapshot_only=True
)
```

## Response Messages

The stream returns `CreateOrderSubscriptionResponse` messages with multiple event types:

### Response Fields

| Field                 | Type        | Description                                  |
| --------------------- | ----------- | -------------------------------------------- |
| `event`               | `oneof`     | One of: `heartbeat`, `snapshot`, or `update` |
| `session_id`          | `str`       | Unique session identifier for this stream    |
| `processed_sent_time` | `Timestamp` | Server timestamp when response was sent      |

### 1. Heartbeat Messages

Keep-alive messages to confirm connection health.

```python theme={null}
if response.HasField('heartbeat'):
    print(f"[{datetime.now().strftime('%H:%M:%S')}] Heartbeat received")
```

### 2. Snapshot Messages

Initial state of all matching orders when subscription starts.

```python theme={null}
from polymarket.v1 import enums_pb2

if response.HasField('snapshot'):
    snapshot = response.snapshot
    print(f"Snapshot received: {len(snapshot.orders)} orders")

    for order in snapshot.orders:
        print(f"  Order ID: {order.id}")
        print(f"  Symbol: {order.symbol}")
        print(f"  Side: {enums_pb2.Side.Name(order.side)}")
        print(f"  State: {enums_pb2.OrderState.Name(order.state)}")
```

### 3. Update Messages

Real-time order updates and executions.

```python theme={null}
if response.HasField('update'):
    update = response.update

    # Process executions
    for execution in update.executions:
        print(f"Execution: {enums_pb2.ExecutionType.Name(execution.type)}")

    # Process cancel rejects
    if update.HasField('cancel_reject'):
        print(f"Cancel rejected: {enums_pb2.CxlRejReason.Name(update.cancel_reject.reject_reason)}")
```

## Order Message Structure

### Core Order Fields

| Field                       | Type            | Description                                                                             |
| --------------------------- | --------------- | --------------------------------------------------------------------------------------- |
| `id`                        | `str`           | Exchange-assigned order ID                                                              |
| `clord_id`                  | `str`           | Client-assigned order ID (from your order request)                                      |
| `symbol`                    | `str`           | Trading symbol                                                                          |
| `side`                      | `Side`          | `SIDE_BUY` or `SIDE_SELL`                                                               |
| `type`                      | `OrderType`     | Order type (LIMIT, MARKET\_TO\_LIMIT, etc.)                                             |
| `state`                     | `OrderState`    | Current order state                                                                     |
| `account`                   | `str`           | Trading account                                                                         |
| `order_qty`                 | `int`           | Original order quantity                                                                 |
| `price`                     | `int`           | **Order price** (÷ price\_scale for decimal)                                            |
| `cum_qty`                   | `int`           | Cumulative filled quantity                                                              |
| `leaves_qty`                | `int`           | Remaining unfilled quantity                                                             |
| `avg_px`                    | `int`           | **Average fill price** (÷ price\_scale)                                                 |
| `fractional_quantity_scale` | `int`           | Quantity scale copied from the instrument at order creation (÷ to get decimal quantity) |
| `price_to_quantity_filled`  | `map<int, int>` | Quantity filled at each price point; key = price, value = filled qty                    |
| `insert_time`               | `Timestamp`     | When order was accepted                                                                 |
| `create_time`               | `Timestamp`     | When order was created                                                                  |

<Warning>
  **Price Fields:**

  * `price`, `avg_px`, `stop_price` are `int64` values
  * Divide by the instrument's `price_scale` to get decimal prices
  * `price_scale` is available from instrument metadata via the RefDataAPI

  ```python theme={null}
  price = order.price / price_scale
  print(f"Price: ${price:.4f}")
  ```
</Warning>

### Order States

| State                          | Value | Description                                   |
| ------------------------------ | ----- | --------------------------------------------- |
| `ORDER_STATE_NEW`              | 0     | Order accepted, resting in book               |
| `ORDER_STATE_PARTIALLY_FILLED` | 1     | Order partially executed                      |
| `ORDER_STATE_FILLED`           | 2     | **Order completely filled**                   |
| `ORDER_STATE_CANCELED`         | 3     | Order canceled (leaves\_qty = 0)              |
| `ORDER_STATE_REPLACED`         | 4     | Order modified/replaced                       |
| `ORDER_STATE_REJECTED`         | 5     | Order rejected by exchange                    |
| `ORDER_STATE_EXPIRED`          | 6     | Order expired (e.g., Day order at end of day) |
| `ORDER_STATE_PENDING_NEW`      | 7     | Order pending acceptance                      |
| `ORDER_STATE_PENDING_REPLACE`  | 8     | Replace request pending                       |
| `ORDER_STATE_PENDING_CANCEL`   | 9     | Cancel request pending                        |

```python theme={null}
from polymarket.v1 import enums_pb2

# Get state name
state_name = enums_pb2.OrderState.Name(order.state)
print(f"State: {state_name}")
```

### Order Sides

| Side        | Value |
| ----------- | ----- |
| `SIDE_BUY`  | 1     |
| `SIDE_SELL` | 2     |

```python theme={null}
side_name = enums_pb2.Side.Name(order.side)
print(f"Side: {side_name}")
```

### Order Types

| Type                         | Value | Description                         |
| ---------------------------- | ----- | ----------------------------------- |
| `ORDER_TYPE_LIMIT`           | 2     | Limit order with specified price    |
| `ORDER_TYPE_MARKET_TO_LIMIT` | 1     | Market order that converts to limit |
| `ORDER_TYPE_STOP`            | 3     | Stop order                          |
| `ORDER_TYPE_STOP_LIMIT`      | 4     | Stop-limit order                    |

## Execution Message Structure

Executions represent order lifecycle events (new, fill, cancel, reject).

### Execution Fields

| Field                 | Type              | Description                                   |
| --------------------- | ----------------- | --------------------------------------------- |
| `id`                  | `str`             | Unique execution ID                           |
| `type`                | `ExecutionType`   | Type of execution event                       |
| `order`               | `Order`           | Current order state after this execution      |
| `last_shares`         | `int`             | Quantity filled in this execution (for fills) |
| `last_px`             | `int`             | **Price of this fill** (÷ price\_scale)       |
| `trade_id`            | `str`             | Trade ID (for fills)                          |
| `aggressor`           | `bool`            | True if you were the aggressor in the trade   |
| `transact_time`       | `Timestamp`       | When execution occurred                       |
| `text`                | `str`             | Free-form text (e.g., reject reason)          |
| `order_reject_reason` | `OrdRejectReason` | Rejection reason (if rejected)                |

### Execution Types

| Type                          | Value | Description                              |
| ----------------------------- | ----- | ---------------------------------------- |
| `EXECUTION_TYPE_NEW`          | 0     | Order accepted (confirmed)               |
| `EXECUTION_TYPE_PARTIAL_FILL` | 1     | Partial fill occurred                    |
| `EXECUTION_TYPE_FILL`         | 2     | **Complete fill** (order fully executed) |
| `EXECUTION_TYPE_CANCELED`     | 3     | Order canceled                           |
| `EXECUTION_TYPE_REPLACE`      | 4     | Order modified                           |
| `EXECUTION_TYPE_REJECTED`     | 5     | Order rejected                           |
| `EXECUTION_TYPE_EXPIRED`      | 6     | Order expired                            |
| `EXECUTION_TYPE_DONE_FOR_DAY` | 7     | Order done for day                       |

### Order Reject Reasons

| Reason                                      | Value | Description             |
| ------------------------------------------- | ----- | ----------------------- |
| `ORD_REJECT_REASON_EXCHANGE_OPTION`         | 0     | Exchange option         |
| `ORD_REJECT_REASON_UNKNOWN_SYMBOL`          | 1     | Symbol not found        |
| `ORD_REJECT_REASON_EXCHANGE_CLOSED`         | 2     | Market is closed        |
| `ORD_REJECT_REASON_INCORRECT_QUANTITY`      | 3     | Invalid quantity        |
| `ORD_REJECT_REASON_INVALID_PRICE_INCREMENT` | 4     | Invalid price increment |
| `ORD_REJECT_REASON_INCORRECT_ORDER_TYPE`    | 5     | Incorrect order type    |
| `ORD_REJECT_REASON_PRICE_OUT_OF_BOUNDS`     | 6     | Price out of bounds     |
| `ORD_REJECT_REASON_NO_LIQUIDITY`            | 7     | No liquidity available  |

## Complete Example (from order\_stream.py)

This example matches the implementation from the Python examples repository:

```python theme={null}
import grpc
import requests
from datetime import datetime, timedelta
from typing import Optional
from polymarket.v1 import trading_pb2
from polymarket.v1 import trading_pb2_grpc
from polymarket.v1 import enums_pb2


class PolymarketOrderStreamer:
    def __init__(self, base_url: str = "https://rest.preprod.polymarketexchange.com",
                 grpc_server: str = "grpc-api.preprod.polymarketexchange.com:443"):
        self.base_url = base_url
        self.grpc_server = grpc_server
        self.access_token: Optional[str] = None
        self.refresh_token: Optional[str] = None
        self.access_expiration: Optional[datetime] = None
        self.session_id: Optional[str] = None
        self.price_scale: int = 1000  # Get from instrument metadata

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

    def stream_orders(self, symbols: list = None, accounts: list = None, snapshot_only: bool = False):
        """Stream order updates using gRPC."""
        if not self.access_token:
            raise ValueError("Not authenticated. Please login first.")

        # Create credentials
        credentials = grpc.ssl_channel_credentials()

        # Create channel
        channel = grpc.secure_channel(self.grpc_server, credentials)

        # Create stub
        stub = trading_pb2_grpc.OrderEntryAPIStub(channel)

        # Create request
        request = trading_pb2.CreateOrderSubscriptionRequest(
            symbols=symbols or [],
            accounts=accounts or [],
            snapshot_only=snapshot_only
        )

        # Set up metadata with authorization
        metadata = [
            ('authorization', f'Bearer {self.access_token}')
        ]

        try:
            print(f"Starting order stream")
            print(f"Symbols: {symbols or 'ALL'}")
            print(f"Accounts: {accounts or 'ALL'}")
            print(f"Snapshot only: {snapshot_only}")
            print("-" * 60)

            # Start streaming
            response_stream = stub.CreateOrderSubscription(request, metadata=metadata)

            for response in response_stream:
                self._process_order_response(response)

        except grpc.RpcError as e:
            print(f"gRPC error: {e.code()} - {e.details()}")
            raise
        except KeyboardInterrupt:
            print("\nStream interrupted by user")
        finally:
            channel.close()

    def _process_order_response(self, response):
        """Process and display order response."""
        # Capture session ID on first message
        if response.session_id and not self.session_id:
            self.session_id = response.session_id
            print(f"\n[{datetime.now().strftime('%H:%M:%S')}] Session established")
            print(f"  Session ID: {self.session_id}")
            print("-" * 60)

        if response.HasField('heartbeat'):
            print(f"[{datetime.now().strftime('%H:%M:%S')}] Heartbeat received")

        elif response.HasField('snapshot'):
            snapshot = response.snapshot
            print(f"\n[{datetime.now().strftime('%H:%M:%S')}] Order Snapshot")
            print(f"  Total orders: {len(snapshot.orders)}")

            for order in snapshot.orders:
                self._display_order(order)

            print("-" * 60)

        elif response.HasField('update'):
            update = response.update
            print(f"\n[{datetime.now().strftime('%H:%M:%S')}] Order Update")

            # Display executions
            if update.executions:
                print(f"  Executions: {len(update.executions)}")
                for execution in update.executions:
                    self._display_execution(execution)

            # Display cancel rejects
            if update.HasField('cancel_reject'):
                self._display_cancel_reject(update.cancel_reject)

            print("-" * 60)

    def _display_order(self, order):
        """Display order details."""
        print(f"  Order ID: {order.id}")
        print(f"    Client Order ID: {order.clord_id}")
        print(f"    Symbol: {order.symbol}")
        print(f"    Side: {enums_pb2.Side.Name(order.side)}")
        print(f"    Type: {enums_pb2.OrderType.Name(order.type)}")
        print(f"    State: {enums_pb2.OrderState.Name(order.state)}")

        if order.price > 0:
            price = order.price / self.price_scale
            print(f"    Price: ${price:.4f}")

        print(f"    Order Qty: {order.order_qty}")
        print(f"    Filled Qty: {order.cum_qty}")
        print(f"    Remaining Qty: {order.leaves_qty}")

        if order.avg_px > 0:
            avg_px = order.avg_px / self.price_scale
            print(f"    Avg Price: ${avg_px:.4f}")

        if order.account:
            print(f"    Account: {order.account}")

        print()

    def _display_execution(self, execution):
        """Display execution details."""
        print(f"  Execution ID: {execution.id}")
        print(f"    Type: {enums_pb2.ExecutionType.Name(execution.type)}")

        if execution.HasField('order'):
            order = execution.order
            print(f"    Order ID: {order.id}")
            print(f"    Symbol: {order.symbol}")
            print(f"    Side: {enums_pb2.Side.Name(order.side)}")
            print(f"    State: {enums_pb2.OrderState.Name(order.state)}")

        if execution.last_shares > 0:
            print(f"    Last Shares: {execution.last_shares}")

        if execution.last_px > 0:
            last_px = execution.last_px / self.price_scale
            print(f"    Last Price: ${last_px:.4f}")

        if execution.trade_id:
            print(f"    Trade ID: {execution.trade_id}")

        if execution.text:
            print(f"    Text: {execution.text}")

        if execution.order_reject_reason != enums_pb2.ORD_REJECT_REASON_EXCHANGE_OPTION:
            print(f"    Reject Reason: {enums_pb2.OrdRejectReason.Name(execution.order_reject_reason)}")

        print()

    def _display_cancel_reject(self, cancel_reject):
        """Display cancel reject details."""
        print(f"  Cancel Reject:")
        print(f"    Order ID: {cancel_reject.order_id}")
        print(f"    Client Order ID: {cancel_reject.clord_id}")
        print(f"    Reject Reason: {enums_pb2.CxlRejReason.Name(cancel_reject.reject_reason)}")
        if cancel_reject.text:
            print(f"    Text: {cancel_reject.text}")
        print()


# Usage
if __name__ == "__main__":
    streamer = PolymarketOrderStreamer()

    # Login using Private Key JWT
    streamer.login(
        auth0_domain="pmx-preprod.us.auth0.com",
        client_id="your_client_id",
        private_key_path="private_key.pem",
        audience="https://api.preprod.polymarketexchange.com"
    )

    # Stream orders
    streamer.stream_orders(
        symbols=["tec-nfl-sbw-2026-02-08-kc"],
        accounts=[]
    )
```

### Sample Output

```
Starting order stream
Symbols: ['tec-nfl-sbw-2026-02-08-kc']
Accounts: ALL
Snapshot only: False
------------------------------------------------------------

[14:30:15] Session established
  Session ID: session_abc123
------------------------------------------------------------

[14:30:15] Order Snapshot
  Total orders: 3

  Order ID: order_12345
    Client Order ID: clord_abc
    Symbol: tec-nfl-sbw-2026-02-08-kc
    Side: SIDE_BUY
    Type: ORDER_TYPE_LIMIT
    State: ORDER_STATE_NEW
    Price: $0.525
    Order Qty: 1000
    Filled Qty: 0
    Remaining Qty: 1000

------------------------------------------------------------
[14:30:45] Heartbeat received

[14:31:02] Order Update
  Executions: 1

  Execution ID: exec_67890
    Type: EXECUTION_TYPE_PARTIAL_FILL
    Order ID: order_12345
    Symbol: tec-nfl-sbw-2026-02-08-kc
    Side: SIDE_BUY
    State: ORDER_STATE_PARTIALLY_FILLED
    Last Shares: 300
    Last Price: $0.525
    Trade ID: trade_xyz

------------------------------------------------------------
```

## Order Lifecycle

### Typical Order Flow

```
1. NEW          -> Order accepted, resting in book
2. PARTIAL_FILL -> First partial fill
3. PARTIAL_FILL -> Additional partial fills (if any)
4. FILL         -> Final fill, order complete
```

### Cancel Flow

```
1. NEW              -> Order resting
2. PENDING_CANCEL   -> Cancel request received
3. CANCELED         -> Cancel confirmed
```

### Reject Flow

```
1. REJECTED -> Order rejected immediately
```

## Next Steps

<CardGroup cols={2}>
  <Card title="Proto Reference" icon="book" href="/streaming-endpoints/proto-reference">
    Detailed message and field reference
  </Card>

  <Card title="Error Handling" icon="triangle-exclamation" href="/streaming-endpoints/error-handling">
    Handle errors and reconnections
  </Card>

  <Card title="Market Data Stream" icon="chart-line" href="/streaming-endpoints/market-data-stream">
    Stream real-time market data
  </Card>
</CardGroup>
