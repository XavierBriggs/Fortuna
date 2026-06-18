> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Connection Options

## Connectivity Options

The exchange provides two supported integration protocols for client applications:

1. **FIX Protocol**
   * **Type:** Financial Information eXchange (FIX)
   * **Usage:** Standardized message format for order entry, trade execution, and market data.
   * **Benefits:** Widely adopted in financial markets; high reliability for order flow and execution reporting.

2. **REST API with gRPC Streaming**
   * **Type:** HTTP-based REST interface combined with gRPC for real-time streaming.
   * **Usage:** REST for request/response operations (e.g., placing orders, querying market state) and gRPC for low-latency, bidirectional data streams.
   * **Benefits:** Simple integration for HTTP-based clients; gRPC enables efficient real-time data delivery.

Both interfaces provide access to trading functionality and market data, allowing clients to select the protocol that best fits their integration requirements.
