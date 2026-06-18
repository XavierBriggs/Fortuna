> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Error Handling

> Understanding and handling API errors

## Connection-Level Errors

Before receiving HTTP status codes, you may encounter connection-level errors:

**Common connection errors:**

* `ECONNRESET` - Connection reset by peer
* `EPIPE` - Broken pipe
* `ECONNREFUSED` - Connection refused
* `ETIMEDOUT` - Connection timeout
* `SocketError: other side closed` - ALB timeout (10 minutes)
* `14 UNAVAILABLE` (gRPC) - Stream dropped

<Card title="Connection Issues Guide" icon="plug" href="/trader-guide/connection-issues">
  See the full Connection Issues guide for detailed troubleshooting of network errors, ALB timeouts, gRPC streams, keepalive settings, and reconnection strategies
</Card>

***

## HTTP Status Codes

<Info>
  **4xx errors are client-side errors.** These indicate problems with your request that you need to fix (invalid data, missing authentication, wrong permissions, etc.). Debug these by examining your request.

  **5xx errors are server-side errors.** These indicate problems on the API server. Retry these with exponential backoff.
</Info>

### 400 Bad Request

Your request format is invalid.

**Common causes:**

* Invalid JSON format
* Missing required fields
* Invalid field values (wrong type, out of range, etc.)
* Invalid parameter combinations

**Solution**: Check the error message in the response for specific details about what's wrong.

### 401 Unauthorized

Your JWT token is missing, invalid, or expired.

**Solution**: Request a new token from Auth0 and retry your request.

See [Authentication](/trader-guide/authentication-troubleshooting) for detailed troubleshooting.

### 403 Forbidden

Your token is valid but doesn't have the required scope for the endpoint, or the `x-participant-id` header is missing or incorrect.

**Common causes:**

* Missing required scope for the endpoint
* Missing `x-participant-id` header on an account-scoped endpoint (trading, positions, reports)
* Incorrect `x-participant-id` value

**Solution**: Verify your token scopes and ensure `x-participant-id` is included for account-scoped endpoints. If you don't know your participant ID, call `GET /v1/whoami` or `GET /v1/users` and put your firm and user into the firms/`<YOURFIRM>`/users/`<USER>` format. Note you will have one firm but can have multiple users.

See [Authentication](/trader-guide/authentication-troubleshooting) for detailed troubleshooting.

### 404 Not Found

The endpoint or resource doesn't exist.

**Common causes:**

* Incorrect API path
* Resource ID doesn't exist
* Typo in the URL

**Solution**: Verify the endpoint path and resource ID are correct.

### 405 Method Not Allowed

The HTTP method (GET, POST, etc.) is not supported for this endpoint.

**Common causes:**

* Using GET on a POST-only endpoint
* Using POST on a GET-only endpoint

**Solution**: Check the API documentation for the correct HTTP method.

### 409 Conflict

The request conflicts with the current state of the resource.

**Common causes:**

* Duplicate order with same ClOrdID
* Attempting to cancel an already-filled order
* Resource already exists

**Solution**: Check the current state of the resource and adjust your request.

### 413 Payload Too Large

Your request body exceeds the maximum allowed size.

**Common causes:**

* Sending too many items in a bulk request
* Large text fields or descriptions
* Batch operations with too many records

**Solution**: Reduce the request size or split into multiple smaller requests.

### 422 Unprocessable Entity

Your request is well-formed but contains semantic errors.

**Common causes:**

* Invalid business logic (e.g., order quantity exceeds position limits)
* Instrument not tradable in current state
* Violates trading rules or risk limits

**Solution**: Check the error message for specific validation failures.

### 429 Too Many Requests

You've exceeded the rate limit.

**Solution**: Check the `Retry-After` header in the response and wait that long before retrying.

See [Rate Limits](/trader-guide/rate-limits) for details and troubleshooting.

### 500 Internal Server Error

An unexpected error occurred on the server.

**Solution**: These are usually temporary. Retry your request after a short delay (1-2 seconds). If errors persist for more than a few minutes, contact support.

### 502 Bad Gateway

The API gateway cannot reach the backend service.

**Solution**: This usually resolves within 60 seconds. Retry your request with exponential backoff. If issues persist, contact support.

### 503 Service Unavailable

The API is temporarily unavailable.

**Solution**: This usually resolves within 60 seconds. Retry your request with exponential backoff. If issues persist, check the status page or contact support.

### 504 Gateway Timeout

Your request took too long to process (over 30 seconds).

**Common causes:**

* Large data queries without pagination
* Complex report generation
* Slow network connections

**Solutions:**

* Use pagination for large result sets (limit/offset parameters)
* Break large operations into smaller requests
* Download reports as files rather than querying all data
* Consider polling for status updates instead of waiting synchronously

## Retry Strategy

**Retry these errors** with exponential backoff:

* **429** (rate limit) - Always retry with backoff
* **500, 502, 503, 504** (server errors) - Retry up to 3-5 times

**Don't retry these errors:**

* **400** (bad request) - Fix your request instead
* **401** (unauthorized) - Get a new token first
* **403** (forbidden) - Request won't succeed without additional scopes
* **404** (not found) - Resource doesn't exist
* **405** (method not allowed) - Use the correct HTTP method
* **409** (conflict) - Resolve the conflict first
* **413** (payload too large) - Reduce request size
* **422** (unprocessable entity) - Fix validation errors

## Exponential Backoff

Implement this retry pattern:

```
Attempt 1: Immediate
Attempt 2: Wait 1 second
Attempt 3: Wait 2 seconds
Attempt 4: Wait 4 seconds
Attempt 5: Wait 8 seconds
Max: 5 attempts, ~15 seconds total
```

For 429 errors, use the `Retry-After` header value instead of the exponential backoff schedule.

## Intermittent Failures

**My request works sometimes but fails other times**

This could indicate:

* **Intermittent rate limiting** - You're close to the rate limit threshold
* **Token expiration** - Your token expires mid-session
* **Network issues** - Temporary connectivity problems

**Solution**: Implement proper error handling with retries for transient errors (429, 502, 503, 504).

## Debugging Failed Requests

**How to debug:**

1. **Check response body**: Error messages include details about what went wrong
2. **Log request/response**: Keep records of your API calls for debugging
3. **Test with curl**: Isolate whether the issue is with your code or the API
4. **Verify token**: Decode your JWT at jwt.io to check claims and expiration
5. **Check environment**: Ensure you're using the correct API endpoint for your environment

## Logging Best Practices

**Which identifiers should be logged?**

At minimum, log these identifiers for troubleshooting:

* `accountId` - Trading account
* `orderId` - Order identifier
* `execId` - Execution identifier
* `tradeId` - Trade identifier
* `traceId` - Request trace ID (when present in responses)

**Example log entry:**

```
2024-01-15 10:30:45 | ORDER_SUBMITTED | accountId=acc_123 | orderId=ord_456 | instrument=tec-nfl-sbw-2026-02-08-kc | side=buy | qty=100
```

**Should error text be used for logic?**

No. Never parse error message text in your code.

**Use instead:**

* Structured error codes
* HTTP status codes
* Specific error fields in response

Error message text may change. Error codes are stable.

## Reporting Issues

Contact support with:

* Environment (dev, preprod, prod)
* Timestamp of the issue
* Request details (endpoint, method, sanitized request body)
* Response (status code, error message)
* Your Client ID (never share your Client Secret)
* Relevant identifiers (accountId, orderId, tradeId, traceId)
