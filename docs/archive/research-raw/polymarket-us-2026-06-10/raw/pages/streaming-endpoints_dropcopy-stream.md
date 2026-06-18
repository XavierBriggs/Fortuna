> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# DropCopy & Trade Capture Streaming

> Real-time execution reports and trade capture via gRPC

Subscribe to real-time execution reports, trade captures, instrument state changes, and position updates for your firm using gRPC streaming.

<Note>
  **gRPC Only** - DropCopy endpoints are gRPC streaming only. There are no REST equivalents.
</Note>

## Service Definition

**Service:** `polymarket.v1.DropCopyAPI`
**Type:** Server-side streaming (all endpoints)

```protobuf theme={null}
service DropCopyAPI {
    rpc CreateDropCopySubscription(CreateDropCopySubscriptionRequest)
        returns (stream CreateDropCopySubscriptionResponse);

    rpc CreateTradeCaptureReportSubscription(CreateTradeCaptureReportSubscriptionRequest)
        returns (stream CreateTradeCaptureReportSubscriptionResponse);

    rpc CreateInstrumentStateChangeSubscription(CreateInstrumentStateChangeSubscriptionRequest)
        returns (stream CreateInstrumentStateChangeSubscriptionResponse);

    rpc CreatePositionChangeSubscription(CreatePositionChangeSubscriptionRequest)
        returns (stream CreatePositionChangeSubscriptionResponse);
}
```

## Available Streams

| Stream                      | Description                        | Use Case                             |
| --------------------------- | ---------------------------------- | ------------------------------------ |
| **DropCopy**                | Execution reports (fills, cancels) | Real-time order execution monitoring |
| **Trade Capture Report**    | Completed trades                   | Trade reconciliation, compliance     |
| **Instrument State Change** | Market state updates               | Trading halts, market open/close     |
| **Position Change**         | Position updates                   | Real-time P\&L, risk monitoring      |

***

## 1. DropCopy Subscription

Stream execution reports as they occur for your firm.

### Request Parameters

| Field          | Type        | Required | Description                                 |
| -------------- | ----------- | -------- | ------------------------------------------- |
| `resume_token` | `bytes`     | No       | Resume from previous position               |
| `resume_time`  | `Timestamp` | No       | Resume from specific time                   |
| `symbols`      | `list[str]` | No       | Filter by symbols. Empty = all symbols      |
| `firms`        | `list[str]` | No       | Filter by firms. Empty = authenticated firm |

### Response Fields

| Field          | Type              | Description                     |
| -------------- | ----------------- | ------------------------------- |
| `resume_token` | `bytes`           | Store for reconnection          |
| `executions`   | `list[Execution]` | Execution reports in this batch |

### Example

```python theme={null}
import grpc
from datetime import datetime
from polymarket.v1 import dropcopy_pb2
from polymarket.v1 import dropcopy_pb2_grpc
from polymarket.v1 import enums_pb2


class DropCopyStreamer:
    def __init__(self, grpc_server: str = "grpc-api.preprod.polymarketexchange.com:443"):
        self.grpc_server = grpc_server
        self.access_token = None
        self.last_resume_token = None

    def stream_executions(self, symbols: list = None, resume_token: bytes = None):
        """Stream execution reports via DropCopy."""
        credentials = grpc.ssl_channel_credentials()
        channel = grpc.secure_channel(self.grpc_server, credentials)
        stub = dropcopy_pb2_grpc.DropCopyAPIStub(channel)

        request = dropcopy_pb2.CreateDropCopySubscriptionRequest(
            symbols=symbols or []
        )
        if resume_token:
            request.resume_token = resume_token

        metadata = [('authorization', f'Bearer {self.access_token}')]

        try:
            print("Starting DropCopy stream...")
            print(f"Symbols: {symbols or 'ALL'}")
            print("-" * 60)

            for response in stub.CreateDropCopySubscription(request, metadata=metadata):
                self.last_resume_token = response.resume_token

                for execution in response.executions:
                    self._display_execution(execution)

        except grpc.RpcError as e:
            print(f"gRPC error: {e.code()} - {e.details()}")
        finally:
            channel.close()

    def _display_execution(self, execution):
        """Display execution details."""
        print(f"\n[{datetime.now().strftime('%H:%M:%S')}] Execution")
        print(f"  ID: {execution.id}")
        print(f"  Type: {enums_pb2.ExecutionType.Name(execution.type)}")

        if execution.HasField('order'):
            order = execution.order
            print(f"  Order ID: {order.id}")
            print(f"  Symbol: {order.symbol}")
            print(f"  Side: {enums_pb2.Side.Name(order.side)}")
            print(f"  State: {enums_pb2.OrderState.Name(order.state)}")

        if execution.last_shares > 0:
            print(f"  Fill Qty: {execution.last_shares}")
        if execution.last_px > 0:
            print(f"  Fill Price: {execution.last_px}")
        if execution.trade_id:
            print(f"  Trade ID: {execution.trade_id}")

        print("-" * 60)


# Usage
if __name__ == "__main__":
    streamer = DropCopyStreamer()
    # streamer.access_token = "your_token"
    streamer.stream_executions()
```

