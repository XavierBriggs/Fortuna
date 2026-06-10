# DOC-DERIVED SAMPLES — NOT OPERATOR-RECORDED FIXTURES

**READ THIS BEFORE TRUSTING ANY FILE IN THIS DIRECTORY.**

Every `.json` file here was transcribed or constructed from Kalshi's **official
documentation** (OpenAPI spec v3.21.0 + docs pages, archived 2026-06-10 under
`docs/research/venue/kalshi-api-2026-06-10/raw/`). **No file here was recorded from a
live or demo Kalshi API response.** They exist so the adapter's parsing and mapping
logic can be developed and unit-tested before fixtures exist.

Consequences:

- These samples pin the adapter to what the DOCS say, not to what the venue DOES.
- Where the docs are silent or ambiguous (see the **Uncertainties /
  fixture-confirmation checklist**, 27 items, in
  `docs/research/venue/kalshi-api-2026-06-10/research.md`), the corresponding sample is
  a **placeholder** and is marked `FIXTURE-NEEDED` below.
- **The Kalshi adapter is NOT cleared for paper or live use until operator-recorded
  fixtures under `fixtures/kalshi/` confirm every item on that checklist.** Until then
  it is cleared for Sim development only (see `crates/fortuna-venues/src/kalshi/mod.rs`
  module docs).

## File provenance

| File | Source | Status |
|---|---|---|
| `create_order_v2_request.json` | Verbatim example, docs create-order-v2 page / OpenAPI `CreateOrderV2Request.example` | doc-verbatim |
| `create_order_v2_response.json` | Verbatim example, OpenAPI `CreateOrderV2Response.example` | doc-verbatim |
| `create_order_v2_response_filled.json` | Constructed from `CreateOrderV2Response` schema (optional `average_fill_price`/`average_fee_paid`) | doc-derived; FIXTURE-NEEDED (checklist #6) |
| `cancel_order_v2_response.json` | Verbatim example, OpenAPI `CancelOrderV2Response.example` | doc-verbatim |
| `rate_limit_429.json` | Verbatim body from rate-limits page (`{"error": "too many requests"}`) | doc-verbatim |
| `error_envelope.json` | Shape from OpenAPI `ErrorResponse`; **values invented** (docs publish no code catalog) | FIXTURE-NEEDED (checklist #3, #8) |
| `error_409_duplicate.json` | Shape from OpenAPI `ErrorResponse`; the exact `code` string for a duplicate `client_order_id` is **undocumented** | FIXTURE-NEEDED (checklist #7) — adapter must not depend on its contents |
| `orderbook_response.json` | Shape from OpenAPI `GetMarketOrderbookResponse` + research §4; values chosen for a consistent uncrossed book | doc-derived; no-leg pricing of `no_dollars` FIXTURE-NEEDED (checklist #20) |
| `markets_response_page1.json`, `markets_response_page2.json` | Constructed from OpenAPI `Market` required-field list; prices/sizes reuse doc example values | doc-derived; lifecycle `status` values FIXTURE-NEEDED (checklist #21) |
| `series_response.json`, `series_response_maker_half.json` | Constructed from OpenAPI `Series` schema; fee fields per fees research 2026-06-09 | doc-derived; FIXTURE-NEEDED (checklist #22) |
| `event_response.json` | Constructed from OpenAPI `GetEventResponse`/`EventData` (markets array elided; adapter ignores it) | doc-derived |
| `order_response.json`, `orders_response.json` | Constructed from OpenAPI `Order` required-field list | doc-derived; cancel-reconcile statuses FIXTURE-NEEDED (checklist #14, #15) |
| `fills_response.json` | Constructed from OpenAPI `Fill` required-field list; ids reuse the docs' WS fill example | doc-derived; `fee_cost` typing + cursor semantics FIXTURE-NEEDED (checklist #17, #19) |
| `balance_response.json` | Constructed from OpenAPI `GetBalanceResponse` | doc-derived; FIXTURE-NEEDED (checklist #19) |
| `positions_response.json` | Constructed from OpenAPI `GetPositionsResponse`/`MarketPosition` | doc-derived |
| `settlements_response.json` | Constructed from OpenAPI `Settlement` (`fee_cost` example value "0.3400" is the spec's own example) | doc-derived; units mix FIXTURE-NEEDED (checklist #19) |

When an operator records the real fixtures, add them under `fixtures/kalshi/`, point the
tests at them, and only then consider retiring these samples.
