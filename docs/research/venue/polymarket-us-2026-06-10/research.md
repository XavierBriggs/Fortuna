# Polymarket US — Venue Research (retrieved 2026-06-10)

Scope: everything needed to decide whether/how to build the FORTUNA Polymarket US
adapter (currently `fortuna_venues::polymarket::PolymarketUsStub`, fixture-gated per
GAPS.md). Covers entity/regulatory status, the full API surface, auth, streaming, fees,
market structure, settlement, rate limits/errors, and the fixture checklist an operator
must record before the adapter is built. All API shapes are taken verbatim from the
official docs at **docs.polymarket.us** (Mintlify site; append `.md` to any page URL for
a clean markdown render — same trick as Kalshi). Raw page captures are archived under
`raw/pages/`, OpenAPI schema JSON under `raw/schemas/`, and non-docs web sources under
`raw/web-sources.md`. Nothing here is invented; silence in the docs is recorded under
"Gaps and unknowns". Per repo rules, no behavior is trusted for live use until an
operator-recorded fixture confirms it.

## Entity disambiguation (read this first)

Three things are called "Polymarket". Sources constantly conflate them:

| Name | Entity | What it is | Docs |
|---|---|---|---|
| **Polymarket (Intl)** | Blockchain Ops Ltd etc. | Crypto/Polygon CLOB, pUSD collateral, EIP-712 orders, non-US | docs.polymarket.com |
| **Polymarket US** | **QCX LLC d/b/a Polymarket US** | CFTC-regulated DCM, fiat USD, KYC US residents | **docs.polymarket.us** |
| **Polymarket Clearing** | **QC Clearing LLC d/b/a Polymarket Clearing** | CFTC-regulated DCO clearing Polymarket US trades | polymarketexchange.com/clearing |

This document is about **Polymarket US (QCX LLC)** only. The earlier in-repo research
`docs/research/venue/polymarket-fees-2026-06-09/research.md` covers both entities'
fees; its US findings are re-verified in §5 below. Note also that the US docs site
itself describes **two distinct API stacks** (§2): a "Retail API" for individual
KYC'd traders and an "Institutional" (Polymarket Exchange / former QCEX) stack for
onboarded firms. One leftover AsyncAPI file on the US docs site
(`developers-old/open-api/connect-wss.json`, archived in `raw/schemas/`) actually
describes the **Intl** CLOB websocket — do not mistake it for the US WS spec.

---

## Sources

All retrieved 2026-06-10. Docs pages archived under `raw/pages/` (flattened filenames,
`/`→`_`); schema JSON under `raw/schemas/`.

| # | URL | Used for |
|---|-----|----------|
| S1 | https://docs.polymarket.us/llms.txt | Full docs index (272 entries) |
| S2 | https://docs.polymarket.us/getting-started/what-is-polymarket-us.md | Entity, DCM/DCO claim, market catalog, vs-Intl |
| S3 | https://docs.polymarket.us/api-reference/introduction.md | Two-part API: api.polymarket.us + gateway.polymarket.us, WS endpoints |
| S4 | https://docs.polymarket.us/api-reference/authentication.md | API keys, Ed25519 signing, headers, 30s skew |
| S5 | https://docs.polymarket.us/api-reference/rate-limits.md | Retail 20 req/s, 429 body, WS-not-polling guidance |
| S6 | https://docs.polymarket.us/fees.md | Fee schedule (verbatim formula, theta table, rounding) |
| S7 | https://docs.polymarket.us/api-reference/orders/overview.md | Endpoints, enums, price/tick/qty rules, batch semantics, reject reasons |
| S8 | https://docs.polymarket.us/api-reference/orders/create-order.md | Full CreateOrder OpenAPI (Order/Execution schemas, commissions) |
| S9 | https://docs.polymarket.us/api-reference/orders/cancel-order.md | Cancel endpoint + empty response |
| S10 | https://docs.polymarket.us/api-reference/orders/cancel-multiple-orders.md / cancel-all-open-orders.md / create-multiple-orders.md / modify-order.md / preview-order.md / close-position-order.md / get-open-orders.md / get-order.md | Order management family |
| S11 | https://docs.polymarket.us/api-reference/websocket/overview.md | WS auth, subscribe protocol, heartbeats, errors |
| S12 | https://docs.polymarket.us/api-reference/websocket/markets.md | Market data / lite / trade message shapes, market states, debouncing |
| S13 | https://docs.polymarket.us/api-reference/websocket/private.md | Order/position/balance streams, execution types, ledger entry types |
| S14 | https://docs.polymarket.us/api-reference/markets/get-markets.md | Market object (feeCoefficient, orderPriceMinTickSize, minimumTradeQty) |
| S15 | https://docs.polymarket.us/api-reference/markets/get-market-book.md | Book shape, MarketStats, settlement calc methods |
| S16 | https://docs.polymarket.us/api-reference/markets/get-market-settlement.md | GET settlement price endpoint |
| S17 | https://docs.polymarket.us/api-reference/portfolio/get-user-positions.md | Positions (decimal fields, cursor+eof) |
| S18 | https://docs.polymarket.us/api-reference/portfolio/get-activities.md | Activities (trade/resolution/balance records, TRADE_STATE_BUSTED) |
| S19 | https://docs.polymarket.us/api-reference/account/get-account-balances.md | Balance/buying-power object |
| S20 | https://docs.polymarket.us/concepts/orders.md | YES-only instrument model, order lifecycle |
| S21 | https://docs.polymarket.us/market-structure/collateral-and-margin.md | Full collateralization, shorting margin, Polymarket Clearing |
| S22 | https://docs.polymarket.us/faqs/general-faqs.md | Maintenance windows, order-cancel-on-maintenance, 503 |
| S23 | https://docs.polymarket.us/faqs/sports-faqs.md | Resolution source hierarchy, LFMP, ties, postponements (the de-facto resolution rulebook) |
| S24 | https://docs.polymarket.us/changelog.md | Timeline: ticks, partial contracts, rate limits, activity types |
| S25 | https://docs.polymarket.us/trader-guide/environments.md | Institutional dev/preprod/prod hosts, Auth0, 3-min tokens |
| S26 | https://docs.polymarket.us/trader-guide/authentication.md | Institutional Private Key JWT (RS256, client_credentials) |
| S27 | https://docs.polymarket.us/trader-guide/error-handling.md | Institutional HTTP error semantics (409 ClOrdID, Retry-After, 504=30s) |
| S28 | https://docs.polymarket.us/trader-guide/rate-limits.md | Institutional limits (100 req/s firm, gRPC 100 msg/s, FIX 150), 429 body |
| S29 | https://docs.polymarket.us/trader-guide/onboarding.md | Institutional onboarding, preprod dummy funds |
| S30 | https://docs.polymarket.us/api-reference/referencedata/list-instruments.md | Institutional Instrument (tickSize, priceScale, fractionalQtyScale, states, EP3) |
| S31 | https://docs.polymarket.us/api-reference/trading/insert-order.md | Institutional InsertOrder (clordId, selfMatchPrevention*, int64 prices) |
| S32 | https://docs.polymarket.us/api-reference/sdks/introduction.md + getting-started/quickstart.md | Official SDKs (`polymarket-us` on npm/PyPI) |
| S33 | https://www.cftc.gov/IndustryOversight/IndustryFilings/TradingOrganizations/49571 | CFTC DCM filing: QCX LLC d/b/a Polymarket US, designated 2025-07-09 |
| S34 | https://www.cftc.gov/IndustryOversight/IndustryFilings/ClearingOrganizations/50836 + https://www.cftc.gov/sites/default/files/filings/orgrules/25/09/rules09092530154.pdf | CFTC DCO: QC Clearing LLC d/b/a Polymarket Clearing |
| S35 | https://polymarketexchange.com/ (+ /clearing/) | Official exchange site; rulebook/participant agreement references |
| S36 | https://www.prnewswire.com/news-releases/polymarket-acquires-cftc-licensed-exchange-and-clearinghouse-qcex-for-112-million-302509626.html | QCEX acquisition ($112M, Jul 2025) |
| S37 | https://www.prnewswire.com/news-releases/polymarket-receives-cftc-approval-of-amended-order-of-designation-enabling-intermediated-us-market-access-302625833.html | Amended Order (Nov 25, 2025), intermediated access |
| S38 | https://www.covers.com/industry/polymarket-removes-waitlist-launches-for-american-ios-users-may-12-2026 (+ other third-party trackers, see raw/web-sources.md) | Launch/waitlist/state availability (THIRD-PARTY) |

