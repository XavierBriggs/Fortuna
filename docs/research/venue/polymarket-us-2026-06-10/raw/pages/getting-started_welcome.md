> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Welcome

> Polymarket US

<div className="relative overflow-hidden">
  <a href="https://docs.polymarket.com" target="_blank" className="group flex items-center gap-3 mx-4 md:mx-24 mt-6 px-5 py-3.5 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-blue-500 transition-all duration-200 no-underline">
    <span className="text-sm text-gray-600 dark:text-zinc-400">Looking for <span className="font-semibold text-gray-800 dark:text-zinc-200">Polymarket International</span> documentation?</span>
    <span className="ml-auto text-sm font-medium text-gray-500 dark:text-zinc-400 group-hover:translate-x-0.5 transition-transform duration-200">Visit International Docs →</span>
  </a>

  <div className="relative z-10 pb-18 pt-8 max-w-6xl mx-auto ">
    <h1 className="block text-3xl px-4  md:px-24 font-semibold text-gray-900 dark:text-zinc-50 tracking-tight">
      Polymarket US <img src="https://mintcdn.com/polymarketusdocs/SEXtdqzXAX6KiYUU/icons/us-flag.svg?fit=max&auto=format&n=SEXtdqzXAX6KiYUU&q=85&s=662eb668d07aa4a45fd6731b88b61261" alt="US flag" className="inline-block w-7 h-7 align-baseline" width="36" height="36" data-path="icons/us-flag.svg" /> Documentation
    </h1>

    <div className="max-w-2xl px-4  md:px-24 mt-4 text-lg text-gray-500 dark:text-zinc-500">
      Build on the world's largest prediction market.
    </div>

    <div className="grid grid-cols-1 md:grid-cols-2 gap-8 mt-12 px-4  md:px-24 ">
      <div className="flex flex-col justify-center ">
        <h2 className="text-xl font-semibold text-gray-900 dark:text-zinc-50">
          Developer Quickstart
        </h2>

        <p className="mt-3 text-gray-500 dark:text-zinc-400">
          Make your first API request in minutes. Learn the basics of the Polymarket US platform, fetch market data, place orders, and redeem winning positions.
        </p>

        <div className="mt-6">
          <a href="/getting-started/quickstart" className="inline-flex items-center px-4 py-2 text-sm font-medium text-white bg-primary rounded-full hover:bg-indigo-700 transition-colors">
            Get Started →
          </a>
        </div>
      </div>

      <CodeGroup>
        ```typescript TypeScript theme={null}
        import { PolymarketUS } from 'polymarket-us';

        const client = new PolymarketUS({
          keyId: process.env.POLYMARKET_KEY_ID,
          secretKey: process.env.POLYMARKET_SECRET_KEY,
        });

        const order = await client.orders.create({
          marketSlug: 'chiefs-super-bowl-lx',
          intent: 'ORDER_INTENT_BUY_LONG',
          type: 'ORDER_TYPE_LIMIT',
          price: { value: '0.55', currency: 'USD' },
          quantity: 100,
          tif: 'TIME_IN_FORCE_GOOD_TILL_CANCEL',
        });
        ```

        ```python Python theme={null}
        import os
        from polymarket_us import PolymarketUS

        client = PolymarketUS(
            key_id=os.environ["POLYMARKET_KEY_ID"],
            secret_key=os.environ["POLYMARKET_SECRET_KEY"],
        )

        order = client.orders.create({
            "marketSlug": "chiefs-super-bowl-lx",
            "intent": "ORDER_INTENT_BUY_LONG",
            "type": "ORDER_TYPE_LIMIT",
            "price": {"value": "0.55", "currency": "USD"},
            "quantity": 100,
            "tif": "TIME_IN_FORCE_GOOD_TILL_CANCEL",
        })
        ```
      </CodeGroup>
    </div>
  </div>

  <div className="max-w-6xl mx-auto px-4  md:px-24 pb-12">
    <h2 className="text-2xl font-semibold text-gray-900 dark:text-zinc-50">
      Get Familiar with Polymarket US
    </h2>

    <p className="mt-2 text-gray-500 dark:text-zinc-400 max-w-2xl">
      Learn the fundamentals, explore our APIs, and start building on the world's largest prediction market.
    </p>

    <div className="mt-8">
      <CardGroup cols={2}>
        <Card title="Quickstart" icon="rocket" href="/getting-started/quickstart">
          Set up your environment and make your first API call in minutes.
        </Card>

        <Card title="Core Concepts" icon="lightbulb" href="/concepts/events-and-markets">
          Understand markets, events, and how trading works.
        </Card>

        <Card title="API Reference" icon="code" href="/api-reference/introduction">
          Explore REST endpoints, WebSocket streams, and authentication.
        </Card>

        <Card title="SDKs" icon="cube" href="/api-reference/sdks/introduction">
          Official Python and TypeScript libraries for faster development.
        </Card>
      </CardGroup>
    </div>
  </div>
</div>
