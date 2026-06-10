# Polymarket fee & order-model research (2026-06-09)

Purpose: confirm/refute the spec claim — "Polymarket Intl mostly zero fees with a 0.0625
formula on fee-enabled markets; Polymarket US flat 10bp taker." All sources retrieved
live on 2026-06-09. Where a small-model summary was used (WebFetch), key pages were
re-fetched raw (curl) and quoted verbatim.

**Verdict up front:**

- "Intl mostly zero fees" — **OUTDATED**. Since the March 30, 2026 "Fee Structure V2",
  taker fees apply to nearly every category (only Geopolitics/World Events are free).
- "0.0625 formula on fee-enabled markets" — **historically accurate (Jan–Mar 2026)**,
  now superseded. The Jan 2026 15-minute-crypto fee curve peaked at 1.56% at 50%
  probability, which is exactly feeRate 0.0625 in the p(1−p) formula. Current crypto
  rate is 0.07; other categories 0.03–0.05.
- "Polymarket US flat 10bp taker" — **WRONG on both counts**. Launch-era (Dec 2025)
  schedule was 0.01% (1bp, not 10bp) of contract premium per third-party reports, and
  since April 3, 2026 the US schedule is NOT flat: Fee = Θ × C × p × (1−p) with taker
  Θ = 0.05 and a maker rebate Θ = −0.0125 (makers are PAID).

---

## Sources (URL + retrieval date)

All retrieved 2026-06-09 unless noted.

Official — Polymarket International:

1. https://docs.polymarket.com/trading/fees — canonical fee page (raw HTML fetched)
2. https://docs.polymarket.com/developers/market-makers/maker-rebates-program — maker rebates
   (same content also at /market-makers/maker-rebates.md)
3. https://docs.polymarket.com/trading/taker-rebates.md — Taker Rebate Program (live May 28, 2026)
4. https://docs.polymarket.com/market-makers/liquidity-rewards.md — liquidity rewards
5. https://docs.polymarket.com/trading/orders.md — order types, tick sizes, post-only, neg-risk
6. https://docs.polymarket.com/concepts/order-lifecycle.md — lifecycle, EIP-712, matching
7. https://docs.polymarket.com/trading/orderbook.md — book schema (tick_size, min_order_size)
8. https://docs.polymarket.com/concepts/prices-orderbook.md — price-as-probability range
9. https://docs.polymarket.com/concepts/resolution.md — redemption mechanics
10. https://docs.polymarket.com/trading/gasless.md — relayer/gas sponsorship
11. https://docs.polymarket.com/v2-migration.md and https://docs.polymarket.com/changelog.md —
    CLOB v2 cutover (Apr 28, 2026), fee timeline entries (Jan–May 2026)
12. https://docs.polymarket.com/api-reference/trade/get-trades.md — trade object (`fee_rate_bps`)
13. https://docs.polymarket.com/api-reference/market-data/get-fee-rate.md — GET /fee-rate (`base_fee`)
14. https://help.polymarket.com/en/articles/13364478-trading-fees — help-center fee article
    (updated Apr 28, 2026; contains mixed-era text, see Uncertainties)

Official — Polymarket US (QCX LLC):

15. https://docs.polymarket.us/fees — US fee schedule (raw HTML fetched; "Effective
    exchange-wide from 3pm ET, Friday April 3, 2026")
16. https://docs.polymarket.us/api-reference/orders/create-order.md — order schema
    (OrderType, TimeInForce, intents, participateDontInitiate)
17. https://docs.polymarket.us/api-reference/referencedata/list-instruments.md — `tickSize` per instrument
18. https://docs.polymarket.us/changelog.md — partial contracts (Jun 11), activity types,
    Volume Incentive Program, maintenance windows
19. https://docs.polymarket.us/api-reference/portfolio/get-activities.md — activity enums

News / third party (used only for history and launch dates):