### Sample Output

```
Starting DropCopy stream...
Symbols: ALL
------------------------------------------------------------

[14:30:15] Execution
  ID: exec_abc123
  Type: EXECUTION_TYPE_FILL
  Order ID: order_xyz789
  Symbol: tec-nfl-sbw-2026-02-08-kc
  Side: SIDE_BUY
  State: ORDER_STATE_FILLED
  Fill Qty: 100
  Fill Price: 525
  Trade ID: trade_def456
------------------------------------------------------------

[14:30:16] Execution
  ID: exec_ghi789
  Type: EXECUTION_TYPE_CANCELED
  Order ID: order_abc123
  Symbol: tec-nfl-sbw-2026-02-08-kc
  Side: SIDE_SELL
  State: ORDER_STATE_CANCELED
------------------------------------------------------------
```

***

## 2. Trade Capture Report Subscription

Stream completed trades for reconciliation and compliance.

### Request Parameters

| Field          | Type        | Required | Description                   |
| -------------- | ----------- | -------- | ----------------------------- |
| `resume_token` | `bytes`     | No       | Resume from previous position |
| `resume_time`  | `Timestamp` | No       | Resume from specific time     |
| `symbols`      | `list[str]` | No       | Filter by symbols             |
| `firms`        | `list[str]` | No       | Filter by firms               |

### Response Fields

| Field                   | Type          | Description            |
| ----------------------- | ------------- | ---------------------- |
| `resume_token`          | `bytes`       | Store for reconnection |
| `trade_capture_reports` | `list[Trade]` | Trade records          |

### Example

```python theme={null}
def stream_trade_captures(self, symbols: list = None):
    """Stream trade capture reports."""
    credentials = grpc.ssl_channel_credentials()
    channel = grpc.secure_channel(self.grpc_server, credentials)
    stub = dropcopy_pb2_grpc.DropCopyAPIStub(channel)

    request = dropcopy_pb2.CreateTradeCaptureReportSubscriptionRequest(
        symbols=symbols or []
    )

    metadata = [('authorization', f'Bearer {self.access_token}')]

    for response in stub.CreateTradeCaptureReportSubscription(request, metadata=metadata):
        for trade in response.trade_capture_reports:
            print(f"Trade ID: {trade.id}")
            print(f"  Aggressor: {trade.aggressor.order.id if trade.aggressor else 'N/A'}")
            print(f"  Passive: {trade.passive.order.id if trade.passive else 'N/A'}")
            print(f"  State: {trade.state}")
```

### Trade Structure

Each trade contains two executions:

| Field        | Description                              |
| ------------ | ---------------------------------------- |
| `id`         | Unique trade ID                          |
| `aggressor`  | Execution for the incoming (taker) order |
| `passive`    | Execution for the resting (maker) order  |
| `trade_type` | Type of trade (REGULAR, CROSS, etc.)     |
| `state`      | Trade state (NEW, CLEARED, etc.)         |

***

## 3. Instrument State Change Subscription

Stream market state changes (halts, opens, closes).

<Info>
  **No Participant ID Required**

  This endpoint only requires Auth0 JWT authentication with `read:instruments` scope. You do not need to provide the `x-participant-id` header or complete KYC onboarding to access instrument state changes.
</Info>

### Request Parameters

| Field          | Type        | Required | Description                   |
| -------------- | ----------- | -------- | ----------------------------- |
| `resume_token` | `bytes`     | No       | Resume from previous position |
| `resume_time`  | `Timestamp` | No       | Resume from specific time     |
| `symbols`      | `list[str]` | No       | Filter by symbols             |

### Response Fields

| Field          | Type               | Description               |
| -------------- | ------------------ | ------------------------- |
| `resume_token` | `bytes`            | Store for reconnection    |
| `instruments`  | `list[Instrument]` | Updated instrument states |

### Instrument States

#### Primary State Flow

| State                                               | Description                                                                                                                                                                                                      |
| --------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `INSTRUMENT_STATE_PENDING`                          | Initial state for a newly created instrument which has not yet begun trading.                                                                                                                                    |
| `INSTRUMENT_STATE_OPEN`                             | In this state, the instrument is open for continuous order entry and matching.                                                                                                                                   |
| `INSTRUMENT_STATE_CLOSED`                           | In this state, orders can not be entered, modified, or canceled, and no matching occurs. Any existing Day orders will be expired.                                                                                |
| `INSTRUMENT_STATE_EXPIRED`                          | An instrument moves to this state when its Expiration Date/Time is reached. In this state, any resting orders are expired and no new orders can be entered.                                                      |
| `INSTRUMENT_STATE_TERMINATED`                       | When an instrument's Termination Date is reached, the order book is removed from the matching engine, orders are canceled, and positions are closed. Historical data will still remain in Polymarket US ledgers. |

#### Exception States

| State                                               | Description                                                                                   |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `INSTRUMENT_STATE_SUSPENDED`                        | Orders can be canceled but no matching occurs, and no order entry or modification is allowed. |
| `INSTRUMENT_STATE_HALTED`                           | This state is similar to SUSPENDED, with the exception that orders cannot be canceled.        |

