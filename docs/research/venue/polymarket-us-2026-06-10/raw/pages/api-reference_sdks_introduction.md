> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Introduction

> Official SDKs for the Polymarket US API

Python and TypeScript SDKs for integrating with the Polymarket US API. Both libraries handle authentication, request signing, and provide typed interfaces for all endpoints.

<Warning>
  You must download the [Polymarket US iOS app](https://apps.apple.com/us/app/polymarket/id6648798962), create an account, and complete identity verification before you can generate API keys.
</Warning>

<Card title="Generate API Keys" icon="key" href="https://polymarket.us/developer">
  Visit the developer portal to generate your API keys. Your private key will be shown only once.
</Card>

## Choose Your SDK

<CardGroup cols={2}>
  <Card title="Python SDK" icon="python" href="/api-reference/sdks/python/quickstart">
    Sync and async support. Python 3.10+.

    [GitHub](https://github.com/Polymarket/polymarket-us-python) · [PyPI](https://pypi.org/project/polymarket-us/)
  </Card>

  <Card title="TypeScript SDK" icon="js" href="/api-reference/sdks/typescript/quickstart">
    Full TypeScript types. Node.js 18+.

    [GitHub](https://github.com/Polymarket/polymarket-us-typescript) · [npm](https://www.npmjs.com/package/polymarket-us)
  </Card>
</CardGroup>

## Installation

<CodeGroup>
  ```bash Python theme={null}
  pip install polymarket-us
  ```

  ```bash TypeScript theme={null}
  npm install polymarket-us
  ```
</CodeGroup>

## Features

* **Automatic authentication** - request signing handled internally
* **Type safety** - Full typing for all requests and responses
* **WebSocket support** - Real-time market data and order updates
* **Error handling** - Typed exceptions for all error cases

## API Coverage

| Resource  | Methods                                                                                   |
| --------- | ----------------------------------------------------------------------------------------- |
| Events    | `list`, `retrieve`, `retrieveBySlug`                                                      |
| Markets   | `list`, `retrieve`, `retrieveBySlug`, `book`, `bbo`, `settlement`                         |
| Orders    | `create`, `list`, `retrieve`, `cancel`, `modify`, `cancelAll`, `preview`, `closePosition` |
| Portfolio | `positions`, `activities`                                                                 |
| Account   | `balances`                                                                                |
| Series    | `list`, `retrieve`                                                                        |
| Sports    | `list`, `teams`                                                                           |
| Search    | `query`                                                                                   |
| WebSocket | `private`, `markets`                                                                      |

## Quick Example

<CodeGroup>
  ```python Python theme={null}
  from polymarket_us import PolymarketUS

  client = PolymarketUS(
      key_id="your-key-id",
      secret_key="your-secret-key",
  )

  # Get markets and place an order
  markets = client.markets.list({"limit": 10})
  order = client.orders.create({
      "marketSlug": "your-market-slug",
      "intent": "ORDER_INTENT_BUY_LONG",
      "type": "ORDER_TYPE_LIMIT",
      "price": {"value": "0.555", "currency": "USD"},
      "quantity": 0.5,
  })
  ```

  ```typescript TypeScript theme={null}
  import { PolymarketUS } from 'polymarket-us';

  const client = new PolymarketUS({
    keyId: 'your-key-id',
    secretKey: 'your-secret-key',
  });

  // Get markets and place an order
  const markets = await client.markets.list({ limit: 10 });
  const order = await client.orders.create({
    marketSlug: 'your-market-slug',
    intent: 'ORDER_INTENT_BUY_LONG',
    type: 'ORDER_TYPE_LIMIT',
    price: { value: '0.555', currency: 'USD' },
    quantity: 0.5,
  });
  ```
</CodeGroup>
