> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Order Management

> Best practices and troubleshooting for order operations

## Supported Order Types

**Market-to-Limit**: Submits as market order; unfilled quantity converts to limit order; prevents walking the book excessively

**Limit**: Standard limit order, executes at specified price or better, rests in book if not immediately filled

**Stop**: Triggers market order when stop price reached; used for stop-loss scenarios

**Stop-Limit**: Triggers limit order when stop price reached; provides price protection after trigger

## Time-In-Force Values

**DAY**: Order expires at end of trading day; canceled automatically at market close

**Good-Till-Cancel (GTC)**: Order remains active until filled or explicitly canceled; persists across trading days

**Immediate-Or-Cancel (IOC)**: Execute immediately or cancel; partial fills allowed; remaining quantity canceled

**Fill-Or-Kill (FOK)**: Execute entire order immediately or cancel; no partial fills; all-or-nothing

**Good-Till-Time**: Expires at specified time; custom expiration timestamp

## Order Lifecycle

Understanding the order lifecycle is critical for proper order management:

1. **New** - Order submitted but not yet acknowledged
2. **Pending** - Order accepted and waiting for execution
3. **Partially Filled** - Some quantity executed, remaining quantity active
4. **Filled** - Entire order quantity executed
5. **Canceled** - Order canceled before full execution
6. **Rejected** - Order rejected by the exchange

## Executions vs Trades

**What is the difference between an execution and a trade?**

* **Execution**: A state change on a single order (your order)
* **Trade**: A matched event between two orders (aggressor and passive side)

A trade contains two executions - one for each side of the match.

**Are trades final immediately?**

No. Trades progress through states:

* **NEW**: Initial matched state
* **CLEARED**: Cleared through DCO
* **BUSTED**: Trade canceled/busted (rare)

**What identifies a trade uniquely?**

`tradeId` - Use this for deduplication and reconciliation.

## Verifying Order Status

<Warning>
  **Important**: A returned order ID with HTTP 200 does **not** guarantee the order was successfully processed. Failed orders don't generate user-visible errors unless a required field is missing.
</Warning>

**Always verify order status** via the order stream or by querying the order status endpoint. Do not rely solely on HTTP 200 responses or returned order IDs.

## Common Order Failures

Orders can fail for several reasons:

### Invalid Account

The account specified in the order doesn't exist or you don't have permission to trade on it.

**Solution**: Verify the account ID is correct and you have trading permissions.

### Expired Instrument

The instrument you're trying to trade has expired or is no longer active.

**Solution**: Check the instrument status before placing orders. Use the reference data API to get current instrument information.

### Insufficient Balance

Your account doesn't have enough funds to place the order.

**Solution**: Check your account balance before placing orders. Consider the total notional value including fees.

### Invalid Price

The price is outside the allowed range for the instrument.

**Common causes:**

* Price too far from market price
* Price increment doesn't match tick size
* Price negative or zero for instruments that require positive prices

**Solution**: Check current market prices and instrument specifications.

### Invalid Quantity

The quantity doesn't meet instrument requirements.

**Common causes:**

* Quantity below minimum order size
* Quantity above maximum order size
* Quantity doesn't match lot size increment

**Solution**: Review instrument specifications for min/max quantities and lot size.

## Pre-Trade Validation

**Is pre-trade validation available?**

Yes. Use the order preview endpoint:

```bash theme={null}
POST /v1/trading/orders/preview
{
  "accountId": "your-account-id",
  "instrument": "tec-nfl-sbw-2026-02-08-kc",
  "side": "buy",
  "quantity": 100,
  "price": "500"
}
```

This validates the order without submitting it, checking account permissions, balance requirements, price and quantity validity, and instrument status.

## Order Submission

**Is order submission synchronous?** Order submission is synchronous (you get an immediate response), but final acceptance may be asynchronous, rejection can occur after initial acceptance, and final state is delivered via order status updates. Always monitor order status via order status endpoint and order subscription stream.

## Partial Fills

**How are partial fills represented?** Through executions: each execution updates cumulative quantity, remaining quantity calculated as original quantity minus cumulative quantity, order remains active until fully filled or canceled.

**Monitoring fills**: Subscribe to order updates, track `cumulativeQty` and `remainingQty` fields, process execution reports as they arrive.

## Order Best Practices

### 1. Validate Before Submitting

Pre-validate orders before submission: check account balance, verify instrument is active, ensure price is within valid range, confirm quantity meets requirements.

### 2. Use Order Preview

Use the order preview endpoint to validate orders without submitting them.

### 3. Monitor Order Status

Implement order status monitoring: subscribe to order status updates via streaming API, poll order status for important orders, track partial fills and remaining quantity, handle unexpected cancellations.

### 4. Handle Rejections Gracefully

When orders are rejected: log the rejection reason, don't automatically retry without fixing the issue, alert on repeated rejections, review rejection patterns to improve order logic.

## Modifying Orders

### Cancel and Replace

To modify an existing order:

1. Cancel the original order
2. Wait for cancellation confirmation
3. Submit the new order with updated parameters

**Don't submit the new order before confirming cancellation** - you may end up with both orders active.

### Bulk Cancellations

When canceling multiple orders: use bulk cancel endpoints when available, handle partial success (some orders canceled, others failed), verify cancellation status for critical orders.

## Order Timing

### Market Hours

Orders can only be placed during market hours. Check the instrument trading schedule: pre-market, regular trading hours, post-market, and closed periods.

### Order Expiration

Set appropriate time-in-force (TIF) values: **Day** (order expires at end of trading day), **GTC** (Good-til-Canceled - order remains active until filled or canceled), **IOC** (Immediate-or-Cancel - execute immediately or cancel), **FOK** (Fill-or-Kill - execute entire order immediately or cancel).

## Rate Limits for Order Operations

Order operations have specific rate limits: maximum orders per second, maximum cancellations per second, maximum modifications per second.

Exceeding these limits results in 429 errors. See [Rate Limits](/trader-guide/rate-limits) for details.

## Order Fill Notifications

Monitor order fills through **Order stream** (real-time updates on order status), **Trade stream** (individual fill notifications), and **Position updates** (reflected in position changes).

**Don't rely on polling** - use streaming APIs for real-time order updates.

## Testing Order Logic

### Pre-production Environment

Always test order logic in pre-production: same API behavior as production, test with dummy funds (no real money), verify order validation logic, test error handling.

### Order Scenarios to Test

1. **Valid orders** - Confirm successful submission and fills
2. **Invalid orders** - Verify proper error handling
3. **Partial fills** - Handle partial execution correctly
4. **Order modifications** - Test cancel/replace logic
5. **Rate limiting** - Verify backoff behavior
6. **Network failures** - Test retry logic
7. **Market closed** - Handle off-hours submissions

## Reporting Order Issues

When reporting order-related issues, include order ID (if available), timestamp, order parameters (instrument, side, quantity, price), account ID, expected behavior vs. actual behavior, and environment (dev, preprod, prod).