#### Other Possible States

| State                                               | Description                                                                                                                                                                                                                                                                                       |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `INSTRUMENT_STATE_PREOPEN`                          | Orders can be entered and modified, but no matching occurs. When the instrument transitions to an OPEN state, the orders entered during PREOPEN will match at a single opening price that is automatically determined by an algorithm that is designed to maximize the volume traded at the open. |
| `INSTRUMENT_STATE_MATCH_AND_CLOSE_AUCTION`          | This state is similar to PREOPEN, with the exception that matching will occur upon the transition of this state to any other state. This state is useful if you want matching to occur at the end of the state, but you don't want the instrument to be open after.                               |

### Example

```python theme={null}
def stream_instrument_states(self, symbols: list = None):
    """Stream instrument state changes."""
    credentials = grpc.ssl_channel_credentials()
    channel = grpc.secure_channel(self.grpc_server, credentials)
    stub = dropcopy_pb2_grpc.DropCopyAPIStub(channel)

    request = dropcopy_pb2.CreateInstrumentStateChangeSubscriptionRequest(
        symbols=symbols or []
    )

    metadata = [('authorization', f'Bearer {self.access_token}')]

    for response in stub.CreateInstrumentStateChangeSubscription(request, metadata=metadata):
        for instrument in response.instruments:
            print(f"Symbol: {instrument.symbol}")
            print(f"  State: {instrument.state}")
            print(f"  Description: {instrument.description}")
```

***

## 4. Position Change Subscription

Stream real-time position updates.

### Request Parameters

| Field          | Type        | Required | Description                   |
| -------------- | ----------- | -------- | ----------------------------- |
| `resume_token` | `bytes`     | No       | Resume from previous position |
| `resume_time`  | `Timestamp` | No       | Resume from specific time     |
| `symbols`      | `list[str]` | No       | Filter by symbols             |
| `firms`        | `list[str]` | No       | Filter by firms               |

### Response Fields

| Field              | Type                   | Description            |
| ------------------ | ---------------------- | ---------------------- |
| `resume_token`     | `bytes`                | Store for reconnection |
| `position_changes` | `list[PositionChange]` | Position updates       |

### PositionChange Structure

| Field         | Type        | Description            |
| ------------- | ----------- | ---------------------- |
| `position`    | `Position`  | Current position state |
| `change_time` | `Timestamp` | When change occurred   |

### Example

```python theme={null}
def stream_position_changes(self, symbols: list = None):
    """Stream position changes."""
    credentials = grpc.ssl_channel_credentials()
    channel = grpc.secure_channel(self.grpc_server, credentials)
    stub = dropcopy_pb2_grpc.DropCopyAPIStub(channel)

    request = dropcopy_pb2.CreatePositionChangeSubscriptionRequest(
        symbols=symbols or []
    )

    metadata = [('authorization', f'Bearer {self.access_token}')]

    for response in stub.CreatePositionChangeSubscription(request, metadata=metadata):
        for change in response.position_changes:
            pos = change.position
            print(f"Position Update: {pos.symbol}")
            print(f"  Account: {pos.account}")
            print(f"  Net Qty: {pos.net_position}")
            print(f"  Avg Price: {pos.average_price}")
            print(f"  Change Time: {change.change_time}")
```

***

## Reconnection Handling

All DropCopy streams support resume tokens for seamless reconnection:

```python theme={null}
# Store resume_token from each response
last_token = response.resume_token

# On reconnect, pass the token
request = dropcopy_pb2.CreateDropCopySubscriptionRequest(
    resume_token=last_token
)
```

<Warning>
  **Resume Token Expiry**

  Resume tokens may expire after extended disconnection periods. If resumption fails, start a fresh subscription and reconcile with the Report API for any missed data.
</Warning>

## Comparing DropCopy vs Order Stream

| Feature            | DropCopy                | Order Stream                 |
| ------------------ | ----------------------- | ---------------------------- |
| **Scope**          | Firm-wide executions    | User's orders only           |
| **Use Case**       | Back-office, compliance | Trading UI, order management |
| **Data**           | Executions, trades      | Orders, executions           |
| **Authentication** | Firm-level token        | User token                   |

<Tip>
  **When to Use DropCopy**

  Use DropCopy when you need:

  * Firm-wide visibility across all users
  * Trade capture reports for reconciliation
  * Instrument state change notifications
  * Real-time position monitoring across accounts
</Tip>

## Next Steps

<CardGroup cols={2}>
  <Card title="Order Stream" icon="list-check" href="/streaming-endpoints/order-stream">
    Stream user-level order updates
  </Card>

  <Card title="Market Data Stream" icon="chart-line" href="/streaming-endpoints/market-data-stream">
    Stream real-time market data
  </Card>

  <Card title="Funding Stream" icon="wallet" href="/streaming-endpoints/funding-stream">
    Stream funding transaction updates
  </Card>

  <Card title="Error Handling" icon="triangle-exclamation" href="/streaming-endpoints/error-handling">
    Handle errors and reconnections
  </Card>
</CardGroup>