Access note: docs.polymarket.us served every page and schema to plain `curl` on
2026-06-10. OpenAPI JSON lives at `/api-reference/oapi-schemas/*.json` (retail) and
`/institutional/oapi-schemas/*.json` (institutional) — all archived in `raw/schemas/`.

---

## 1. Entity / regulatory status

**DOCUMENTED (official):**

- **QCX LLC d/b/a Polymarket US** is a CFTC **Designated Contract Market**, status
  "Designated", designation date **2025-07-09**; "QCX LLC is now operating under the
  assumed name of Polymarket US" (S33, CFTC filing page). An **Amended Order of
  Designation** is on CFTC record (S33; announced Nov 25, 2025 as "enabling
  intermediated US market access", S37).
- **QC Clearing LLC d/b/a Polymarket Clearing** is the CFTC-registered **DCO** that
  clears Polymarket US trades; per its CFTC order it is "permitted to clear fully
  collateralized positions" and "does not employ a margin-setting methodology" (S34).
  The docs' margin page confirms: "Polymarket Exchange operates with
  fully-collateralized contracts… At settlement, Polymarket Clearing holds the
  seller's $1.00 margin to guarantee payout" (S21).
- Polymarket's own docs: "Polymarket US is a fiat-based, US-regulated platform
  operating as a designated contract market (DCM) and derivatives clearing
  organization (DCO) under CFTC oversight. All trading is conducted in US dollars"
  (S2). It is explicitly distinguished from Intl: "Polymarket is our international,
  crypto-based product that operates on blockchain technology" (S2).
- Both companies were acquired by Polymarket as "QCEX" for $112M (announced July 2025,
  S36). The institutional docs and gRPC schemas still carry the pre-acquisition
  exchange-platform lineage: the Instrument message is "Wire-compatible with Connamara
  EP3 Instrument (core fields)" (S30) — QCEX was built on Connamara's EP3 platform,
  which explains `fromEp3` / `ep3Status` fields in the retail schemas.
- **What it lists today**: sports only — "NFL, NBA, NHL, MLB, MLS, CBB, Tennis, golf,
  and more. Politics, culture, finance, and economics coming soon" (S2). Changelog
  confirms active listing expansion through June 2026 (soccer, UFC, NHL props, World
  Cup futures; S24).
- **Who may trade (direct)**: US persons after KYC in the **iOS app** ("Download the
  app… Complete identity verification… you'll see a confirmation in the app", S4).
  API keys come from the same KYC'd account (§3). Intermediated access (FCMs/IBs) and
  partner integrations (ISVs) exist under `partners/` docs; institutional direct
  market access requires firm onboarding (S29).

**THIRD-PARTY ONLY (unverified against official sources — treat as soft):**

- Launch date Dec 3, 2025; invite-only until the waitlist was removed for iOS on
  **May 12, 2026**; Android still waitlist-gated; **no web trading app** as of June
  2026 (S38 + raw/web-sources.md). The API docs' instruction to "download the app"
  for account creation is consistent with app-only onboarding.
- State availability: third-party trackers list AZ, IL, MA, MD, MI, MT, NJ, NV, OH as
  excluded and a Minnesota ban effective 2026-08-01. **No official state list is
  published** in the docs or on polymarketexchange.com. UNKNOWN officially.

---

## 2. API surface map

Polymarket US exposes **three** API stacks (S1, S3, S25). The FORTUNA adapter target
is the **Retail API + public Gateway** (individual KYC'd account, self-serve keys);
the Institutional stack is documented here because it is the only one with a
test environment and richer error/idempotency semantics.

### 2a. Retail authenticated API — `https://api.polymarket.us`

"The Polymarket US API is split into two parts: an authenticated API for trading and
a public API for reading market data" (S3). All paths are `/v1/...`, no version in
the host. Auth: Ed25519 signed headers (§3). Groups (S3, S7):

| Area | Endpoint | Notes |
|---|---|---|
| Create order | `POST /v1/orders` | §6; 200 on success |
| Preview order | `POST /v1/order/preview` | validate + expected fills, no insertion |
| Close position | `POST /v1/order/close-position` | sells all contracts in a market |
| Get open orders | `GET /v1/orders/open` | snapshot of working orders |
| Get order | `GET /v1/order/{orderId}` | full Order incl. commissions; ~100ms after submit (S7) |
| Modify order | `POST /v1/order/{orderId}/modify` | cancel-replace; new price/qty/tif |
| Cancel order | `POST /v1/order/{orderId}/cancel` | body `{marketSlug?}`; **empty response on success** (S9) |
| Cancel all | `POST /v1/orders/open/cancel` | optional market filter |
| Batch create/cancel/modify | `POST /v1/orders/batched[/cancel|/modify]` | ≤20; see batch caveat §6 |
| Positions | `GET /v1/portfolio/positions` | map slug→position; `nextCursor` + `eof` (S17) |
| Activities | `GET /v1/portfolio/activities` | trades, resolutions, balance changes (S18) |
| Balances | `GET /v1/account/balances` | buying power, margin requirement (S19) |
| Private WS | `wss://api.polymarket.us/v1/ws/private` | orders/positions/balance (§4) |
| Markets WS | `wss://api.polymarket.us/v1/ws/markets` | book/lite/trades (§4) |

There is **no fills/executions REST list endpoint on the retail API** — executions
arrive on the private WS (`orderSubscriptionUpdate.execution`), embedded in the Order
(`cumQuantity`, `avgPx`, commissions), or as `ACTIVITY_TYPE_TRADE` activities (S18).
There is no dedicated settlements-list endpoint either; settlement appears as
`ACTIVITY_TYPE_POSITION_RESOLUTION` activities + the public settlement endpoint (§7).

### 2b. Public market-data Gateway — `https://gateway.polymarket.us`

"No API key needed" (S3). Backed by gRPC-gateway (`protos/gateway/market/v1/...`,
S14). Key endpoints (S1, S14–S16):

| Endpoint | Returns |
|---|---|
| `GET /v1/markets` | paged markets; filters: `limit`, `offset`, `orderBy`, `orderDirection`, `id`, `slug`, `archived`, `active`, `closed`, `volumeNumMin/Max`, `startDateMin/Max`, `endDateMin/Max` … (S14) |
| `GET /v1/market/slug/{slug}` | single market by slug (note singular `/market/`) |
| `GET /v1/markets/{slug}/book` | order book + stats + state (§4 shape, same `v1MarketData`) |
| `GET /v1/markets/{slug}/bbo` | lightweight best bid/offer |
| `GET /v1/markets/{slug}/settlement` | `{slug, settlement}` decimal settlement price (§7) |
| `GET /v1/events…`, `/v1/series…`, `/v1/sports…`, `/v1/search`, `/v1/tags…`, `/v1/subjects…` | catalog/navigation |

Pagination on the gateway is `limit`/`offset` (S14); on the retail authenticated API
it is `cursor`/`nextCursor` + `eof` (S17, S18). Two different conventions — do not mix.

### 2c. Institutional "Polymarket Exchange" stack — `https://api.{dev01|preprod|prod}.polymarketexchange.com`

For onboarded firms (Entity Participant Agreement → onboarding@polymarket.us, S29).
REST (`/v1/{service}/{operation}`, e.g. `POST /v1/trading/orders`), gRPC
(`grpc-{env}.polymarketexchange.com:443`, streaming market data / order execution /
dropcopy / ledgers), and FIX (via AWS PrivateLink only, S25). Auth: Private Key JWT →
Auth0 token, §3. Distinct features the retail API lacks: `clordId` (client order id),
`selfMatchPreventionId`/`selfMatchPreventionInstruction`, stop orders, mass-quote
protection, drop copy, valuations/MTM, position+balance ledgers (history floor
`2026-05-01T00:00:00Z`, S24), and raw integer prices scaled by per-instrument
`priceScale` (S30, S31). FORTUNA does not need this stack for a first adapter, but
its **preprod is the only sandbox** (§10) and its FIX/REST reject taxonomies are the
richest documentation of exchange-level error behavior (§9).

### Versioning

All three stacks use a path prefix `/v1/`. Docs are versioned by changelog tags
("Retail API" vs "Institutional API"), current docs release v0.0.44 (June 9, 2026,
S24). No sunset/deprecation policy is documented for the retail API; deprecations so
far are field-level (`netPosition` → `netPositionDecimal` etc., S17) with old fields
retained.

---

## 3. Auth model

### Retail API: Ed25519 key + per-request signature (S4, S8)

- **Key acquisition**: create account + complete KYC in the iOS app → sign in at
  **polymarket.us/developer** ("with the same method you used in the app — Apple,
  Google, or email"; switching sign-in methods "may break your API key access") →
  Create API key → receive **Key ID** (UUID) + **Secret Key** (shown **only once**)
  (S4). Revocation at the same portal. No documented key scopes, no documented limit
  on number of keys. (Contrast Kalshi: RSA keypair, scopes.)
- **Headers** on every authenticated request (S4, verbatim):

  | Header | Value |
  |---|---|
  | `X-PM-Access-Key` | Your Key ID |
  | `X-PM-Timestamp` | Current time in milliseconds |
  | `X-PM-Signature` | A signature generated from your secret key |

- **Signing payload**: `message = f"{timestamp}{method}{path}"` — concatenation with
  no separators; method uppercase; `path` is the URL path (e.g.
  `/v1/portfolio/positions`). The official sample signs the path **without host**;
  whether query params are included is NOT addressed by the sample (the sample GET has
  none) — fixture item #4. (S4 sample code verbatim:
  `message = f"{timestamp}{method}{path}"`.)
- **Algorithm**: **Ed25519** (not RSA, not HMAC). The secret key is base64; the
  sample decodes and takes the first 32 bytes as the private key:
  `ed25519.Ed25519PrivateKey.from_private_bytes(base64.b64decode(SECRET)[:32])`,
  signature is `base64(sign(message))` standard encoding (S4). OpenAPI confirms:
  "Base64-encoded Ed25519 signature of `timestamp + method + path`" (S8).
  Rust mapping: `ed25519-dalek` `SigningKey::from_bytes(&secret[..32])`,
  `base64::engine::general_purpose::STANDARD`.
- **Clock skew is documented** (unlike Kalshi): "Timestamps must be within **30
  seconds** of server time" (S4); OpenAPI: "Must be within 30 seconds of server
  time" (S8).
- **WebSocket auth**: same three headers in the upgrade handshake; signed message is
  `timestamp + "GET" + path` with path `/v1/ws/private` or `/v1/ws/markets` (S11).

### Public gateway: none

`gateway.polymarket.us` endpoints carry `security: []` in the OpenAPI and "No API key
needed" (S3, S14). Rate-limited per IP (§9).

### Institutional: Private Key JWT → OAuth2 (S25, S26, S29)

RSA-2048 keypair per environment (public key submitted at onboarding) → sign a JWT
(RS256, `iss`=`sub`=Client ID, `aud`=Auth0 token URL) → POST to
`https://pmx-{env}.us.auth0.com/oauth/token` with
`client_assertion_type=urn:ietf:params:oauth:client-assertion-type:jwt-bearer`,
`grant_type=client_credentials`, `audience` = REST base URL → bearer token. **"Tokens
must be refreshed every 3 minutes across all environments"** (S25). Account-scoped
calls additionally require an `x-participant-id` header (403 if missing, S27).
Scoped permissions exist (e.g. `read:positions`, S24).

Secrets handling for FORTUNA: `FORTUNA_POLYMARKET_KEY_ID` / `FORTUNA_POLYMARKET_SECRET`
env vars per house rules; the secret is a private signing key, never log it.

---

## 4. WebSocket / streaming (retail)

Two endpoints, same auth as REST in the handshake (S11):

- `wss://api.polymarket.us/v1/ws/private` — orders, positions, account balance
- `wss://api.polymarket.us/v1/ws/markets` — book, lite price data, trades

**Protocol** (S11): JSON messages. Subscribe:

```json
{ "subscribe": { "requestId": "md-sub-1",
                 "subscriptionType": "SUBSCRIPTION_TYPE_MARKET_DATA",
                 "marketSlugs": ["market-slug-1", "market-slug-2"] } }
```

Unsubscribe: `{ "unsubscribe": { "requestId": "original-request-id" } }` — keyed by the
ORIGINAL request id. Responses echo `requestId` + `subscriptionType` and one payload
field. Subscription failure: `{ "requestId": "...", "error": "Error description" }`.
Limit: **max 100 markets per subscription**; use multiple subscriptions beyond that
(S12, S13). NOTE: the overview page (S11) shows `subscription_type` as an integer with
snake_case keys, while the dedicated pages (S12, S13) show camelCase + string enums —
shape conflict, fixture item #20.

**Heartbeats** (S11): server sends `{ "heartbeat": {} }` periodically; "Clients should
respond to heartbeats or implement their own keep-alive mechanism." Interval and
timeout are NOT documented (fixture item #21).

**Markets channel** subscription types (S12):

| Type | Content |
|---|---|
| `SUBSCRIPTION_TYPE_MARKET_DATA` | full book: `marketData{marketSlug, bids[], offers[], state, stats{…}, transactTime}`; levels `{px:{value,currency}, qty}` (qty string, decimals possible), "sorted best-to-worst" |
| `SUBSCRIPTION_TYPE_MARKET_DATA_LITE` | `marketDataLite{currentPx, lastTradePx, bestBid, bestAsk, bidDepth, askDepth, sharesTraded, openInterest}` |
| `SUBSCRIPTION_TYPE_TRADE` | `trade{marketSlug, price, quantity, tradeTime, maker{side,intent}, taker{side,intent}}` |

**The full-book feed is snapshot-only re-publication, not snapshot+delta**: each
`marketData` message carries complete `bids`/`offers` arrays ("the full market data
subscription includes the top levels of the order book", S12). There is NO documented
sequence number, book checksum, or delta message on the retail WS — fundamentally
different from Kalshi's `seq`-tracked `orderbook_delta`. Depth of "top levels" is not
quantified (fixture item #19). Optional `"responsesDebounced": true` batches updates
"at regular intervals rather than on every change" (S12).

**Private channel** subscription types (S13): `SUBSCRIPTION_TYPE_ORDER` (snapshot of
open orders with `eof: true`, then `orderSubscriptionUpdate.execution{id, order{...},
lastShares, lastPx, type, tradeId}`), `SUBSCRIPTION_TYPE_ORDER_SNAPSHOT`,
`SUBSCRIPTION_TYPE_POSITION` (before/after position + `entryType` + `tradeId`),
`SUBSCRIPTION_TYPE_ACCOUNT_BALANCE` (snapshot + `balanceChange` updates). Empty
`marketSlugs` = all markets. Execution types: `NEW`, `PARTIAL_FILL`, `FILL`,
`CANCELED`, `REPLACE`, `REJECTED`, `EXPIRED`, `DONE_FOR_DAY` (S13, S8). Ledger entry
types include `ORDER_EXECUTION`, `DEPOSIT`, `WITHDRAWAL`, `RESOLUTION`, `COMMISSION`,
`CORRECTION`, `NETTING`, `MANUAL_ADJUSTMENT`, `CONTRACT_EXPIRATION` (S13).

Institutional streaming (gRPC `BiDirectionalStreamMarketData`, order stream, dropcopy,
balance/position ledger streams with `resume_time` replay) is documented under
`streaming-endpoints/` (raw captured) but out of scope for the retail adapter.

---

## 5. Fee schedule — VERIFIES the 2026-06-09 research

Source: S6 (`https://docs.polymarket.us/fees`, fetched 2026-06-10), verbatim:

- "Effective exchange-wide from 3pm ET, Friday April 3, 2026." — unchanged since the
  2026-06-09 capture.
- Formula: **`Fee = Θ × C × p × (1 - p)`** where C = contracts, p = trade price
  ($0.01 to $0.99), Θ = fee coefficient.
- **Taker Θ = 0.05** (max $1.25 per 100-lot at p=$0.50).
- **Maker Θ = −0.0125** — a rebate, "Maker rebate (25% of taker fees) is applied at
  the point of trade"; maker receives up to $0.31 per 100-lot.
- **Rounding: "All fees and rebates are rounded to the nearest $0.01 using banker's
  rounding (round half to even)"** with worked examples ($0.025→$0.02, $0.035→$0.04).
- Fees only on execution: "If your order is canceled, expires, or is rejected, no fee
  is charged." Taker fees deducted at trade time; maker rebates credited at fill
  time. Fees can round to $0.00 on small/extreme-priced trades.
- Promo: ">$250,000 taker volume between May 15, 2026 and June 30, 2026 (inclusive)
  receive a taker rebate of 30% of their total taker fees for that period."

**Verdict on the prior research (docs/research/venue/polymarket-fees-2026-06-09/):
US quadratic taker 0.05 / maker −0.0125 / banker's rounding — CONFIRMED, still
current on 2026-06-10.** The full per-price fee table is archived in
`raw/pages/fees.md`.

**One correction to the 2026-06-09 doc**: it stated "Fees are not a field on
order/execution objects in the published schemas." That is no longer true (or was
missed): the retail Order schema carries **`commissionNotionalTotalCollected`
(Amount), `commissionsBasisPoints` (string), `makerCommissionsBasisPoints` (string)**
and the Execution schema carries **`commissionNotionalCollected` (Amount),
`commissionSpreadPx` (Amount), `aggressor` (bool)** (S8). So per-fill fee data IS
available on the retail API, in addition to balance-ledger entries
(`LEDGER_ENTRY_TYPE_COMMISSION`, S13) and activity types
(`ACTIVITY_TYPE_TAKER_FEE_REBATE`, `ACTIVITY_TYPE_LIQUIDITY_PROGRAM` — added May 26,
2026, S24/S18).

**Runtime fee fields (the GAPS.md "fd/feeSchedule" requirement)**: the retail Market
object has a nullable decimal **`feeCoefficient`** field (S14) — the per-market Θ.
The semantics (taker-only? does a maker coefficient exist per market?) are NOT
documented beyond the field name; the fee page documents only the exchange-wide
schedule. The adapter must read `feeCoefficient` per market at runtime and
fixture-verify how it maps to taker/maker Θ (fixture item #16). On orders,
`commissionsBasisPoints` / `makerCommissionsBasisPoints` give the applied rates per
order (S8).

**Sign/rounding interaction worth flagging for the fee engine**: the maker "fee" is
negative (a credit). The fee table shows maker receives $0.31 at p=0.50 per 100-lot
(raw 0.3125 rounds half-to-even to 0.31) — banker's rounding applies to the rebate
too (S6 table). Incentive programs (deposit, liquidity, market-maker, referral,
volume) exist under `/incentives/*` and pay outside the trade-fee path (S1, S24).

---

## 6. Market structure & order model

### Instrument model (S20, S7)

- Binary YES/NO event contracts; **only the YES side is a tradable instrument**:
  "There's only one instrument per market — the YES side. To trade against an
  outcome, you sell YES (which is the same as buying NO)" (S20). The API accepts
  NO-denominated orders via `intent`/`outcomeSide` but **`price.value` always refers
  to the YES leg**: "To trade the NO side at any price X, set
  `price.value = 1.00 - X`" (S7). (Same yes-leg quoting convention as Kalshi V2.)
- Contract payout: settles at **$1.00** per contract if YES, **$0.00** if NO (S2),
  with non-binary settlement values possible (ties at $0.50, co-winners $1/n, LFMP —
  §7).
- **Price bounds: $0.01–$0.99** ("the exchange's absolute price limits", S7; fee page
  states the same range, S6).
- **Tick size is per-market and can be sub-cent**: read decimal
  **`orderPriceMinTickSize`** from the market object before ordering — "A value of
  `0.005` means half-cent ticks" (S7). First 0.5¢-tick market listed June 1, 2026
  (NBA Finals `aec-nba-ny-sa-2026-06-05`, "expect `orderPriceMinTickSize: 0.005`");
  0.25¢ tick instruments exist in institutional preprod (S24). Changelog warning:
  "always derive tick size from the instrument — do not assume all instruments under
  a single contract type share the same tick" (S24).
  **FORTUNA impact**: prices are NOT integer cents. The integer-`Cents` core cannot
  represent $0.005 ticks; the adapter boundary needs a sub-cent price representation
  (e.g. decicent/millicent integer or `Decimal` at the venue boundary) per the
  GAPS.md "sub-cent tick handling per the cents-only core policy" item.
- **Min order size is per-market**: decimal **`minimumTradeQty`** ("0.01 means 1% of
  a contract", S7/S14). **Partial contracts**: all newly listed instruments become
  partial-contract markets at **June 11, 2026 5:00 PM ET**; long-dated futures and
  World Cup instruments already are (S24). Quantities are decimals everywhere
  (`quantity` double on orders; `qtyDecimal`/`netPositionDecimal` strings on
  trades/positions; integer fields deprecated, S17/S18).
- Quantity/price normalization: "Extra precision is not part of the public contract
  and can be normalized to the market precision; for example… `quantity: 0.015` can
  be accepted and returned as `0.01`, and `price.value: "0.515"` can be returned as
  `"0.51"`" (S7) — i.e. the venue may silently round, not reject. Fixture item #11.

### Order types & TIF (S8, S7)

- Types: `ORDER_TYPE_LIMIT` (price required) | `ORDER_TYPE_MARKET` (executes at best
  available; supports `cashOrderQty` dollar-sizing and `slippageTolerance{currentPrice,
  bips|ticks}`, ticks take priority; **default = unlimited slippage**, S7).
- TIF enum: `TIME_IN_FORCE_DAY`, `_GOOD_TILL_CANCEL`, `_GOOD_TILL_DATE` (+
  `goodTillTime`), `_IMMEDIATE_OR_CANCEL`, `_FILL_OR_KILL` (S8). (DAY exists here,
  unlike Kalshi.)
- **Post-only**: `participateDontInitiate: true` — "order must rest on the book prior
  to matching (maker only). Order will be rejected if it would immediately match"
  (S8).
- Side encoding: either `intent` (`ORDER_INTENT_BUY_LONG` / `SELL_LONG` / `BUY_SHORT`
  / `SELL_SHORT`) or `outcomeSide` (`OUTCOME_SIDE_YES|NO`) + `action`
  (`ORDER_ACTION_BUY|SELL`); if both sent, outcomeSide+action wins (S7). All enums are
  strings.
- `manualOrderIndicator`: `MANUAL_ORDER_INDICATOR_MANUAL` | `_AUTOMATIC` — "Required
  to indicate whether the order is placed by a human or automated system… Required
  for regulatory compliance" (S7). FORTUNA must send `_AUTOMATIC`.
- `synchronousExecution: true` + `maxBlockTime`: blocks "until the order is filled,
  rejected, canceled, or expired", up to ~10s; docs recommend async + `GET
  /v1/order/{orderId}` (~100ms) for resting orders (S7, S8).
- Response: `CreateOrderResponse{id, executions[]}` — executions only when
  synchronous (S8). HTTP 200 (not 201).
- **An accepted order is not a validated order**: "Invalid prices (below 0.01 or
  above 0.99) are restricted at the exchange level. Since the order is sent to the
  exchange, you will still receive an orderID, but the order will never fill because
  it gets rejected during validation" (S7). The reject arrives as
  `EXECUTION_TYPE_REJECTED` with `orderRejectReason` on the order stream / order
  state. The adapter MUST NOT treat `200 + id` as acceptance — it must confirm via
  order state (`ORDER_STATE_NEW` vs `ORDER_STATE_REJECTED`).
- Order states: `PENDING_NEW → NEW → PARTIALLY_FILLED → FILLED / CANCELED / REPLACED
  / REJECTED / EXPIRED`, plus `PENDING_REPLACE`, `PENDING_CANCEL`, `PENDING_RISK`
  (S8, S7).
- Reject reasons (complete documented enum, S8): `ORD_REJECT_REASON_EXCHANGE_OPTION`
  (generic), `_UNKNOWN_MARKET`, `_EXCHANGE_CLOSED`, `_INCORRECT_QUANTITY`,
  `_INVALID_PRICE_INCREMENT`, `_INCORRECT_ORDER_TYPE`, `_PRICE_OUT_OF_BOUNDS`,
  `_NO_LIQUIDITY`. Unsolicited cancel reasons: `UNSOLICITED_CXL_REASON_CONNECTION_LOSS`,
  `_LOGOUT`, `_EXCHANGE_OPTION`, `_OTHER` (S8).
- **No client order ID on the retail API.** `CreateOrderRequest` has no
  client-supplied idempotency key (S8) — the institutional API has `clordId` (S31) and
  its 409 semantics ("Duplicate order with same ClOrdID", S27), but retail does not.
  Retail duplicate-submission/timeout semantics are UNDOCUMENTED → fixture item #9,
  and FORTUNA's duplicate-order protection must be built client-side (track
  exchange-assigned ids + reconcile open orders after any ambiguous send).
- **Batch endpoints do not confirm per-entry success** (S7 warning, verbatim
  highlights): gateway validation is atomic (any malformed entry → whole batch 400);
  "The exchange may silently ignore unknown `orderId`s"; `canceledOrderIds` /
  `modifiedOrderIds` "echo the request; they do not certify that each ID was acted
  on"; real outcomes come from the order stream. `createdOrderIds` are real
  exchange-assigned IDs but accept/fill/reject still stream async.
- Modify = cancel-replace (`ORDER_STATE_REPLACED`, `EXECUTION_TYPE_REPLACE`); batched
  modify entries are "forwarded to the exchange as a cancel-replace" (S7, S10).

### Self-trade rules

The retail docs document self-match only by example: "buying both YES at 0.60 and NO
at 0.40… causes a **self-match error**" (S7) — i.e. self-matching is rejected, but
which side dies (taker vs maker) and the exact error surface are UNDOCUMENTED on
retail. The institutional API has explicit `selfMatchPreventionId` +
`selfMatchPreventionInstruction` (S31) and a FIX Self-Match Prevention page (S1).
Fixture item #12.

### Market/instrument states

Retail market-data states (book + WS): `MARKET_STATE_OPEN`, `_PREOPEN`, `_SUSPENDED`,
`_HALTED`, `_EXPIRED`, `_TERMINATED`, `_MATCH_AND_CLOSE_AUCTION` (S12, S15).
Institutional instrument states add semantics (S30): PREOPEN = "Orders accepted, no
matching. Dutch Auction on transition to OPEN"; SUSPENDED = "Cancel only";
**HALTED = "no cancels allowed"** (!); EXPIRED = "All resting orders expired";
TERMINATED = "Order book removed, all orders and positions closed"; plus PENDING and
CLOSED. The catalog Market object separately carries `active`/`closed`/`archived`
booleans and `ep3Status` (S14). Mapping catalog flags ↔ book state is undocumented
(fixture item #15).

### Trading calendar / maintenance (S22)

"Nearly 24/7" with a **recurring weekly maintenance window every Thursday 6–8am ET**
(since 2026-04-16; one-off June 11 3–5am EST window announced, S24). During
maintenance: **all open orders are canceled before the window** ("Leaving resting
orders on the book during maintenance would expose traders to stale fills"), **all
API requests return 503**, then connections re-enable and markets move
SUSPENDED→OPEN with **empty books** (S22). The adapter/runner must treat Thursday
06:00 ET as a scheduled mass-cancel + reconcile point.

### Collateral (S21)

Fully collateralized, no leverage, no margin calls: buyers post price; **short
sellers post $1.00/contract** (buying power decreases by $1.00 − sale price);
"If you attempt a trade that would cause your buying power to fall below zero, the
trade will fail automatically." Portfolio-level margining exists across positions
(directional / mutually-exclusive collateral return pages, S1). Balance object
exposes `buyingPower`, `marginRequirement`, `openOrders`, `unsettledFunds`,
`pendingCredit`, `assetNotional`, `assetAvailable`, `balanceReservation` (S19) — all
JSON **numbers** (decimal), not strings; another precision hazard for the adapter.

---

## 7. Settlement / resolution

**Resolution source: in-house per published rules — NOT UMA, NOT an external oracle.**
The US entity resolves markets itself using a documented source hierarchy (S23,
sports — currently the only live category):

- "**Primary source.** The official governing body or sanctioning organization…
  **Secondary sources.** …official competition scorecards, referee or umpire
  reports, press releases, and results databases maintained by the governing body.
  **Tertiary sources.** …the Associated Press, Reuters, ESPN, BBC Sport, official
  team and league websites, and major sports wire services" (S23).
- "Settlement is delayed if the official result is under review. If no official
  result is declared by the Contract's expiration date, the Contract settles at last
  fair market prices."

**There is no "void/refund" state.** Polymarket US's equivalent of Kalshi's voided
market is **settlement at Last Fair Market Price (LFMP)**: "the prevailing fair
market price on the Exchange at a specified moment in time, typically the moment an
official announcement is made (e.g. a cancellation, walkover, or no-contest). It is
NOT the last traded price at market close" (S23). Applied to: canceled/never-replayed
games, postponements beyond the expiration window (typically two weeks), pre-event
withdrawals, no-contests, mid-play abandonment below the official threshold, soccer
home/away flips. Other non-binary outcomes: ties in winner-without-tie markets settle
at **$0.50**; co-winners settle at **$1.00/n rounded DOWN to the nearest tick**;
futures of withdrawn/eliminated participants settle at $0.00 (S23).

**Adapter consequence**: settlement value is a **decimal price in [0,1]**, not an
enum. `GET /v1/markets/{slug}/settlement` returns `{slug, settlement}` with
`settlement` a decimal number; 404 = "Market not found or not settled" (S16). The
Kalshi-adapter pattern of hard-erroring on non-yes/no results maps here to: treat ANY
settlement, including 0.50/LFMP values, as a first-class outcome — the FORTUNA
position model must support partial payouts per contract.

**Settlement metadata** (S15): `MarketStats` carries `settlementPx`,
`settlementPreliminaryFlag` (bool), `settlementSetTime`, and
`settlementPriceCalculationMethod` — an enum of VWAP tiers and EVENT tiers where
`SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_1` = "set as a result of
'PerformResolution'… considered **final** and will no longer be updated", and
`_OVERRIDE` = "overridden by the exchange". The VWAP/daily marks are exchange marks
(for valuation), distinct from final resolution — the preliminary flag + method
distinguish them. `settlementPriceCalculationText` is "free-form text describing the
outcome that determined the settlement".

**Notification surfaces**:

1. Private WS `SUBSCRIPTION_TYPE_POSITION` updates with
   `entryType: LEDGER_ENTRY_TYPE_RESOLUTION` (S13);
2. `GET /v1/portfolio/activities` → `ACTIVITY_TYPE_POSITION_RESOLUTION` with
   before/after positions and `POSITION_RESOLUTION_SIDE_LONG|SHORT|NEUTRAL` (S18);
3. Balance stream `LEDGER_ENTRY_TYPE_RESOLUTION` / `_CONTRACT_EXPIRATION` entries (S13);
4. Public `GET /v1/markets/{slug}/settlement` + market state → `EXPIRED`/`TERMINATED`.

**Settlement timing**: winners receive $1.00/contract automatically, "No margin
calls, reconciliations, or additional obligations" (S21). Concrete latency from
event end → resolution → balance credit is UNDOCUMENTED (fixture item #18).
**Trade busts exist**: `TRADE_STATE_BUSTED` is a documented trade state (S18) — a
cleared fill can be reversed; the adapter's fill ledger must tolerate busts
(fixture item #22).

---

## 8. Rate limits

### Retail API (S5, S7)

- **Global 20 requests/second per API key** across all authenticated endpoints.
- **Public (unauthenticated): 20 req/s per IP** (gateway).
- Enforced "at the edge (Cloudflare) before requests reach the API" (S7).
- 429 body (S5, verbatim): `{ "status": 429, "message": "Too Many Requests" }` —
  note: NOT the institutional shape. No documented `Retry-After` header on retail;
  docs say "wait at least 1 second, then retry with exponential backoff".
- Higher limits: email support@polymarket.us with use case + endpoints (S5).
- Docs push WS over polling (one connection replaces polling of orders/positions/
  balances/bbo/book; S5 table).

### Institutional (S28)

REST trading: 100 req/s per firm (1-min average, bursts allowed); query/report
endpoints have much lower per-endpoint caps (0.5–60 req/min); gRPC ingress 100 msg/s
per firm (egress unlimited); FIX 150 msg/s per session; public 20 req/s/IP. 429 body:
`{ "code": 8, "message": "rate limit exceeded", "details": [] }`; institutional docs
say to honor `Retry-After` (S27).

### FORTUNA I3 mapping

The dual token-bucket invariant (per-venue + per-market) must be configured well
inside 20 req/s; note cancels, creates, and reads share ONE retail budget — a
runaway-cancel loop can starve order placement. Plan capacity: order ops via REST,
market data via WS.

## 8b. Error semantics (retail)

Documented per-endpoint statuses are sparse: create/cancel/modify declare only
**400** ("invalid order request"), **401** ("invalid or missing authentication
token"), **500**; portfolio endpoints add nothing; gateway endpoints declare 404/500
(S8–S10, S14–S16). **No structured error body schema is published for the retail
API** (the only documented body is the 429 `{status, message}`); exact 400/401 bodies
are fixture items #2/#8. Maintenance returns **503 on everything** (S22).
Business-logic rejections do NOT use HTTP errors — they arrive as
`EXECUTION_TYPE_REJECTED` + `orderRejectReason` after a 200 (S7, §6). Timeout
semantics (server-side processing windows, in-flight order disposition on dropped
connections) are UNDOCUMENTED for retail; institutional documents 504 after 30s and
ALB idle timeout of 10 minutes (S27), and FIX has cancel-on-disconnect
(`UNSOLICITED_CXL_REASON_CONNECTION_LOSS` exists in the retail enum too — whether
retail REST/WS sessions ever trigger it is UNKNOWN, fixture item #13).

---

## 9. SDKs and tooling

Official SDKs for the retail API: **`polymarket-us`** on npm (TS, Node 18+) and PyPI
(Python 3.10+); GitHub `Polymarket/polymarket-us-typescript` (S32). They wrap auth
(pass `keyId` + `secretKey`). Useful as a behavioral reference when fixtures are
ambiguous, but the adapter builds against recorded HTTP/WS traffic per repo rules.

---

## 10. Test environment / sandbox status

- **Retail API: NO sandbox is documented.** Nothing in the docs mentions a demo or
  paper environment for `api.polymarket.us`; keys come only from a KYC'd live
  account (S4). Order **preview** (`POST /v1/order/preview`) is the only documented
  no-risk validation tool. UNKNOWN whether support can provision test accounts —
  operator should ask support@polymarket.us.
- **Institutional: dev01 + preprod environments exist** with separate hosts and
  Auth0 tenants (S25); "Your pre-production account will be funded with **dummy
  funds** for testing purposes" (S29). Preprod gets features early and hosts dummy
  instruments for tick-size/partial-contract testing (e.g. 0.25¢ tick
  `aec-nhl-edm-ana-2026-06-15`, S24). Access requires firm onboarding (Entity
  Participant Agreement → onboarding@polymarket.us) — heavier than Kalshi's
  self-serve demo signup.
- Consequence for fixture recording (§12): EITHER the operator records against the
  retail PROD account with minimal size (fees can round to $0 on 1-lot extreme-price
  orders, S6), OR onboards to institutional preprod and accepts that the surfaces
  differ (different auth, different schemas, clordId present). The checklist below
  assumes retail-prod-with-tiny-size as the default path, since that is the API the
  adapter will speak.

---

## 11. Gaps and unknowns (do NOT fill from memory)

1. **No retail sandbox** (§10) — biggest operational gap vs Kalshi. Confirmation
   needed from support whether test accounts exist.
2. **No client order id / idempotency key on retail order entry** (S8). Duplicate
   detection on resubmit-after-timeout is undocumented. (Institutional `clordId` +
   409 exists, S27/S31.)
3. **Retail error body shapes** for 400/401/403(?)/404 are unpublished (only the 429
   `{status, message}` shape is shown, S5).
4. **Signature path details**: whether query strings are part of the signed `path`
   is not stated (sample has no query params); Kalshi explicitly strips them — do not
   assume the same here.
5. **WS message casing conflict**: overview shows snake_case + integer
   subscription_type; channel pages show camelCase + string enums (S11 vs S12/S13).
6. **WS heartbeat cadence/timeout** and reconnect/backfill guarantees (does the
   private stream replay missed executions on resubscribe? `ORDER_SNAPSHOT` exists
   but gap-coverage is undocumented).
7. **Book depth** of `SUBSCRIPTION_TYPE_MARKET_DATA` / REST book ("top levels" —
   how many?), and whether REST book and WS book are the same snapshot pipeline.
8. **`feeCoefficient` semantics** on the Market object (taker Θ only? scaled? is
   there a maker analog per market?) — only the field name is documented (S14).
9. **Trading-hours metadata per market**: "Specific markets may have different
   trading hours" (S22) but the retail Market object exposes no schedule field
   (institutional Instrument has `tradingSchedule`, S30).
10. **State availability list** — third-party only (raw/web-sources.md).
11. **GTD `goodTillTime` format/timezone** on retail create-order (institutional uses
    RFC3339 date-time; retail schema says plain string).
12. **Settlement latency** (event end → resolution → balance credit) unquantified.
13. **Self-match prevention behavior on retail** (which order is rejected/canceled,
    and its error surface).
14. **`ORDER_STATE_PENDING_RISK` triggers** — when does an order sit in risk review?
15. **Rate-limit headers** (X-RateLimit-*, Retry-After) on retail 429s — undocumented;
    only the body is shown.
16. **Non-sports resolution rules** — politics/finance/economics are "coming soon"
    (S2); their resolution-source rules don't exist yet. The DCM rulebook on
    polymarketexchange.com (S35) is the authoritative document to re-read when those
    list.
17. **Spec 5.2 fee drift**: spec still says "Polymarket US flat 10bp taker" — this
    research re-confirms the 2026-06-09 finding that it is wrong (quadratic 0.05 /
    −0.0125). Spec v0.9 touch-up remains operator-blocked (already in GAPS.md).

---

## 12. Fixture checklist (operator recording session)

Same shape as the Kalshi 27-item list
(`docs/research/venue/kalshi-api-2026-06-10/research.md`). Recordings go under
`fixtures/polymarket/`. Default environment: **retail production with minimum size**
(no sandbox exists, §10) — every order fixture should use `minimumTradeQty` on a
liquid market at extreme prices (e.g. $0.02) where fees round to $0.00 and notional
risk is ≤ a few cents. Capture full HTTP request/response (headers minus secrets)
and raw WS frames with timestamps.

**Auth round-trip**
1. Happy-path signed `GET /v1/account/balances` — confirm 200 and the exact balance
   object (numbers vs strings).
2. Auth failure bodies: bad signature, unknown key id, missing header — capture
   exact 401 body shape for adapter error mapping (undocumented).
3. **Skew**: timestamps at −60s, −31s, −29s, +29s, +31s — confirm the documented
   ±30s window and capture the rejection body.
4. **Signed path with query params**: `GET /v1/portfolio/positions?limit=5` signed
   WITH vs WITHOUT the query string — determine which is correct (undocumented).
5. WS handshake auth on both `/v1/ws/private` and `/v1/ws/markets` (signed
   `timestamp + "GET" + path`), plus a failed handshake (bad signature) — HTTP status
   of the rejected upgrade.

**Catalog / market data (gateway)**
6. `GET /v1/markets?active=true&limit=...&offset=...` two pages + `GET
   /v1/market/slug/{slug}` for: (a) a 1¢-tick whole-contract market, (b) a
   **0.5¢-tick** market, (c) a **partial-contract** market (`minimumTradeQty < 1`)
   — capture `orderPriceMinTickSize`, `minimumTradeQty`, `feeCoefficient`,
   `active/closed/archived`, `marketSides`, `outcomePrices` for each.
7. `GET /v1/markets/{slug}/book` and `/bbo` for the same markets — level shapes,
   `state`, decimal `qty` strings on a partial-contract book; confirm depth.
8. Error shapes: unknown slug on market/book/settlement (404 body), malformed offset
   (400 body), and unauthenticated rate-limit 429 from the gateway.

**Orders (retail, min size)**
9. **Resubmit-after-timeout probe**: send `POST /v1/orders`, kill the connection
   before reading the response, resubmit the identical request; then `GET
   /v1/orders/open` — count resulting orders. This decides the adapter's
   duplicate-protection design (no clientOrderId exists).
10. Happy-path GTC limit far from touch: capture `CreateOrderResponse{id}`,
    immediate `GET /v1/order/{orderId}` (expect `ORDER_STATE_PENDING_NEW`→`NEW`),
    and the private-WS `EXECUTION_TYPE_NEW` execution.
11. **Off-tick and out-of-bounds prices**: `price.value: "0.515"` on a 1¢ market
    (silently normalized to 0.51? rejected?), `"0.005"` and `"45"` (expect orderID
    then `EXECUTION_TYPE_REJECTED` + `ORD_REJECT_REASON_PRICE_OUT_OF_BOUNDS` /
    `_INVALID_PRICE_INCREMENT` on the stream — confirm reject reasons and that NO
    HTTP error occurs). Also `quantity` below `minimumTradeQty`
    (`_INCORRECT_QUANTITY`?).
12. **Self-match**: rest a YES bid, then send a crossing NO buy at the equivalent
    price — capture which order dies, the reject/cancel surface, and the documented
    "self-match error" text.
13. `participateDontInitiate: true` crossing the spread → rejection shape
    (HTTP-level or stream-level?).
14. IOC partial fill + FOK no-fill + market order with `slippageTolerance{ticks}`
    exceeded (`ORD_REJECT_REASON_NO_LIQUIDITY`?) — fills on the WS with `lastShares`,
    `lastPx`, `aggressor`, `commissionNotionalCollected`.
15. `synchronousExecution: true` marketable order — `executions[]` array in the HTTP
    response; compare against the same events on the WS.
16. **Fee verification on a real fill**: maker fill + taker fill at a mid-range price
    with size chosen so the fee is non-zero; verify `Fee = Θ·C·p·(1−p)` with
    banker's rounding against `commissionNotionalTotalCollected`,
    `commissionsBasisPoints`, `makerCommissionsBasisPoints`, the balance-ledger
    `LEDGER_ENTRY_TYPE_COMMISSION` entry, and the market's `feeCoefficient`.
17. Modify (`POST /v1/order/{orderId}/modify`): price change on a resting order →
    `EXECUTION_TYPE_REPLACE`, `ORDER_STATE_REPLACED`, and whether the order id
    changes (cancel-replace). Then cancel: 200 **empty body**; cancel of
    already-canceled and of unknown orderId (400? 200? silently ignored?); batch
    cancel with one bogus id ("exchange may silently ignore unknown orderIds" —
    confirm `canceledOrderIds` echoes it anyway).
18. `POST /v1/orders/batched` with one malformed entry → whole-batch 400 (atomic
    gateway validation); with 21 entries → rejection shape.

**WebSocket (market data + private)**
19. `/v1/ws/markets`: subscribe MARKET_DATA + MARKET_DATA_LITE + TRADE on an active
    market; capture ≥10 minutes spanning real trades — measure book depth, update
    cadence, `responsesDebounced: true` vs false on the same market, and whether
    full-book messages are truly stateless snapshots (no seq to track).
20. **Casing probe**: subscribe using camelCase/string-enum form AND snake_case/int
    form — which does the server accept/emit? (Docs conflict.)
21. Heartbeat cadence over ≥10 min idle; stop responding to heartbeats — does the
    server disconnect, and after how long? Are resting orders affected by a private-WS
    disconnect (any cancel-on-disconnect)?
22. Private stream during fixtures 10–17: order snapshot (`eof: true`), every
    execution type observed, `SUBSCRIPTION_TYPE_POSITION` before/after deltas, and
    `SUBSCRIPTION_TYPE_ACCOUNT_BALANCE` changes incl. a `LEDGER_ENTRY_TYPE_COMMISSION`
    and (if observable) `LEDGER_ENTRY_TYPE_RESOLUTION`. Unsubscribe/resubscribe and
    check whether missed executions replay.

**Settlement**
23. Hold a tiny position through a game resolution: capture
    `ACTIVITY_TYPE_POSITION_RESOLUTION`, the resolution ledger entries, `GET
    /v1/markets/{slug}/settlement` (decimal), `settlementPreliminaryFlag` /
    `settlementPriceCalculationMethod` transitions on the book stats, and the final
    market `state`. Record the end-to-end latency.
24. **A non-binary settlement** if one occurs in the window (tie at $0.50, LFMP
    cancellation, co-winner): full record of the same surfaces. If none occurs,
    leave the adapter hard-coded to alert on settlement ∉ {0, 1} until one is
    captured.

**Environment / ops**
25. A **Thursday 6–8am ET maintenance window**: confirm pre-window mass-cancel of a
    resting order (what execution type/unsolicited reason?), 503 bodies during, and
    the SUSPENDED→OPEN transition + empty book after.
26. Rate limit: burst >20 req/s on a read endpoint → 429 body + response headers
    (any `Retry-After`?); confirm the budget is shared across reads and writes.
27. **Quantity precision probe**: on a partial-contract market send `quantity:
    0.015` where `minimumTradeQty: 0.01` — accepted-and-normalized vs rejected;
    capture how the normalized qty appears on order/position/trade records
    (decimal strings vs doubles), to lock the adapter's parsers.

---

## Bottom line for the adapter decision

Polymarket US is real, regulated (QCX LLC DCM + QC Clearing DCO), and has a complete,
self-serve, well-documented retail REST+WS API with runtime-readable per-market fee
and tick parameters. The hard blockers vs the Kalshi adapter are: (1) **no sandbox**
— fixtures must be recorded against live prod with real (tiny) money, or via heavier
institutional preprod onboarding; (2) **no client order id** — FORTUNA's
duplicate-order protection must be redesigned for this venue (fixture #9 decides
how); (3) **sub-cent ticks + decimal quantities + decimal settlements** — three
places the integer-cents core needs an explicit boundary representation; (4)
**order acceptance ≠ validation** (200+id then async reject) — the order-state
machine must reconcile via stream/GET, never trust the create response.
