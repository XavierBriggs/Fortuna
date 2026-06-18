> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Quickstart

> Place your first order in 5 minutes

This guide walks you through placing your first order on Polymarket US. By the end, you'll have:

1. Authenticated with the API
2. Listed available instruments
3. Checked your account balance
4. Placed a limit order

<Info>
  **Prerequisites:** Complete [Onboarding](/trader-guide/onboarding) first to generate your keys and receive your Client ID.
</Info>

## Step 1: Get an Access Token

Create a signed JWT and exchange it for an access token:

```python theme={null}
import jwt
import uuid
import time
import requests
from cryptography.hazmat.primitives import serialization

# Your credentials (from onboarding)
AUTH_DOMAIN = "pmx-preprod.us.auth0.com"
CLIENT_ID = "your_client_id"
AUDIENCE = "https://api.preprod.polymarketexchange.com"
PRIVATE_KEY_PATH = "private_key.pem"

def get_access_token():
    # Load private key
    with open(PRIVATE_KEY_PATH, 'rb') as f:
        private_key = serialization.load_pem_private_key(f.read(), password=None)

    # Create JWT assertion
    now = int(time.time())
    claims = {
        "iss": CLIENT_ID,
        "sub": CLIENT_ID,
        "aud": f"https://{AUTH_DOMAIN}/oauth/token",
        "iat": now,
        "exp": now + 300,
        "jti": str(uuid.uuid4()),
    }
    assertion = jwt.encode(claims, private_key, algorithm="RS256")

    # Exchange for access token
    response = requests.post(
        f"https://{AUTH_DOMAIN}/oauth/token",
        json={
            "client_id": CLIENT_ID,
            "client_assertion_type": "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
            "client_assertion": assertion,
            "audience": AUDIENCE,
            "grant_type": "client_credentials"
        }
    )
    response.raise_for_status()
    return response.json()["access_token"]

# Get token
access_token = get_access_token()
print("Got access token!")
```

**Required packages:**

```bash theme={null}
pip install PyJWT cryptography requests
```

## Step 2: Verify Authentication

Check that your token works:

```python theme={null}
BASE_URL = "https://api.preprod.polymarketexchange.com"
PARTICIPANT_ID = "firms/YourFirm/users/your-user"  # From onboarding - or call GET /v1/whoami or GET /v1/users to discover users and firm

headers = {
    "Authorization": f"Bearer {access_token}",
    "Content-Type": "application/json",
    "x-participant-id": PARTICIPANT_ID
}

# Check who you are
response = requests.get(f"{BASE_URL}/v1/whoami", headers=headers)
response.raise_for_status()
print(f"Authenticated as: {response.json()}")
```

<Warning>
  **The `x-participant-id` header is required** for all account-scoped endpoints (trading, positions, reports). If you don't know your participant ID, call `GET /v1/whoami` to see your firm, and `GET /v1/users` to list all users and their IDs.
</Warning>

Expected response:

```json theme={null}
{
  "user": "firms/Acme-Trading/users/trader1",
  "userDisplayName": "Trader 1",
  "firm": "Acme-Trading",
  "firmDisplayName": "Acme Trading",
  "firmType": "FIRM_TYPE_TRADER"
}
```

## Step 3: List Available Instruments

Find instruments to trade:

```python theme={null}
# List all tradable instruments
response = requests.post(
    f"{BASE_URL}/v1/refdata/instruments",
    headers=headers,
    json={"tradable_filter": "TRADABLE_FILTER_TRADABLE"}
)
response.raise_for_status()

instruments = response.json().get("instruments", [])
print(f"Found {len(instruments)} instruments")

# Show first 5
for inst in instruments[:5]:
    print(f"  {inst['symbol']}: {inst.get('description', 'N/A')}")
```

<Info>
  Cache the `price_scale` for each instrument - you'll need it to convert prices.
</Info>

## Step 4: Check Your Balance

Verify you have funds to trade:

```python theme={null}
# Get account balance
response = requests.post(
    f"{BASE_URL}/v1/positions/balance",
    headers=headers,
    json={}
)
response.raise_for_status()

balance = response.json()
print(f"Available balance: {balance}")
```

## Step 5: Place Your First Order

Place a limit order to buy:

```python theme={null}
import uuid

# Choose an instrument (use one from Step 4)
symbol = "tec-nfl-sbw-2026-02-08-kc"  # Kansas City Chiefs to win Super Bowl 2026
price_scale = 100  # Get this from instrument metadata

# Order parameters
order = {
    "clord_id": str(uuid.uuid4()),  # Your unique order ID
    "symbol": symbol,
    "side": "SIDE_BUY",
    "type": "ORDER_TYPE_LIMIT",
    "time_in_force": "TIME_IN_FORCE_GOOD_TILL_CANCEL",
    "order_qty": 10,  # Quantity to buy
    "price": int(0.50 * price_scale),  # $0.50 as integer
}

# Submit order
response = requests.post(
    f"{BASE_URL}/v1/trading/orders",
    headers=headers,
    json=order
)

if response.status_code == 200:
    result = response.json()
    print(f"Order placed successfully!")
    print(f"  Order ID: {result['order']['id']}")
    print(f"  State: {result['order']['state']}")
else:
    print(f"Order failed: {response.text}")
```

Expected response:

```json theme={null}
{
  "order": {
    "id": "ord_abc123",
    "clord_id": "your-uuid",
    "symbol": "tec-nfl-sbw-2026-02-08-kc",
    "side": "SIDE_BUY",
    "state": "ORDER_STATE_NEW",
    "order_qty": 10,
    "price": 5000,
    "leaves_qty": 10,
    "cum_qty": 0
  }
}
```

