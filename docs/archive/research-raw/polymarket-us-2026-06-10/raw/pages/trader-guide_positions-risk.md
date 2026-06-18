> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Positions & Risk

> Position tracking and risk management

## Available Position Data

**Net Position**: Current position quantity, long or short, per instrument

**Cost Basis**: Average entry price, total cost, unrealized P\&L calculation

**Realized P\&L**: Profit/loss from closed positions, per instrument and aggregate, intraday and cumulative

**Position State**: Open positions, position changes from trades, real-time updates

## Intraday Updates

**Are positions updated intraday?**

Yes. Positions reflect real-time execution state, clearing state, and risk calculations. Positions update as orders fill, trades clear, and risk limits change.

## Position Streaming

**Are position updates streamed?**

Yes, via the **Positions Subscription API**.

Subscribe to receive real-time position updates:

```
POST /v1/positions/subscribe
{
  "accountId": "your-account-id"
}
```

Updates are pushed whenever a trade executes, a position changes, or risk metrics update.

## Querying Positions

**REST endpoint:**

```bash theme={null}
GET /v1/positions?accountId=your-account-id
```

Returns current positions for the specified account: all open positions, position quantities, unrealized P\&L, and cost basis.

## Buying Power

**What determines buying power?**

Available cash balance, collateral value, margin requirements, and risk model calculations.

**Checking buying power:**

```bash theme={null}
GET /v1/accounts/accounts?accountId=your-account-id
```

Returns account details including current buying power.

## Position Reconciliation

**Best practices**: Subscribe to position updates for real-time state, periodically query positions endpoint to reconcile, use `tradeId` and `execId` for audit trail, compare position changes with execution records.

## Mark-to-Market

Positions are marked-to-market using current market prices (for liquid instruments), settlement prices (for less liquid instruments), and daily official settlement.

Unrealized P\&L is calculated from current mark vs. cost basis.

## Position Lifecycle

1. **Order fills** → Position opens or changes
2. **Trade clears** → Position confirmed
3. **Mark-to-market** → Unrealized P\&L updates
4. **Position closes** → Realized P\&L recorded
5. **Settlement** → Final P\&L determined

## Multiple Accounts

If you manage multiple trading accounts: each account has independent positions, risk limits apply per account, query positions per account using `accountId`.

## Position Reports

Download position reports for end-of-day positions, historical position snapshots, P\&L statements, and compliance reporting.

See [Reporting](/trader-guide/reporting) for report access.

## Ledger Access

For full audit-grade history of every position change and every cash event, use the ledger APIs (covered by the `read:positions` scope, with a hard historical floor of `2026-05-01`):

* **[Position Ledger](/institutional/positions/overview#position-ledger)** — `GET /v1/positions/ledger` (paginated) and `/download` (CSV) return every position change with both deltas (`quantityChange`, `costChange`, `realizedChange`) and the cumulative state after each change.
* **[Balance Ledger](/institutional/funding/overview)** — `GET /v1/funding/balance-ledger` (paginated) and `/download` (CSV) return every cash balance change with `before_balance` / `after_balance` and a typed `entry_type` (deposits, withdrawals, fills, fees, adjustments, …).
* **[Balance Ledger Stream](/streaming-endpoints/balance-ledger-stream)** — gRPC `CreateBalanceLedgerSubscription` pushes the same balance ledger entries in real time, with `resume_time` for gap-free reconnection.

## Troubleshooting

**Position doesn't match expectations**

Check:

* Have all trades cleared?
* Are you looking at the correct account?
* Is there a pending order that will affect the position?
* Have you received all position updates?

**Unrealized P\&L calculation differs**

Verify:

* What mark price is being used?
* What cost basis is being used?
* Are fees included in P\&L?

**Can't place order due to risk limits**

Check current position size, order size would exceed limits, available buying power, and account status (active, suspended, etc.).