20. https://www.prnewswire.com/news-releases/polymarket-receives-cftc-approval-of-amended-order-of-designation-enabling-intermediated-us-market-access-302625833.html — CFTC Amended Order, Nov 25, 2025
21. https://www.coindesk.com/business/2025/11/25/polymarket-secures-cftc-approval-for-regulated-u-s-return
22. https://www.prnewswire.com/news-releases/polymarket-acquires-cftc-licensed-exchange-and-clearinghouse-qcex-for-112-million-302509626.html — QCEX acquisition ($112M, July 2025)
23. https://www.gate.com/news/detail/15518344 — launch-era US fee schedule ("Taker fee is
    0.01% of the total contract Premium")
24. https://www.theblock.co/post/384461/... (403; headline only) and
    https://www.financemagnates.com/cryptocurrency/polymarket-introduces-dynamic-fees-to-curb-latency-arbitrage-in-short-term-crypto-markets/ — Jan 2026 15-min-crypto fees, "~3.15% on a 50-cent contract"
25. https://marketmath.io/blog/polymarket-fees-explained — March-2026-era per-category
    feeRate/exponent table (third party, unverified)
26. https://github.com/Polymarket/py-clob-client/issues/326 — docs vs API fee discrepancy
    (opened Apr 8, 2026, open/unresolved)
27. http://archive.org/wayback/available?url=docs.polymarket.us/fees — earliest snapshot
    of US fee page is 2026-04-14 (no pre-April archive)

---

## Polymarket Intl fees (formula, worked example)

### Current structure (docs.polymarket.com/trading/fees, retrieved 2026-06-09)

- "Polymarket charges a small taker fee on certain markets. Fees are set by the protocol
  and applied at match time — you don't include fee information in your orders."
- "Makers are never charged fees. Only takers pay fees."
- "Geopolitical and world events markets are fee-free."
- "There are also no Polymarket fees to deposit or withdraw USDC."
- Formula (verbatim): `fee = C × feeRate × p × (1 - p)` where C = number of shares
  traded, p = price of the shares. Fee is symmetric around p = 0.50.

Per-category parameters (verbatim table):

| Category | Taker fee rate | Maker fee rate | Maker rebate |
|---|---|---|---|
| Crypto | 0.07 | 0 | 20% |
| Sports | 0.03 | 0 | 25% |
| Finance | 0.04 | 0 | 25% |
| Politics | 0.04 | 0 | 25% |
| Economics | 0.05 | 0 | 25% |
| Culture | 0.05 | 0 | 25% |
| Weather | 0.05 | 0 | 25% |
| Other / General | 0.05 | 0 | 25% |
| Mentions | 0.04 | 0 | 25% |
| Tech | 0.04 | 0 | 25% |
| Geopolitics | 0 | 0 | — |

Docs' own peak fees per 100 shares at p = $0.50: Crypto $1.75, Sports $0.75,
Finance/Politics/Mentions/Tech $1.00, Economics/Culture/Weather/Other $1.25.

Worked example (current, Crypto): buy 100 shares at $0.50 →
`fee = 100 × 0.07 × 0.50 × 0.50 = $1.75` (3.5% of the $50 notional; 1.75% of the
100-share count). Same trade in Sports: `100 × 0.03 × 0.25 = $0.75`.

Precision: "Fees are rounded to 5 decimal places. The smallest fee charged is 0.00001
USDC. Anything smaller rounds to zero." (Help center, describing the pre-CLOB-v2 sports
rollout, says 4 decimals / 0.0001 pUSD — dev docs treated as current.)

### The "0.0625" claim — historical reconstruction

Timeline from the official changelog (docs.polymarket.com/changelog.md) + news:

