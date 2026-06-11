> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Streaming

> Real-time data streaming semantics and best practices

## Streaming Semantics

Streaming APIs provide real-time updates over long-lived HTTP connections.

## Delivery Guarantees

**Are streaming APIs exactly-once?** No. Delivery is **at-least-once**. This means messages may be delivered more than once, clients must be idempotent (handle duplicates), and you should use message IDs to deduplicate.

**Example deduplication:**

```python theme={null}
seen_messages = set()

for message in stream:
    message_id = message.get('id')

    if message_id in seen_messages:
        continue  # Skip duplicate

    seen_messages.add(message_id)
    process_message(message)
```

## Snapshots and Deltas

**Do streams always start with a snapshot?** Yes. When you subscribe to a stream, the first message is a complete snapshot and subsequent messages are updates (deltas or new snapshots). You must apply the initial snapshot to establish state and process subsequent updates against that base state.

**Example:**

```
Message 1: Snapshot of all open orders
Message 2: Order 123 filled (delta)
Message 3: Order 456 canceled (delta)
Message 4: New order 789 opened (delta)
```

## Stream Types

**Market Data Streams**: Order book updates, BBO (best bid/offer), instrument state changes

**Order Streams**: Order status changes, execution reports, order lifecycle events

**Position Streams**: Position changes, realized/unrealized P\&L updates, risk metric changes

**Drop Copy Streams**: Authoritative post-trade feed, executions and trades, settlement updates

## Connection Management

**Persistent connections**: Streams use long-lived HTTP connections, not WebSockets (standard HTTP streaming), server pushes updates as they occur.

**Reconnection**: Connections can drop due to network issues; implement automatic reconnection with exponential backoff.

**Example reconnection:**

```python theme={null}
def connect_with_retry(max_retries=5):
    for attempt in range(max_retries):
        try:
            return connect_to_stream()
        except ConnectionError:
            wait = 2 ** attempt  # 1, 2, 4, 8, 16 seconds
            time.sleep(wait)
    raise Exception("Max retries exceeded")
```

## Resume Capability

**Some streams are resumable**: Persist resume state (token or sequence number), resume from last processed event after disconnect, avoid missing events during disconnection.

**Example resume:**

```bash theme={null}
POST /v1/orders/subscribe
{
  "accountId": "your-account-id",
  "resumeToken": "last_processed_token"
}
```

Not all streams support resume. Check API documentation for each endpoint.

## Message Ordering

Messages on a single stream are delivered in order.

Messages across different streams may not be ordered relative to each other. If you need cross-stream ordering, use timestamps in messages, implement local ordering logic, and use sequence numbers where available.

## Idempotency

**Why is idempotency important?** With at-least-once delivery, you may process the same message twice and your logic must handle duplicates gracefully.

**How to be idempotent**: Track processed message IDs, use database unique constraints on IDs, design operations to be repeatable (e.g., "set to X" not "add Y").

**Example idempotent order processing:**

```python theme={null}
def process_order_update(order_update):
    order_id = order_update['orderId']
    exec_id = order_update['execId']

    # Try to insert execution
    try:
        db.insert_execution(exec_id, order_update)
    except UniqueConstraintError:
        # Already processed this execution
        return

    # Update order state
    db.update_order(order_id, order_update['status'])
```

## Rate Limiting on Streams

**Client-to-server messages**: Limited to 100 messages per second per firm (across all streams), averaged over a 1-minute window. Short bursts above this rate are allowed. Applies to requests you send (subscriptions, commands), does not apply to server-pushed updates.

**Connection limits**: Maximum 20 concurrent streams per firm; plan your subscription strategy accordingly.

## Snapshot Refresh

Some streams send periodic snapshots even if state hasn't changed to help detect missed messages, allow clients to reconcile state, typically every few minutes. When you receive a snapshot, replace your entire local state with the snapshot; don't try to merge or diff against previous state.

## Troubleshooting

**Missing messages**: Possible causes include connection dropped (implement reconnection), resume state lost (persist resume tokens), at-least-once delivery issue (check for duplicates elsewhere). Solution: Use resumable streams and persist state.

**Duplicate messages**: Expected behavior. Implement deduplication using message IDs.

**Stream stops sending updates**: Check if the connection is still alive, send periodic heartbeats or test messages, implement connection timeout detection, reconnect if no messages received for X seconds.

**State inconsistency**: If your local state doesn't match server state, unsubscribe and resubscribe (forces new snapshot), reconcile with REST API query, or check for processing errors in your code.

## Best Practices

**Always handle reconnections**: Network issues are inevitable. Auto-reconnect with backoff.

**Deduplicate messages**: Track processed message IDs and skip duplicates.

**Persist resume state**: Save tokens/sequence numbers to resume after restart.

**Monitor stream health**: Detect stale connections and reconnect.

**Process snapshots correctly**: Replace state, don't merge.

**Use appropriate connection counts**: Stay within the 10-connection limit.

**Handle snapshot + delta pattern**: Apply initial snapshot, then process deltas.