## Step 6: Check Your Order

Verify your order is in the book:

```python theme={null}
# Search for your orders
response = requests.post(
    f"{BASE_URL}/v1/report/orders/search",
    headers=headers,
    json={
        "symbols": [symbol],
        "state_filter": "ORDER_STATE_FILTER_OPEN"
    }
)
response.raise_for_status()

orders = response.json().get("orders", [])
print(f"You have {len(orders)} open orders")

for order in orders:
    price = order['price'] / price_scale
    print(f"  {order['id']}: {order['side']} {order['order_qty']} @ ${price:.2f}")
```

## Step 7: Cancel Your Order (Optional)

Cancel the order if you don't want it to execute:

```python theme={null}
# Cancel by order ID
response = requests.post(
    f"{BASE_URL}/v1/trading/orders/cancel",
    headers=headers,
    json={
        "order_id": "ord_abc123"  # Use the order ID from Step 6
    }
)

if response.status_code == 200:
    print("Order cancelled successfully!")
else:
    print(f"Cancel failed: {response.text}")
```

## Complete Example

Here's the full working script:

```python theme={null}
#!/usr/bin/env python3
"""Polymarket US API Quickstart - Place your first order"""

import jwt
import uuid
import time
import requests
from cryptography.hazmat.primitives import serialization

# Configuration
AUTH_DOMAIN = "pmx-preprod.us.auth0.com"
CLIENT_ID = "your_client_id"  # From onboarding
AUDIENCE = "https://api.preprod.polymarketexchange.com"
PRIVATE_KEY_PATH = "private_key.pem"
BASE_URL = "https://api.preprod.polymarketexchange.com"
PARTICIPANT_ID = "firms/YourFirm/users/your-user"  # From onboarding


def get_access_token():
    with open(PRIVATE_KEY_PATH, 'rb') as f:
        private_key = serialization.load_pem_private_key(f.read(), password=None)

    now = int(time.time())
    claims = {
        "iss": CLIENT_ID,
        "sub": CLIENT_ID,
        "aud": f"https://{AUTH_DOMAIN}/oauth/token",
        "iat": now,
        "exp": now + 300,
        "jti": str(uuid.uuid4()),
    }
    assertion = jwt.encode(claims, private_key, algorithm="RS256")

    response = requests.post(
        f"https://{AUTH_DOMAIN}/oauth/token",
        json={
            "client_id": CLIENT_ID,
            "client_assertion_type": "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
            "client_assertion": assertion,
            "audience": AUDIENCE,
            "grant_type": "client_credentials"
        }
    )
    response.raise_for_status()
    return response.json()["access_token"]


def main():
    # 1. Get access token
    print("1. Authenticating...")
    token = get_access_token()
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json", "x-participant-id": PARTICIPANT_ID}
    print("   Authenticated!")

    # 2. Verify identity
    print("\n2. Checking identity...")
    resp = requests.get(f"{BASE_URL}/v1/whoami", headers=headers)
    resp.raise_for_status()
    print(f"   Logged in as: {resp.json()}")

    # 3. List instruments
    print("\n3. Listing instruments...")
    resp = requests.post(
        f"{BASE_URL}/v1/refdata/instruments",
        headers=headers,
        json={"tradable_filter": "TRADABLE_FILTER_TRADABLE", "page_size": 5}
    )
    resp.raise_for_status()
    instruments = resp.json().get("instruments", [])
    print(f"   Found {len(instruments)} instruments (showing first 5)")

    if not instruments:
        print("   No instruments available. Exiting.")
        return

    # Use first instrument
    symbol = instruments[0]["symbol"]
    price_scale = instruments[0].get("price_scale", 100)
    print(f"   Using: {symbol} (price_scale={price_scale})")

    # 4. Place order
    print("\n4. Placing order...")
    order = {
        "clord_id": str(uuid.uuid4()),
        "symbol": symbol,
        "side": "SIDE_BUY",
        "type": "ORDER_TYPE_LIMIT",
        "time_in_force": "TIME_IN_FORCE_GOOD_TILL_CANCEL",
        "order_qty": 10,
        "price": int(0.50 * price_scale),
    }
    resp = requests.post(f"{BASE_URL}/v1/trading/orders", headers=headers, json=order)

    if resp.status_code == 200:
        result = resp.json()
        order_id = result["order"]["id"]
        print(f"   Order placed: {order_id}")
        print(f"   State: {result['order']['state']}")

        # 5. Cancel order
        print("\n5. Cancelling order...")
        resp = requests.post(
            f"{BASE_URL}/v1/trading/orders/cancel",
            headers=headers,
            json={"order_id": order_id}
        )
        if resp.status_code == 200:
            print("   Order cancelled!")
        else:
            print(f"   Cancel failed: {resp.text}")
    else:
        print(f"   Order failed: {resp.text}")

    print("\nQuickstart complete!")


if __name__ == "__main__":
    main()
```

## Next Steps

<CardGroup cols={2}>
  <Card title="Authentication Deep Dive" icon="key" href="/trader-guide/authentication">
    Token refresh and key rotation
  </Card>

  <Card title="gRPC Streaming" icon="bolt" href="/streaming-endpoints/grpc-overview">
    Real-time order and market data
  </Card>

  <Card title="REST API Reference" icon="book" href="/api-reference/introduction">
    Complete endpoint documentation
  </Card>

  <Card title="Rate Limits" icon="gauge" href="/trader-guide/rate-limits">
    Usage limits and best practices
  </Card>
</CardGroup>
