> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Reporting Data

> Historical data, trade searches, and drop copy

## Reporting APIs

Reporting APIs provide query-based access to historical data for reconciliation, compliance reporting, historical analysis, and CSV exports.

## Trade Search

**What does `/v1/report/trades/search` return?**

It returns only trades associated with the authenticated participant's authorized accounts.

**Does it return all exchange trades?**

No. You only see trades for accounts you have access to.

**Query capabilities**: Filter by time range, filter by instrument, filter by account, paginated results.

**Example:**

```bash theme={null}
POST /v1/report/trades/search
{
  "accountId": "your-account-id",
  "startTime": "2024-01-01T00:00:00Z",
  "endTime": "2024-01-31T23:59:59Z",
  "instrument": "tec-nfl-sbw-2026-02-08-kc"
}
```

## Order Search

Search for historical orders: all order states (filled, canceled, rejected), filter by instrument/time/status, include execution details, paginated results.

## Report Downloads

Download reports as CSV files: trade reports, execution reports, position reports, and fee reports. CSV exports are useful for importing into spreadsheets, compliance record-keeping, and third-party analysis tools.

### Ledger CSV Exports

Two streaming CSV endpoints expose the full position and cash audit trail:

| Endpoint                                  | Description                                                                                                                                 |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET /v1/positions/ledger/download`       | Every position change (deltas + cumulative state). See [Position Ledger](/institutional/positions/overview#position-ledger).                |
| `GET /v1/funding/balance-ledger/download` | Every cash balance change (`before_balance` / `after_balance` + typed `entry_type`). See [Balance Ledger](/institutional/funding/overview). |

Both require the `read:positions` scope and are subject to a per-firm rate limit of \~5 downloads per minute. The historical floor is `2026-05-01`; pre-floor data is not retrievable.

## Real-Time vs Historical

**Are reporting APIs real-time?**

No. Reporting APIs are query-based (not streaming), paginated (for large result sets), and designed for historical analysis. For real-time updates, use [Streaming APIs](/trader-guide/streaming-apis) for orders and positions and [Drop Copy](#drop-copy) for authoritative post-trade feed.

## Drop Copy

**What is Drop Copy?**

A real-time, authoritative post-trade feed delivering executions, trades, position changes, and instrument state changes.

**Who should use Drop Copy?** ISVs building trading platforms, IBs managing customer order flow, FCMs requiring authoritative execution records, and any participant needing real-time post-trade data.

**Is Drop Copy redundant with reporting APIs?** No. **Drop Copy** is a real-time feed of events as they happen, while **Reporting APIs** are historical queries of past events.

Use Drop Copy for real-time operations and reporting APIs for historical analysis.

**Are Drop Copy streams resumable?** Yes, where supported. Clients should persist resume state (token or sequence number), resume from last processed event after reconnection, and handle at-least-once delivery (deduplicate using IDs).

## Pagination

**Are search APIs paginated?**

Yes. All search endpoints use pagination:

```json theme={null}
{
  "results": [...],
  "nextPageToken": "abc123..."
}
```

**How to paginate:**

1. Make initial request
2. Check for `nextPageToken` in response
3. Include token in next request
4. Repeat until `nextPageToken` is null/absent

**Example pagination:**

```python theme={null}
all_trades = []
page_token = None

while True:
    response = api.search_trades(
        account_id=account_id,
        page_token=page_token
    )

    all_trades.extend(response['trades'])

    page_token = response.get('nextPageToken')
    if not page_token:
        break
```

## Large Time Ranges

**Can large time ranges be queried?** Yes, but large ranges may require many pages; consider incremental querying (query each day separately), be mindful of rate limits, and cache results to avoid repeated queries.

**Recommendation:**
Query incrementally and store results locally rather than querying large ranges repeatedly.

## Data Retention

Historical data is retained according to regulatory requirements. Contact support for specific retention policies.

## Trade Identification

**What identifies a trade uniquely?**

`tradeId` - Use this for deduplication and reconciliation.

**Pagination must use** `pageToken` not trade IDs. Don't attempt to paginate by incrementing trade IDs.

## Execution vs Trade

**What is the difference?** An **execution** is a state change on a single order, while a **trade** is a matched event between two orders (aggressor and passive side).

A trade contains two executions (one for each side).

**Are trades final immediately?** No. Trades progress through states: **NEW** (initial matched state), **CLEARED** (cleared through DCO), **BUSTED** (trade canceled/busted, rare).

## Reporting Best Practices

**Query incrementally**: Query recent data frequently, archive older data locally.

**Use appropriate time ranges**: Don't query months of data repeatedly. Cache and query incrementally.

**Monitor pagination**: Always follow `nextPageToken` until exhausted.

**Reconcile using IDs**: Use `tradeId` and `execId` for reconciliation, not timestamps.

**Export to CSV**: For compliance and record-keeping, use CSV export endpoints.
