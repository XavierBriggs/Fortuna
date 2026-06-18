> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Market Data

> Accessing market data and order books

## Available Market Data

**Instrument Reference Data**: Symbols and metadata, trading state and limits, contract specifications, expiration dates

**Order Book Data**: Aggregated order book depth, best bid and offer (BBO), multiple price levels

**Trading State**: Market status (open, closed, halted), trading hours, circuit breakers

## Live Market Data Delivery

Market data is delivered via the **Market Data Subscription API** over long-lived HTTP connections.

**Key characteristics**: Server pushes updates over persistent connection; snapshot-style updates (each message is complete); treat each message as a full update unless documentation specifies delta semantics; no WebSockets (uses HTTP streaming).

## Order Books

**Are order books aggregated?**

Yes. REST order book endpoints return aggregated depth only. Multiple orders at the same price level are combined; individual order IDs are not visible in market data; only aggregate quantity is shown per price level.

**Price levels available**: Multiple levels of depth (configurable), best bid/offer always included, deeper book available with appropriate scopes.

## Price and Quantity Encoding

**Are prices and quantities floating point?**

No. All prices and quantities are **integer-encoded strings**.

**Example:**

```json theme={null}
{
  "price": "550",  // Divide by price_scale (e.g., 550 / 1000 = 0.55)
  "quantity": "1000"  // Integer quantity
}
```

Always use string parsing for prices and quantities. Never use floating-point math for price calculations.

## Trade Tape

**Is there a public trade tape?**

No. Trades are not disseminated via market data APIs. You can only see your own trades via [Reporting APIs](/trader-guide/reporting) and your own executions via [Order subscriptions](/trader-guide/streaming-apis).

## Market Data Scopes

**Basic market data** (`read:marketdata`): Best bid/offer, top-of-book snapshots, basic instrument data

**Level 2 market data** (`read:l2marketdata`): Full order book depth, multiple price levels, aggregate depth

**Reference data** (`read:instruments`): Instrument listings, metadata and specifications, trading schedules

## Subscribing to Market Data

Use the Market Data Subscription API to receive updates:

```
POST /v1/marketdata/subscribe
{
  "instruments": ["tec-nfl-sbw-2026-02-08-kc", "aec-nfl-buf-nyj-2025-01-15"],
  "dataType": "orderbook"
}
```

The connection remains open and the server pushes updates as market conditions change.

## Update Frequency

Market data updates are sent on every change to the order book, at regular intervals (even if no changes), and as snapshots (full book, not deltas).

## Best Practices

**Cache reference data**: Instrument metadata changes infrequently. Cache it locally and refresh periodically (every 5-15 minutes).

**Use streaming for real-time data**: Don't poll market data endpoints repeatedly. Use the subscription API for live updates.

**Handle reconnections**: Streaming connections can drop. Implement automatic reconnection with exponential backoff.

**Process snapshots correctly**: Each market data message is a complete snapshot. Replace your local book state with each update.

## Troubleshooting

**Market data seems stale**

Check:

* Is your subscription still active?
* Has the connection dropped?
* Is the market actually open?

**Missing price levels**

Check:

* Do you have the `read:l2marketdata` scope?
* Are you requesting depth parameter correctly?
* Is there actually liquidity at those levels?

**Can't see my own orders in the book**

Correct behavior. Market data shows aggregated depth only. To see your own orders, use the [Order Management](/trader-guide/order-management) APIs.