- Pre-Jan 2026: trading was fee-free platform-wide (the spec's "mostly zero" era).
- Jan 2026: taker fees quietly introduced on 15-minute crypto markets to fund maker
  rebates (The Block, Cointelegraph/Finance Magnates: fees "can reach approximately
  3.15% on a 50-cent contract").
- Changelog Feb 12, 2026: 5-minute crypto markets "Launched with taker fees enabled.
  Fees follow the same curve as 15-minute crypto markets, **peaking at 1.56% at 50%
  probability**."
- 1.56% peak = 0.015625 per share = `0.0625 × 0.50 × 0.50` → **feeRate (baseRate) was
  0.0625** in the `C × feeRate × p × (1−p)` formula. Cross-check: 0.015625/0.50 = 3.125%
  of notional ≈ the "~3.15%" reported by news. Both views are consistent only with 0.0625.
- Feb 18, 2026: fees extended to NCAAB and Serie A sports markets.
- Mar 6, 2026: fees extended to all new crypto markets (1H/4H/daily/weekly).
- Mar 30, 2026: "Fee Structure V2" — per-category fees on Crypto, Sports, Finance,
  Politics, Economics, Culture, Weather, Tech, Mentions, Other; geopolitics stays free.
  (Crypto effectively moved 0.0625 → 0.07.)
- Mar 31, 2026: REST API — "Fees should now be calculated using the `feeSchedule`
  object within a market."
- Apr 28, 2026: CLOB v2 + pUSD migration. "Fees collected onchain at match time (no
  longer embedded in the signed order)"; order struct drops `feeRateBps`, `nonce`,
  `taker`. Fees now charged in pUSD (previously partly collected in shares for buys).

So: the spec text was a correct snapshot of roughly January–March 2026 and is stale by
three fee regimes as of 2026-06-09.

### Settlement / redemption / gas (Intl)

- Redemption: winning tokens redeem for $1.00 each ("100 winning tokens → $100 pUSD"
  via the CTF collateral adapter). **No settlement or redemption fee is documented
  anywhere in the official docs**; the only fee Polymarket charges is the taker fee at
  match time. 50/50 resolutions redeem each side at $0.50.
- Deposits/withdrawals: no Polymarket fee (intermediaries/bridges may charge; non-pUSD
  deposits on Polygon may incur swap/bridge/gas costs).
- Gas: Polymarket's Relayer sponsors gas for "wallet deployment, token approvals, CTF
  operations (split, merge, and redeem positions), transfers" — users need pUSD only,
  no POL. Trading itself is gasless for the user (orders are EIP-712 signed messages;
  the operator settles on-chain).
- Collateral is pUSD (ERC-20 on Polygon, 1:1 USDC-backed) since Apr 28, 2026.

### How fees appear in the API (Intl)

- Market object: `feesEnabled: true` flags fee-enabled markets; a `feeSchedule` object
  was added Mar 31, 2026; SDK `getClobMarketInfo(conditionID)` returns
  `info.fd = { r: feeRate, e: exponent, to: takerOnly }`.
- Trade objects (GET /data/trades): include `fee_rate_bps` (example value `'30'`).
- GET /fee-rate?token_id=... returns `{ "base_fee": <bps> }` for a token.
- Orders: in CLOB v2 the signed order **no longer carries** `feeRateBps` — fees are
  operator-set at match time. (CLOB v1 orders had a `feeRateBps` field.)
- Known inconsistency: py-clob-client issue #326 (open, Apr 8, 2026) reports /fee-rate
  returning `base_fee: 0` for NHL and `1000` for NBA/MLB while docs say sports
  feeRate = 0.03, and notes the on-chain CalculatorHelper uses
  `fee = (feeRateBps × min(price, 1−price) × outcomeTokens) / (price × BPS_DIVISOR)` —
  the historical min(p, 1−p) form — vs the docs' p(1−p) form. Unresolved as of retrieval.

---

## Maker rewards

Three distinct programs exist on Intl as of 2026-06-09:

1. **Maker Rebates Program** (docs: market-makers/maker-rebates). Taker fees fund a
   daily rebate pool per category: 20% of crypto fees, 25% for all other fee-enabled
   categories, redistributed to makers. Fee-curve weighted: your share =
   `your_fee_equivalent / total_fee_equivalent`, where fee_equivalent applies the fee
   formula to your maker fills. Paid daily in pUSD directly to wallets; minimum accrued
   payout $1. Rebates calculated per market (since Feb 2026), so makers compete only
   within each market.
2. **Liquidity Rewards** (docs: market-makers/liquidity-rewards). The older dYdX-style
   program: daily distribution at midnight UTC to resting orders scored by size,
   two-sidedness, and tightness to the size-cutoff-adjusted midpoint, subject to
   per-market `min_incentive_size` and `max_incentive_spread`. Min payout $1. Separate
   from (and additive to) maker rebates. Episodic boosts exist (e.g., "$2M+ March
   Madness liquidity rewards", 3.5s minimum resting time).
3. **Taker Rebate Program** (docs: trading/taker-rebates; live May 28, 2026) — for
   takers, not makers, but fee-relevant: 7 tiers (Bronze→Obsidian) on 30-day Weighted
   Volume `wV = Trade Size × (1 − Entry Price) × Category Weight × Bonuses` (weights:
   Sports 1.0, Politics/Finance/Mentions/Tech 1.3, Economics/Culture/Weather/Other 1.7,
   Crypto 2.3, Geopolitics 0). Rebates 3% ($2k wV) up to 50% ($10M+ wV) of taker fees,
   paid daily in pUSD, plus one-time level-up bonuses ($10–$25,000).

---

## Polymarket US status & fees

### Status

- Polymarket acquired QCEX (QCX LLC, a CFTC-licensed DCM + QC Clearing, a DCO) for
  $112M, announced July 2025 (PRNewswire).
- CFTC granted QCX a no-action letter Sept 3, 2025 (CoinDesk) and approved an
  **Amended Order of Designation on Nov 25, 2025** enabling intermediated U.S. access
  (PRNewswire, CoinDesk).
- **Polymarket US (polymarket.us, operated by QCX LLC) launched Dec 3, 2025** and is
  live as of 2026-06-09 (official docs site docs.polymarket.us is active and versioned;
  changelog v0.0.44 dated June 9, 2026). KYC-verified, USD-settled, CFTC-regulated.
  Some states restricted (e.g., NY/NV litigation noted by third parties — not verified
  from an official source).

### Fees (docs.polymarket.us/fees, retrieved 2026-06-09 — verbatim)

"Effective exchange-wide from 3pm ET, Friday April 3, 2026."

- Formula: `Fee = Θ × C × p × (1 - p)` — C contracts, p trade price ($0.01–$0.99),
  Θ the fee coefficient.
- **Taker: Θ = 0.05 → max $1.25 per 100-lot at p = $0.50.**
- **Maker: Θ = −0.0125 → maker RECEIVES up to $0.31 per 100-lot** ("Maker rebate (25%
  of taker fees) is applied at the point of trade").
- Worked example (theirs): buy 1,000 contracts at $0.50 → taker pays
  `0.05 × 1000 × 0.5 × 0.5 = $12.50`; the maker on the other side receives $3.13.
- Rounding: nearest $0.01, banker's rounding (half to even). Fees only on execution
  (never on cancel/expire/reject); deducted from balance at trade time; can round to $0
  on small/extreme-priced trades.
- Taker rebate promo: ">$250,000 in taker volume between May 15, 2026 and June 30, 2026
  (inclusive) receive a taker rebate of 30% of their total taker fees for that period."

So the current US schedule is the same p(1−p) curve shape as Intl, with an effective
peak of 2.5% of notional (taker) at 50¢ ($1.25 / $50), NOT a flat rate.

### The "flat 10bp taker" claim

- No official source found stating 10bp. Launch-era (Nov/Dec 2025) third-party reports
  (Gate News quoting the official fee schedule; Phemex) say: "Taker fee is 0.01% of the
  total contract Premium" — i.e. **1bp on premium, immediately-matched taker orders** —
  with no maker fee mentioned. That schedule was **superseded April 3, 2026**.
- The spec's "flat 10bp" most plausibly garbles the launch-era "0.01%" (1bp). Either
  way it does not describe the venue today.
- No Wayback snapshot of docs.polymarket.us/fees exists before 2026-04-14, so the
  launch-era schedule could not be confirmed from an official archive.

### US incentives & API surfacing

- Volume Incentive Program live May 21, 2026 (rewards by share of eligible taker-side
  notional); market-specific liquidity reward pools (e.g., NBA Playoffs Moneyline,
  $100k per market). Public endpoint: GET incentive programs; earnings bucketed by ET day.
- Fees are not a field on order/execution objects in the published schemas; they appear
  as balance effects: balance ledger entries ("deposits, withdrawals, fills, fees,
  corrections"; REST GET /v1/funding/balance-ledger, hard history floor
  2026-05-01T00:00:00Z) and portfolio activity types `ACTIVITY_TYPE_TAKER_FEE_REBATE`
  and `ACTIVITY_TYPE_LIQUIDITY_PROGRAM` (added May 26, 2026).

---

## Order model facts

### Intl CLOB (clob.polymarket.com, Polygon chain 137)

- Binary outcome tokens (CTF ERC-1155, "conditional tokens"); winning side redeems at
  $1.00. Prices are probabilities "between $0.00 and $1.00".
- "All orders on Polymarket are expressed as limit orders." Market orders = marketable
  limit orders. Orders are EIP-712-signed messages executed on-chain by the Exchange
  contract (v2 domain version "2"); order uniqueness via millisecond `timestamp`
  (nonces removed in v2).
- Order types (verbatim table): **GTC** (rests until filled/cancelled), **GTD**
  (expires at UTC-seconds timestamp; 1-minute security threshold — for 90s expiry send
  now + 1min + 30s), **FOK** (fill entirely immediately or cancel), **FAK** (fill
  available, cancel remainder). FOK/FAK are the market-order types (BUY specifies
  dollars to spend, SELL specifies shares). **Post-only** supported on GTC/GTD only;
  rejected if it would cross — "guarantees you're always the maker, never the taker."
- Maker/taker determination: an order that matches on entry (marketable) is the taker;
  resting orders it hits are makers. Some markets apply a short taker delay before
  matching.
- Tick sizes per market: `0.1`, `0.01`, `0.001`, or `0.0001` (`minimum_tick_size` on
  the market object; `tick_size` on the book; SDK `getTickSize(tokenID)`). Order price
  must conform or be rejected. Book also carries `min_order_size` (example: "5") and
  `neg_risk`.
- Multi-outcome events trade on the separate **Neg Risk CTF Exchange** contract
  (`negRisk: true` required in order options).
- Heartbeat endpoint: if heartbeats stop, all open orders auto-cancel (dead-man's
  switch for bots). Batch ops: post up to 15 orders, cancel up to 3,000 per request.
- SDKs: `@polymarket/clob-client-v2`, `py-clob-client-v2`, plus a Rust SDK
  (`polymarket-client-sdk-v2`); unified TS/Python SDKs in beta.

### US exchange (api.polymarket.us)

- Order entry by `marketSlug`; `ORDER_TYPE_LIMIT` (price required) or
  `ORDER_TYPE_MARKET` (`cashOrderQty` cash-quantity supported).
- TimeInForce enum: `DAY`, `GOOD_TILL_CANCEL`, `GOOD_TILL_DATE` (+ `goodTillTime`),
  `IMMEDIATE_OR_CANCEL`, `FILL_OR_KILL`.
- `participateDontInitiate: true` = post-only / maker-only ("rejected if it would
  immediately match").
- Sides expressed as intents: `BUY_LONG` / `SELL_LONG` (YES), `BUY_SHORT` /
  `SELL_SHORT` (NO); or `outcomeSide` + `action`. Currency USD. Prices $0.01–$0.99
  (fee page); per-instrument `tickSize`, `priceScale`, `fractionalQtyScale` in
  reference data. Decimal contract quantities on markets with `minimumTradeQty` < 1
  ("partial contracts"). Ed25519-signed API auth; synchronous execution and
  `slippageTolerance` options; modify = cancel-replace; batch create/cancel/modify
  up to 20.

---

## Announced upcoming changes (as of 2026-06-09)

- **US, June 11, 2026 5:00 PM ET**: all newly listed instruments become
  partial-contract markets (rollout delayed from an earlier date; long-dated futures
  and all World Cup instruments already are).
- **US, June 11, 2026 3:00–5:00 AM EST**: one-off maintenance window.
- **US, June 30, 2026**: $250k/30% taker-rebate promotion window ends (May 15–Jun 30).
- **Intl**: Taker Rebate Program just launched May 28, 2026; docs note "Category
  weights are set by Polymarket and may change over time." No further fee-rate changes
  announced in either changelog as of today.

---

## Uncertainties (do NOT fill from memory)

1. **US launch-era fee schedule (Dec 2025–Apr 2026)** — the "0.01% of total contract
   premium" taker fee is sourced ONLY from third-party news (Gate, Phemex) quoting an
   announcement; no official copy or archive located (earliest Wayback snapshot of
   docs.polymarket.us/fees is 2026-04-14). The spec's "10bp" could not be matched to
   any source, official or otherwise.
2. **Per-market fee parameters vs the category table** — fd `{r, e, to}` includes an
   exponent, and a third-party March-2026 table (marketmath.io) shows exponents ≠ 1 and
   rates like 0.25/0.072 for some categories. The current official page presents only
   the e = 1 form. Whether any live market today uses e ≠ 1 was not confirmable from
   official docs. Always read `feeSchedule`/`fd` per market rather than hard-coding the
   category table.
3. **GET /fee-rate inconsistencies** — GitHub issue #326 (open) shows `base_fee` of 0
   (NHL) and 1000 (NBA/MLB) disagreeing with documented sports rate 0.03, plus a
   docs-vs-onchain formula mismatch (p(1−p) vs min(p,1−p)/p). Polymarket has not
   responded in the issue. Treat /fee-rate values with suspicion; verify empirically
   against `fee_rate_bps` on real fills before relying on it in a gate.
4. **Help-center article is internally inconsistent** (mixed-era text): it
   simultaneously says "Sell orders are not subject to taker fees" and "taker fees
   apply only to taker orders, collected in shares for buy orders and in pUSD for sell
   orders" (the latter describes the pre-Apr-28 share-collection mechanism). Developer
   docs (takers pay, both sides; makers never; collected in pUSD at match time) treated
   as canonical, but this was not reconciled by any official statement.
5. **The exact January 2026 go-live date and the literal "0.0625" figure** — feeRate
   0.0625 is established by arithmetic from the official changelog ("peaking at 1.56%
   at 50% probability") and news ("~3.15% on a 50-cent contract"), but no surviving
   official page printing "0.0625" verbatim was found.
6. **US state-level availability** (NY/NV restrictions) — third-party claims only; not
   verified against official Polymarket US or state sources.
7. **Intl "effective max rate" marketing figures** (e.g., "1.80% crypto") differ from
   the current table's $1.75/100-share peak; they appear to be hold-overs from the
   March-era (0.072) parameters. The current docs table is treated as authoritative.
8. **15-minute/5-minute crypto markets today** — whether they now use the standard
   crypto 0.07 rate or retain bespoke parameters was not confirmable; query per-market
   `fd` at runtime.
