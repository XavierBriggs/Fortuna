> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Getting Started with gRPC Streaming

> Quick start guide for setting up gRPC streaming with Python

This guide will help you set up gRPC streaming with Python and connect to your first data stream in minutes.

## Prerequisites

Before you begin, ensure you have:

* **API Credentials**: Client ID from [authentication setup](/trader-guide/authentication)
* **Python 3.7+**: Python development environment
* **Network Access**: Ability to connect to `grpc-api.preprod.polymarketexchange.com:443`

## Step 1: Install Python gRPC Libraries

```bash theme={null}
pip install grpcio grpcio-tools protobuf requests
```

**Required packages:**

* `grpcio`: gRPC runtime library
* `grpcio-tools`: Tools for generating Python code from proto files
* `protobuf`: Protocol buffer runtime
* `requests`/`httpx`: For REST API authentication

## Step 2: Obtain Protocol Buffer Definitions

Proto files define the gRPC service interfaces and message structures. You can obtain them in two ways:

### Option A: Download Proto Files

Download the proto files directly: [polymarket-protos.zip](https://drive.google.com/uc?export=download\&id=1tM8BY-hQrv1cLtwfs7nPypUZkuxMjYa8)

### Option B: Use Server Reflection

<Info>
  Server reflection allows you to discover service definitions at runtime without proto files. This is useful for dynamic clients and debugging.
</Info>

```bash theme={null}
# Install reflection client
pip install grpcio-reflection

# Use grpcurl to explore services (TLS connection - no -plaintext flag)
grpcurl grpc-api.preprod.polymarketexchange.com:443 list

# Describe a service
grpcurl grpc-api.preprod.polymarketexchange.com:443 describe polymarket.v1.MarketDataSubscriptionAPI
```

## Step 3: Generate Python Client Code

Once you have the proto files, generate Python client code:

```bash theme={null}
python -m grpc_tools.protoc \
  --python_out=. \
  --grpc_python_out=. \
  --proto_path=protos \
  protos/polymarket/v1/marketdatasubscription.proto \
  protos/polymarket/v1/trading.proto \
  protos/polymarket/v1/enums.proto \
  protos/polymarket/v1/types.proto \
  protos/polymarket/v1/refdata.proto
```

This generates:

* `*_pb2.py` files: Message definitions
* `*_pb2_grpc.py` files: Service stubs

## Step 4: Authenticate

<Warning>
  **CRITICAL: Tokens must be refreshed every 3 minutes.**

  Access tokens have a short expiration. Your application MUST implement automatic token refresh before expiration to maintain uninterrupted streaming connections.
</Warning>

### Auth Domains

| Environment    | Auth Domain                |
| -------------- | -------------------------- |
| **Production** | `pmx-prod.us.auth0.com`    |
| **Preprod**    | `pmx-preprod.us.auth0.com` |

Obtain a JWT token using Private Key JWT authentication:

```python theme={null}
import jwt
import uuid
import time
import requests
from cryptography.hazmat.primitives import serialization

# Load your private key
with open("private_key.pem", "rb") as f:
    private_key = serialization.load_pem_private_key(f.read(), password=None)

# Create signed JWT assertion
now = int(time.time())
claims = {
    "iss": "YOUR_CLIENT_ID",
    "sub": "YOUR_CLIENT_ID",
    "aud": "https://pmx-preprod.us.auth0.com/oauth/token",
    "iat": now,
    "exp": now + 300,
    "jti": str(uuid.uuid4()),
}
assertion = jwt.encode(claims, private_key, algorithm="RS256")

# Exchange for access token
response = requests.post(
    "https://pmx-preprod.us.auth0.com/oauth/token",
    json={
        "client_id": "YOUR_CLIENT_ID",
        "client_assertion_type": "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
        "client_assertion": assertion,
        "audience": "YOUR_API_AUDIENCE",
        "grant_type": "client_credentials"
    }
)
access_token = response.json()["access_token"]
```

The response contains your `access_token`:

```json theme={null}
{
  "access_token": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIs...",
  "token_type": "Bearer",
  "expires_in": 180
}
```

<Info>
  The `expires_in` is 180 seconds (3 minutes). Implement automatic token refresh before expiration.
</Info>

<Info>
  See the [Authentication Setup Guide](/trader-guide/authentication) for complete authentication details.
</Info>

## Step 5: Connect to Market Data Stream

Create your first streaming connection:

```python theme={null}
import grpc
from datetime import datetime
from polymarket.v1 import marketdatasubscription_pb2
from polymarket.v1 import marketdatasubscription_pb2_grpc

# Create secure channel
credentials = grpc.ssl_channel_credentials()
channel = grpc.secure_channel('grpc-api.preprod.polymarketexchange.com:443', credentials)

# Create stub
stub = marketdatasubscription_pb2_grpc.MarketDataSubscriptionAPIStub(channel)

# Create request
request = marketdatasubscription_pb2.CreateMarketDataSubscriptionRequest(
    symbols=["tec-nfl-sbw-2026-02-08-kc"],
    unaggregated=False,
    depth=10,
    snapshot_only=False
)

# Set up metadata with authorization
metadata = [
    ('authorization', f'Bearer {access_token}')
]

# Start streaming
print("Starting market data stream...")
response_stream = stub.CreateMarketDataSubscription(request, metadata=metadata)

for response in response_stream:
    if response.HasField('heartbeat'):
        timestamp = datetime.now().strftime('%H:%M:%S')
        print(f"[{timestamp}] Heartbeat received")

    elif response.HasField('update'):
        update = response.update
        timestamp = datetime.now().strftime('%H:%M:%S')
        print(f"\n[{timestamp}] Market Update for {update.symbol}")
        print(f"  Bids: {len(update.bids)}")
        print(f"  Offers: {len(update.offers)}")

        # Get price_scale from instrument metadata (via list_instruments API)
        # You must implement this function to fetch from your instrument cache
        price_scale = get_instrument_price_scale(update.symbol)  # implement this

        if update.bids:
            top_bid_px = update.bids[0].px / price_scale
            print(f"  Top Bid: ${top_bid_px:.4f} x {update.bids[0].qty}")

        if update.offers:
            top_offer_px = update.offers[0].px / price_scale
            print(f"  Top Offer: ${top_offer_px:.4f} x {update.offers[0].qty}")
```

<Note>
  **Price Representation**: All prices are `int64` values. Divide by the instrument's `price_scale` to get the decimal price.

  `price_scale` varies by instrument. Get it from:

  * Instrument metadata via `list_instruments` or `get_instrument_metadata`
  * The `price_scale` field in order responses

  ```python theme={null}
  price_scale = get_instrument_price_scale(symbol)
  decimal_price = raw_price / price_scale
  ```
</Note>

## Common Setup Issues

### Connection Refused

* **Cause**: Firewall blocking outbound gRPC connections
* **Solution**: Ensure port 443 is open for outbound connections

### Authentication Failed

* **Cause**: Invalid or expired access token
* **Solution**: Verify JWT, refresh token if expired (tokens expire every 3 minutes)

### Import Errors

* **Cause**: Generated proto files not in Python path
* **Solution**: Ensure proto files are generated in the correct directory, or add to `PYTHONPATH`:
  ```bash theme={null}
  export PYTHONPATH="${PYTHONPATH}:."
  ```

### Module Not Found: polymarket

* **Cause**: Proto files not generated or in wrong location
* **Solution**: Re-run the `protoc` command from step 3, ensure you're in the correct directory

## Next Steps

<CardGroup cols={3}>
  <Card title="Authentication Guide" icon="key" href="/streaming-endpoints/authentication">
    Deep dive into authentication and token management
  </Card>

  <Card title="Market Data Streaming" icon="chart-line" href="/streaming-endpoints/market-data-stream">
    Learn about all market data streaming features
  </Card>

  <Card title="Order Streaming" icon="file-invoice" href="/streaming-endpoints/order-stream">
    Subscribe to order execution updates
  </Card>
</CardGroup>

## Need Help?

If you encounter issues during setup:

1. Check the [Error Handling Guide](/streaming-endpoints/error-handling)
2. Review the [Market Data Stream](/streaming-endpoints/market-data-stream) or [Order Stream](/streaming-endpoints/order-stream) pages for complete implementations
3. Contact [onboarding@polymarket.us](mailto:onboarding@polymarket.us) for assistance
