> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Error Handling & Best Practices

> Handle errors, implement reconnection strategies, and follow best practices with Python

Learn how to handle errors gracefully, implement robust reconnection strategies, and follow production-ready best practices for gRPC streaming in Python.

## gRPC Status Codes

gRPC uses standard status codes to indicate errors. Understanding these codes is essential for proper error handling in Python.

### Common Status Codes

| Code | Name                | Description                | Action                                                                        |
| ---- | ------------------- | -------------------------- | ----------------------------------------------------------------------------- |
| `0`  | `OK`                | Success                    | Continue processing                                                           |
| `1`  | `CANCELLED`         | Operation canceled         | Clean up resources                                                            |
| `3`  | `INVALID_ARGUMENT`  | Invalid request parameters | Fix request and retry. See [Request Parameters](proto-reference) for details. |
| `4`  | `DEADLINE_EXCEEDED` | Operation timeout          | Retry with backoff                                                            |
| `7`  | `PERMISSION_DENIED` | Insufficient permissions   | Check account permissions                                                     |
| `14` | `UNAVAILABLE`       | Service unavailable        | Reconnect with backoff                                                        |
| `16` | `UNAUTHENTICATED`   | Authentication failed      | Refresh token and retry                                                       |

### Checking Error Codes in Python

```python theme={null}
import grpc

try:
    for response in response_stream:
        # Process response
        pass

except grpc.RpcError as e:
    status_code = e.code()

    if status_code == grpc.StatusCode.UNAUTHENTICATED:
        print("Authentication failed")
    elif status_code == grpc.StatusCode.UNAVAILABLE:
        print("Service unavailable")
    elif status_code == grpc.StatusCode.INVALID_ARGUMENT:
        print("Invalid request")
    else:
        print(f"Error: {status_code} - {e.details()}")
```

***

## Common Errors and Solutions

### 1. UNAUTHENTICATED - Authentication Failed

**Causes:**

* Invalid access token
* Expired access token
* Missing authorization metadata

### 2. UNAVAILABLE - Service Unavailable

**Causes:**

* Network connectivity issues
* Server temporarily unavailable
* Firewall blocking connection

### 3. INVALID\_ARGUMENT - Bad Request

**Causes:**

* Invalid symbol
* Invalid depth value
* Malformed request

***

## Next Steps

<CardGroup cols={2}>
  <Card title="Authentication" icon="key" href="/streaming-endpoints/authentication">
    Review authentication best practices
  </Card>

  <Card title="Market Data Stream" icon="chart-line" href="/streaming-endpoints/market-data-stream">
    Learn about market data streaming
  </Card>

  <Card title="Order Stream" icon="file-invoice" href="/streaming-endpoints/order-stream">
    Learn about order streaming
  </Card>
</CardGroup>
