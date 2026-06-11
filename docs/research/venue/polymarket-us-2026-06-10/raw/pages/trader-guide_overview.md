> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Trader Guide Overview

> Connect to the Polymarket Exchange for direct market access trading

## Who Is This For?

The Trader Guide is designed for:

* **Institutional traders** accessing markets directly
* **Proprietary trading firms** executing strategies programmatically
* **Market makers** providing liquidity
* **Quantitative traders** building automated trading systems

## What You Can Do

With trading access, you can:

| Action                   | Description                              | Access Method            |
| ------------------------ | ---------------------------------------- | ------------------------ |
| **Place Orders**         | Submit limit orders, market orders       | REST API, gRPC           |
| **Cancel Orders**        | Cancel individual or bulk orders         | REST API, gRPC           |
| **Monitor Positions**    | Track your positions and balances        | REST API, gRPC Streaming |
| **Access Market Data**   | Real-time quotes, order book, statistics | REST API, gRPC Streaming |
| **Query Reference Data** | Instruments, symbols, metadata           | REST API                 |

## Key Concepts

### Environments

Polymarket Exchange provides multiple environments for testing and production:

* **Development** - Internal testing environment
* **Pre-production** - UAT environment with test funds
* **Production** - Live trading environment

<Card title="Environments" icon="globe" href="/trader-guide/environments">
  View API endpoints and configuration
</Card>

### Rate Limits

API requests are subject to rate limits to ensure fair usage:

* REST API: Request-based limits per endpoint
* gRPC: Connection and message limits

<Card title="Rate Limits" icon="gauge" href="/trader-guide/rate-limits">
  Understand rate limiting policies
</Card>

### Connection Options

Choose the right protocol for your use case:

* **REST API** - Simple request/response for orders and queries
* **gRPC** - High-performance streaming for market data and order updates
* **FIX** - Industry-standard protocol for institutional trading

<Card title="Connection Options" icon="network-wired" href="/trader-guide/connection-options">
  Compare connection protocols
</Card>

### Schema

Strongly-typed instrument fields for safely identifying sports markets without parsing symbols or slugs.

<Card title="Sports Schema" icon="table" href="/trader-guide/sports-schema">
  Identify markets via instrument metadata
</Card>
