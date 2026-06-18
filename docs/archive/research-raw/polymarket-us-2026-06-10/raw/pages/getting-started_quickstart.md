> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Quickstart

> Make your first API request in minutes.

## Step 1: Get your API keys

1. **Download the app** - Get the [Polymarket US app](https://apps.apple.com/us/app/polymarket/id6648798962) and create an account.

2. **Complete identity verification** - You'll be asked to verify your identity before you can trade or access the API. Once approved, you'll see a confirmation in the app.

<Frame>
  <img src="https://polymarket-upload.s3.us-east-2.amazonaws.com/approved-to-start-trading-6d9aa7abb3.png" alt="Approved to Start Trading" style={{ maxWidth: '300px' }} />
</Frame>

3. **Go to the developer portal** - Visit [polymarket.us/developer](https://polymarket.us/developer) and sign in with the same method you used in the app (Apple, Google, or email).

<Frame>
  <img src="https://polymarket-upload.s3.us-east-2.amazonaws.com/developer-api-access-ae4cff5410.png" alt="Developer Portal" />
</Frame>

4. **Create an API key** - Click to create a new key. You'll get a **Key ID** and a **Secret Key**.

<Frame>
  <img src="https://polymarket-upload.s3.us-east-2.amazonaws.com/create-api-key-825cc3e241.png" alt="Create API Key" />
</Frame>

<Warning>
  Your secret key is shown **only once**. Copy it somewhere safe before closing the dialog.
</Warning>

If you need help getting set up or need an invite code to access the app, email [support@polymarket.us](mailto:support@polymarket.us).

***

## Step 2: Install the SDK

<CodeGroup>
  ```bash TypeScript theme={null}
  npm install polymarket-us
  ```

  ```bash Python theme={null}
  pip install polymarket-us
  ```
</CodeGroup>

<Note>TypeScript requires Node.js 18+. Python requires 3.10+.</Note>

***

## Step 3: Configure the client

<CodeGroup>
  ```typescript TypeScript theme={null}
  import { PolymarketUS } from 'polymarket-us';

  const client = new PolymarketUS({
    keyId: process.env.POLYMARKET_KEY_ID,
    secretKey: process.env.POLYMARKET_SECRET_KEY,
  });
  ```

  ```python Python theme={null}
  import os
  from polymarket_us import PolymarketUS

  client = PolymarketUS(
      key_id=os.environ["POLYMARKET_KEY_ID"],
      secret_key=os.environ["POLYMARKET_SECRET_KEY"],
  )
  ```
</CodeGroup>

***

## Step 4: Fetch market data

No authentication required for public endpoints.

<CodeGroup>
  ```typescript TypeScript theme={null}
  const client = new PolymarketUS();

  const events = await client.events.list({ limit: 10, active: true });
  const market = await client.markets.retrieveBySlug('chiefs-super-bowl');
  const book = await client.markets.book('chiefs-super-bowl');
  ```

  ```python Python theme={null}
  client = PolymarketUS()

  events = client.events.list({"limit": 10, "active": True})
  market = client.markets.retrieve_by_slug("chiefs-super-bowl")
  book = client.markets.book("chiefs-super-bowl")
  ```
</CodeGroup>

***

## Step 5: Place an order

<CodeGroup>
  ```typescript TypeScript theme={null}
  const order = await client.orders.create({
    marketSlug: 'chiefs-super-bowl',
    intent: 'ORDER_INTENT_BUY_LONG',
    type: 'ORDER_TYPE_LIMIT',
    price: { value: '0.55', currency: 'USD' },
    quantity: 100,
    tif: 'TIME_IN_FORCE_GOOD_TILL_CANCEL',
  });
  ```

  ```python Python theme={null}
  order = client.orders.create({
      "marketSlug": "chiefs-super-bowl",
      "intent": "ORDER_INTENT_BUY_LONG",
      "type": "ORDER_TYPE_LIMIT",
      "price": {"value": "0.55", "currency": "USD"},
      "quantity": 100,
      "tif": "TIME_IN_FORCE_GOOD_TILL_CANCEL",
  })
  ```
</CodeGroup>

***

## Check your account

<CodeGroup>
  ```typescript TypeScript theme={null}
  const balances = await client.account.balances();
  const positions = await client.portfolio.positions();
  const openOrders = await client.orders.list();
  ```

  ```python Python theme={null}
  balances = client.account.balances()
  positions = client.portfolio.positions()
  open_orders = client.orders.list()
  ```
</CodeGroup>

***

## Error handling

<CodeGroup>
  ```typescript TypeScript theme={null}
  import {
    AuthenticationError,
    BadRequestError,
    NotFoundError,
    RateLimitError,
  } from 'polymarket-us';

  try {
    const order = await client.orders.create({ marketSlug: '...' });
  } catch (error) {
    if (error instanceof AuthenticationError) {
      console.error('Invalid credentials');
    } else if (error instanceof BadRequestError) {
      console.error('Invalid parameters:', error.message);
    } else if (error instanceof RateLimitError) {
      console.error('Rate limited');
    } else if (error instanceof NotFoundError) {
      console.error('Not found');
    }
  }
  ```

  ```python Python theme={null}
  from polymarket_us import (
      AuthenticationError,
      BadRequestError,
      NotFoundError,
      RateLimitError,
      APITimeoutError,
      APIConnectionError,
  )

  try:
      order = client.orders.create({"marketSlug": "..."})
  except AuthenticationError as e:
      print(f"Invalid credentials: {e.message}")
  except BadRequestError as e:
      print(f"Invalid parameters: {e.message}")
  except RateLimitError as e:
      print(f"Rate limited: {e.message}")
  except NotFoundError as e:
      print(f"Not found: {e.message}")
  ```
</CodeGroup>

| Error                 | Description                    |
| --------------------- | ------------------------------ |
| `AuthenticationError` | Invalid or missing credentials |
| `BadRequestError`     | Invalid request parameters     |
| `NotFoundError`       | Resource not found             |
| `RateLimitError`      | Rate limit exceeded            |
| `APITimeoutError`     | Request timed out              |
| `APIConnectionError`  | Network connection error       |

***

## Next steps

<CardGroup cols={2}>
  <Card title="TypeScript SDK" icon="js" href="/api-reference/sdks/typescript/quickstart">
    Full SDK reference for TypeScript.
  </Card>

  <Card title="Python SDK" icon="python" href="/api-reference/sdks/python/quickstart">
    Full SDK reference for Python.
  </Card>

  <Card title="API Reference" icon="code" href="/api-reference">
    Explore all REST endpoints.
  </Card>

  <Card title="WebSockets" icon="bolt" href="/streaming-endpoints">
    Stream live market data.
  </Card>
</CardGroup>
