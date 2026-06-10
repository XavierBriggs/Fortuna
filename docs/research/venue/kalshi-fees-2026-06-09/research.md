# Kalshi fee & order-model research (2026-06-09)

Research conducted 2026-06-09 (America/Los_Angeles; browser retrievals timestamped
2026-06-10 UTC). All facts below were verified against live official sources on that
date. Where a fact comes from a single source, or is inferred rather than quoted, it is
flagged in **Uncertainties**.

Key verification note: `kalshi.com` sits behind a Vercel bot-protection checkpoint that
blocks plain HTTP clients (curl/fetch get HTTP 429 / "Vercel Security Checkpoint"). The
official fee schedule PDF and fee-schedule page were therefore retrieved through a real
browser session (Playwright). The live PDF's SHA-256 was computed in-browser and matched
byte-for-byte against an independently archived copy (Wayback 2026-02-18), confirming
the version analyzed here is the one currently being served.

---

## Sources (URL + retrieval date + what it supports)

| # | Source | Retrieved | Supports |
|---|--------|-----------|----------|
| S1 | https://kalshi.com/docs/kalshi-fee-schedule.pdf (official Fee Schedule PDF, 7 pages, footer "Last updated and effective: Feb 5, 2026"). Live copy fetched in-browser 2026-06-09; SHA-256 `b1a37aa734771f42929ccd9ba90ab845e07303f03f549f1c2f493c08b13d2fc9`, byte-identical (214,972 bytes) to Wayback capture `web.archive.org/web/20260218003606/...` | 2026-06-09 | Taker formula 0.07, maker formula 0.0175, "round up = rounds to the next cent", S&P500/NASDAQ-100 0.035 formula + tables, settlement/membership/deposit/withdrawal fees, maker rounding reimbursement, effective date |
| S2 | https://kalshi.com/fee-schedule (live web fee schedule, rendered in Playwright browser; full non-standard table paginated through 10 pages). Also Wayback snapshot 2026-05-08 for corroboration | 2026-06-09 | "No upcoming fee changes scheduled", standard taker range $0.07–$1.75/100 contracts, per-series maker-fee ranges ($0.02–$0.44 at ×1; $0.01–$0.22 at ×0.5), zero-fee series shown as "no fees", "Zero trading fees for a limited time on Kalshi Perpetuals" banner |
| S3 | https://help.kalshi.com/trading/fees (Kalshi Help Center) | 2026-06-09 | Fee charged as "a transaction fee on the expected earnings on the contract"; maker fees exist on some markets; fees "only charged when a trade is ultimately executed, there are no fees associated with canceling a resting order"; some markets differ (special events); defers detail to PDF |
| S4 | https://docs.kalshi.com/getting_started/fee_rounding (official API docs) | 2026-06-09 | Per-fill fee mechanics: trade fee rounded **up** to nearest $0.0001; rounding fee; whole-cent rebate accumulator across fills of an order; balance precision direct member $0.0001 vs non-direct $0.01; "Net fee = trade fee + rounding fee − rebate (always >= $0.00)" |
| S5 | https://docs.kalshi.com/api-reference/exchange/get-series-fee-changes (`GET /series/fee_changes`) | 2026-06-09 | Fee-change schema: `fee_type` enum (`quadratic`, `quadratic_with_maker_fees`, `flat`), `fee_multiplier` (number), `scheduled_ts` |
| S6 | https://docs.kalshi.com/api-reference/market/get-series | 2026-06-09 | Series fee fields: `fee_type` ("quadratic – described by the General Trading Fees Table; quadratic_with_maker_fees – General Trading Fees Table with maker fees section; flat – described by the Specific Trading Fees Table"); `fee_multiplier` "a floating point multiplier applied to the fee calculations" |
| S7 | Live public trade API `https://api.elections.kalshi.com/trade-api/v2/series` (all 10,801 series paginated) and `/series/fee_changes?show_historical=true`; also `/markets/KXGREENLAND-29`, `/markets?series_ticker=KXHIGHNY` | 2026-06-09 | Current per-series fee table (counts below), empty upcoming-fee-change list, recent historical fee changes, `price_level_structure` values, `notional_value_dollars: "1.0000"`, `market_type: "binary"` |
| S8 | https://docs.kalshi.com/api-reference/orders/create-order | 2026-06-09 | Order model: side/action enums, price fields and ranges, `client_order_id`, `time_in_force`, `post_only`, `buy_max_cost`, self-trade prevention, response fee fields (`taker_fees_dollars`, `maker_fees_dollars`) |
| S9 | https://docs.kalshi.com/changelog | 2026-06-09 | `fee_cost` on Fills API (Jan 28, 2026) and fill WebSocket (Jan 29, 2026); market order type removed (Feb 11, 2026); limit-only since Sep 25, 2025; sub-penny live Mar 9, 2026 on 2 markets; rate-limit overhaul Apr 23, 2026; tier automation Jun 5, 2026; announced Jun 11, 2026 changes |
| S10 | https://docs.kalshi.com/getting_started/rate_limits | 2026-06-09 | Tier table (Basic/Advanced/Premier/Paragon/Prime), 10-token default request cost, separate read/write buckets, burst capacity, volume-based tier qualification |
| S11 | https://docs.kalshi.com/getting_started/market_settlement | 2026-06-09 | "$1 per contract" payout to winning side; "Settlement fees are zero for simple yes/no determinations but may apply for sub-cent scalar settlement"; payout rounded to whole cents |
| S12 | docs.kalshi.com FIX/REST docs surfaced via search (order-entry, error-handling, quick-start) | 2026-06-09 | `client_order_id` idempotency: resubmission with same id is rejected as duplicate (`ORDER_ALREADY_EXISTS` / FIX OrdRejReason "Duplicate order") |
| S13 | https://docs.kalshi.com/getting_started/fixed_point_migration + market objects (S7) | 2026-06-09 | `deci_cent` = $0.001 ticks across full range; `tapered_deci_cent` = $0.001 ticks below $0.10 and above $0.90, $0.01 in the middle; `linear_cent` observed on standard markets |
| S14 | Secondary cross-checks (non-official, used only for agreement, never as primary): marketmath.io/platforms/kalshi, predictionhunt.com Kalshi fee guide 2026, oddsassist.com Kalshi fee calculator, whirligigbear.substack.com "Maker/Taker Math on Kalshi" | 2026-06-09 | All agree: taker ceil(0.07·C·P·(1−P)), maker ceil(0.0175·C·P·(1−P)), no settlement fee, ACH free, debit ~2% |

---

## Fee formulas (exact, with worked examples)

### General (taker) fee — applies to all markets except series with a different multiplier

Quoted from the Fee Schedule PDF (S1, effective Feb 5, 2026):

> "Trading fees are only charged for orders that are immediately matched with orders
> sitting on the orderbook. Trading fees are not charged for orders placed that are not
> immediately matched and are instead left as resting orders on the orderbook unless
> they are included in our 'Maker Fees' section."
>
> fees = round up(0.07 x C x P x (1-P))
> P = the price of a contract in dollars (50 cents is 0.5)
> C = the number of contracts being traded
> round up = rounds to the next cent

With the per-series multiplier `m` (from the API, S6/S7), the operative general form is:

```
taker_fee = ceil_to_cent(m × 0.07 × C × P × (1 − P))      m = 1 for most markets
maker_fee = ceil_to_cent(m × 0.0175 × C × P × (1 − P))    only on maker-fee series
```

The ceiling applies to the **total** of the computation (C inside the formula), not per
contract — confirmed by the PDF's own table (1 contract at $0.50 → $0.02 = ceil(0.0175);
100 contracts at $0.50 → $1.75 exactly).

Worked examples (all cross-checked against the PDF's printed General Trading Fees Table, S1):

| Trade | Computation | Fee |
|---|---|---|
| 100 contracts @ 50¢, general taker | 0.07 × 100 × 0.5 × 0.5 = 1.75 | **$1.75** (table ✓) |
| 100 @ 25¢, general taker | 0.07 × 100 × 0.25 × 0.75 = 1.3125 → ceil | **$1.32** (table ✓) |
| 100 @ 1¢ or 99¢, general taker | 0.07 × 100 × 0.01 × 0.99 = 0.0693 → ceil | **$0.07** (table ✓) |
| 1 @ 50¢, general taker | 0.07 × 1 × 0.25 = 0.0175 → ceil | **$0.02** (table ✓) |
| 100 @ 50¢, S&P/Nasdaq taker (m = 0.5) | 0.035 × 100 × 0.25 = 0.875 → ceil | **$0.88** (PDF table ✓) |
| 100 @ 50¢, maker (m = 1 series) | 0.0175 × 100 × 0.25 = 0.4375 → ceil | **$0.44** (live page range ✓) |
| 100 @ 50¢, maker (m = 0.5, e.g. KXINXY) | 0.5 × 0.0175 × 100 × 0.25 = 0.21875 → ceil | **$0.22** (live page range ✓) |

Fee is maximal at P = $0.50 (1.75¢/contract taker) and shrinks toward the tails
(0.07¢/contract at 1¢/99¢ before rounding).

---

## Maker fees

- Formula (S1): `fees = round up(0.0175 x C x P x (1-P))` — exactly **1/4 of the taker
  coefficient**. Same quadratic shape, same next-cent round-up.
- Maker fees apply **only on designated series**. API representation: series with
  `fee_type = "quadratic_with_maker_fees"` (S6/S7). On all other (`quadratic`) series,
  resting orders that get filled pay **zero** fee; only the taker side pays.
- Quoted (S1): "Maker fees are charged for orders placed that are not immediately
  matched and are instead left as resting orders on the orderbook. These fees are only
  charged when a trade is ultimately executed, there are no fees associated with
  canceling a resting order." (Help Center S3 says the same.)
- Rounding reimbursement (S1): "Users who pay more in maker fees as a result of rounding
  will be reimbursed in the first week of the following month if their reimbursement
  exceeds $10." Below $10/month of excess, no reimbursement. (Note: the newer per-fill
  centicent accounting in S4 largely supersedes the practical impact of this.)
- **Which series, as of 2026-06-09** (live API enumeration, S7): exactly **128 of
  10,801 series** have `quadratic_with_maker_fees` at multiplier 1, plus **2** at
  multiplier 0.5 (KXINXY, KXNASDAQ100Y). The 128 are overwhelmingly Sports (single-game,
  spread, total, and championship series for NFL/NBA/MLB/NHL/NCAA/soccer leagues/tennis/
  golf/F1/NASCAR), plus key Economics releases (KXCPI, KXCPIYOY, KXFED, KXFEDDECISION,
  KXGDP, KXPAYROLLS, KXU3, KXRATECUTCOUNT, KXEGGS, KXAAAGASM), Emmys/entertainment
  awards, KXIPO, KXLLM1, KXBTCMAX125/150. Full ticker list:

  KXAAAGASM, KXAOMENSINGLES, KXATPMATCH, KXBALLONDOR, KXBTCMAX125, KXBTCMAX150,
  KXBUNDESLIGA, KXBUNDESLIGAGAME, KXCLUBWC, KXCONNSMYTHE, KXCPI, KXCPIYOY, KXEGGS,
  KXEMMYCACTO, KXEMMYCACTR, KXEMMYCSERIES, KXEMMYDACTO, KXEMMYDACTR, KXEMMYDSERIES,
  KXEPLGAME, KXEPLTOP4, KXF1RACE, KXF1RACEPODIUM, KXFED, KXFEDDECISION, KXFOMEN,
  KXFOMENSINGLES, KXFOWOMEN, KXFOWOMENSINGLES, KXGDP, KXHEISMAN, KXINDY500, KXIPO,
  KXLALIGA, KXLALIGAGAME, KXLIGUE1, KXLIGUE1GAME, KXLLM1, KXMARMAD, KXMENWORLDCUP,
  KXMLB, KXMLBAL, KXMLBASGAME, KXMLBGAME, KXMLBHRDERBY, KXMLBNL, KXMLBSERIES, KXNASCAR,
  KXNASCARRACE, KXNASCARRACEOLD, KXNATHANDOGS, KXNATHANSHD, KXNBA, KXNBACOY, KXNBAEAST,
  KXNBAFINALSMVP, KXNBAGAME, KXNBAMVP, KXNBAROY, KXNBASERIES, KXNBASPREAD, KXNBATOTAL,
  KXNBAWEST, KXNCAAF, KXNCAAFACC, KXNCAAFB10, KXNCAAFB12, KXNCAAFGAME, KXNCAAFPLAYOFF,
  KXNCAAFSEC, KXNCAAFSPREAD, KXNCAAFTOTAL, KXNCAAMBGAME, KXNCAAMBSPREAD, KXNCAAMBTOTAL,
  KXNFL2TD, KXNFLAFCCHAMP, KXNFLAFCEAST, KXNFLAFCNORTH, KXNFLAFCSOUTH, KXNFLAFCWEST,
  KXNFLANYTD, KXNFLCOTY, KXNFLCPOTY, KXNFLDPOTY, KXNFLDROTY, KXNFLFIRSTTD, KXNFLGAME,
  KXNFLMVP, KXNFLNFCCHAMP, KXNFLNFCEAST, KXNFLNFCNORTH, KXNFLNFCSOUTH, KXNFLNFCWEST,
  KXNFLOPOTY, KXNFLOROTY, KXNFLSPREAD, KXNFLTOTAL, KXNHL, KXNHLEAST, KXNHLGAME,
  KXNHLSERIES, KXNHLWEST, KXPAYROLLS, KXPGA, KXPGARYDER, KXPGASOLHEIM, KXPGATOUR,
  KXRATECUTCOUNT, KXSB, KXSERIEA, KXSERIEAGAME, KXSUPERBOWLHEADLINE, KXTHEOPEN,
  KXTOURDEFRANCE, KXU3, KXUCL, KXUCLGAME, KXUEFACL, KXUSOMENSINGLES, KXUSOPEN,
  KXUSOWOMENSINGLES, KXWCGAME, KXWMENSINGLES, KXWNBA, KXWNBAGAME, KXWTAMATCH,
  KXWWOMENSINGLES

- Historical note (was true in earlier years, NOT true now): the old flat
  ~$0.0025/contract maker fee model is gone; current maker fees are quadratic at the
  0.0175 coefficient. No official source retrieved on 2026-06-09 describes a flat
  per-contract maker fee.
- The set changes over time. Query it programmatically: per-series
  `GET /trade-api/v2/series/{ticker}` → `fee_type`/`fee_multiplier`; scheduled changes
  via `GET /trade-api/v2/series/fee_changes` (S5/S7). Example from the historical log:
  KXWCGAME switched to `quadratic_with_maker_fees` effective 2026-06-05T22:00Z.

---

## Series-specific schedules / multipliers (table)

Current state from full enumeration of all 10,801 series via the live API on 2026-06-09
(S7), cross-checked against the live fee-schedule page (S2):

| fee_type | fee_multiplier | # series | Effective formula (taker / maker) | Series |
|---|---|---|---|---|
| quadratic | 1 | 10,651 | ceil(0.07·C·P·(1−P)) / no maker fee | All other markets ("Most markets — $0.07–$1.75 taker fees" per 100 contracts, S2) |
| quadratic_with_maker_fees | 1 | 128 | ceil(0.07·C·P·(1−P)) / ceil(0.0175·C·P·(1−P)) ($0.02–$0.44 maker per 100, S2) | List above |
| quadratic | 0.5 | 7 | ceil(0.035·C·P·(1−P)) / no maker fee ($0.04–$0.88 per 100, S2) | KXINX, KXINXMAXY, KXINXMINY, KXINXPOS, KXINXU, KXNASDAQ100, KXNASDAQ100U |
| quadratic_with_maker_fees | 0.5 | 2 | ceil(0.035·C·P·(1−P)) / ceil(0.00875·C·P·(1−P)) (maker $0.01–$0.22 per 100, S2) | KXINXY, KXNASDAQ100Y |
| quadratic | 0 | 13 | zero fees ("no fees" on S2) | KXBTCY, KXCITRINI, KXDOED, KXELECTIRAN, KXETHY, KXEXPAND, KXGAMBLINGREPEAL, KXGREENLAND, KXIRANDEMOCRACY, KXLAYOFFSYINFO, KXNEXTIRANLEADER, KXPAHLAVIHEAD, KXTRUMPOUT |
| flat | — | 0 | (defined in API enum; **no live series uses it**) | — |

- The PDF (S1) expresses the S&P500/NASDAQ-100 schedule as its own formula —
  "fees = round up(0.035 x C x P x (1-P))" for all markets whose Rulebook ticker begins
  with INX or NASDAQ100 — which is identical to the API's representation as
  `quadratic` with `fee_multiplier = 0.5`.
- Kalshi **perpetual futures** (KX*PERP series) were set to `fee_multiplier = 0`
  effective 2026-06-03/06-08 per the fee-change history (S7), matching the site banner
  "Zero trading fees for a limited time on Kalshi Perpetuals" (S2). Perps otherwise use
  a separate margin fee model: `GET /trade-api/v2/margin/fee_tiers` returns maker (and,
  per a changelog entry dated 2026-06-11, taker) fee rates as decimal fractions of
  notional (e.g. 0.0005 = 5 bps) (S9). Out of scope for event-contract trading but
  noted for completeness.

---

## Rounding

Two layers, both official:

1. **Fee-schedule statement (S1):** the trading-fee formula rounds **up to the next
   cent**, applied to the total fee of the trade (not per contract). This is the rule
   embodied in the printed fee tables and is the conservative bound for cost modeling.
2. **Current exchange mechanics (S4, API docs "Fee Rounding"):** fees are assessed
   **per fill**:
   - "Trade fee: Fee from the fee model, rounded up to the nearest $0.0001 (centicent)."
   - `balance_change = revenue − trade_fee` is floored toward −∞ to the user's balance
     precision; the difference is charged as a "rounding fee".
   - Balance precision: direct members $0.0001; **non-direct members $0.01** (typical
     API users are non-direct).
   - A per-order **fee accumulator** tracks cumulative rounding overpayment "across all
     fills of an order. Once the accumulated rounding exceeds $0.01, a whole-cent rebate
     is issued and the accumulator is reduced by $0.01."
   - "Net fee = trade fee + rounding fee − rebate (always >= $0.00)."
   Net effect: multi-fill orders converge to (approximately) the same total fee as a
   single-fill computation; you never pay less than the centicent-rounded model fee, and
   for non-direct members the cash impact stays in whole cents.
3. Maker-fee rounding overpayment is additionally reimbursable monthly when it exceeds
   $10 (S1).

**For conservative system modeling: `fee = ceil_to_cent(m × k × C × P × (1−P))` per
order per execution side (k = 0.07 taker / 0.0175 maker) is an upper bound that matches
the published tables exactly.**

---

## Order model facts

From Create Order docs (S8), changelog (S9), settlement docs (S11), and live market
objects (S7):

- **Contract payout:** $1.00 per contract to the winning side
  (`notional_value_dollars: "1.0000"`; S11: "Yes contract holders receive $1 per
  contract"). Prices are implied probabilities of $1.
- **Sides:** `side` ∈ {`yes`, `no`}. **Actions:** `action` ∈ {`buy`, `sell`}. Response
  also exposes `outcome_side` (yes/no) and `book_side` (bid/ask).
- **Order types: limit only.** `type=market` was removed from `POST /portfolio/orders`
  on 2026-02-11; since 2025-09-25 "only limit type orders will be supported" (S9).
  Marketable behavior is achieved with aggressive limit prices + IOC/FOK.
- **Time in force:** `time_in_force` ∈ {`good_till_canceled`, `immediate_or_cancel`,
  `fill_or_kill`}. `expiration_ts` (Unix seconds) gives timed expiry; orders with
  `expiration_ts` in the past are rejected, and IOC cannot be combined with
  `expiration_ts` (breaking changes 2025-11-21, S9).
- **Prices:** integer cents `yes_price`/`no_price` ∈ **1–99**; or fixed-point dollar
  strings `yes_price_dollars`/`no_price_dollars` ("up to 6 decimal places", S8).
  Standard markets have `price_level_structure = "linear_cent"` (1¢ ticks). Sub-penny
  structures exist and went **live 2026-03-09 on 2 markets**: `deci_cent` ($0.001 ticks
  across the range) and `tapered_deci_cent` ($0.001 ticks below $0.10 / above $0.90,
  1¢ in the middle) — observed live: KXGREENLAND-29 is `tapered_deci_cent` (S7, S9,
  S13). `price_level_structure` is a **market-level** field. Quantities likewise have
  fixed-point variants (`count_fp`).
- **Idempotency:** `client_order_id` is supported; resubmitting with the same
  `client_order_id` is rejected as a duplicate (`ORDER_ALREADY_EXISTS`; FIX
  OrdRejReason "Duplicate order") — safe retry semantics (S12).
- **Other order controls:** `post_only` (bool — reject rather than cross),
  `reduce_only`, `buy_max_cost` (cents cap; implies fill-or-kill behavior),
  `self_trade_prevention_type` ∈ {`taker_at_cross`, `maker`}, `cancel_order_on_pause`,
  `order_group_id` (order groups), `subaccount` (int ≥ 0).
- **Order statuses:** `resting` | `canceled` | `executed`.
- **Where fees appear in the API:**
  - Order object: `taker_fees_dollars`, `maker_fees_dollars`, plus
    `taker_fill_cost_dollars` / `maker_fill_cost_dollars` (fixed-point dollar strings;
    added with the sub-penny migration 2025-10-09).
  - Fills: `fee_cost` on the Fills API **since 2026-01-28**, and on fill WebSocket
    messages (fixed-point dollars string) since 2026-01-29 (S9). Note
    `client_order_id` was removed from `GET /portfolio/fills` responses.
  - Settlements: `GET /portfolio/settlements` returns the sum of trade fees paid on the
    settled position (since 2025-11-13) — reporting only; **no settlement fee is
    charged**.
- **When fees are charged:** at trade execution, per fill. Taker fee on the
  immediately-matching side of every trade; maker fee only on maker-fee series, charged
  when the resting order fills. Placing and canceling orders is free. Both sides pay
  only in `quadratic_with_maker_fees` series (taker at 0.07·m, maker at 0.0175·m);
  in standard series only the taker pays.
- **Settlement fees:** none. (S1: "There is no settlement fee."; S11 adds the only
  caveat: "Settlement fees are zero for simple yes/no determinations but may apply for
  sub-cent scalar settlement.")
- **Membership fee:** none (S1).
- **Deposits/withdrawals (S1):** ACH deposits and withdrawals free. Wire: no Kalshi fee
  (bank fees may apply); wire **withdrawals not supported under $500,000**. Debit-card
  deposits: max 2% fee. Crypto deposits/withdrawals: third-party processor fees may
  apply, disclosed pre-transaction. FCM-intermediated customers may face different FCM
  fees.

---

## Rate limits

Token-based system (introduced 2026-04-23, replacing the old scheme; S9, S10). Two
independent budgets: **Read** (GETs) and **Write** (order create/amend/cancel, order
groups, RFQ/quotes). Most requests cost the **default 10 tokens**; some ops are cheaper
(order cancels, single-order reads, quote create/cancel, multivariate-collection
lookup). Batch order endpoints bill **per item** (N orders = N × 10 tokens).

| Tier | Read tokens/s | Write tokens/s | ≈ default requests/s (R/W) |
|---|---|---|---|
| Basic | 200 | 100 | 20 / 10 |
| Advanced | 300 | 300 | 30 / 30 |
| Premier | 1,000 | 1,000 | 100 / 100 |
| Paragon | 2,000 | 2,000 | 200 / 200 |
| Prime | 4,000 | 4,000 | 400 / 400 |

- Burst: the Write bucket holds **2 seconds** of budget (Basic: 1 second), refills
  continuously; exceeding it returns HTTP 429.
- Tier acquisition: Basic automatic at signup; Advanced via the "Upgrade Account API
  Usage Level" endpoint; Premier/Paragon/Prime earned by 30-day volume share of
  exchange volume (earn 0.25% / 0.50% / 1.00%; keep 0.20% / 0.40% / 0.80%) — automated
  as of 2026-06-05; self-serve endpoints
  (`GET .../account/api_usage_level/volume_progress`,
  `POST .../account/api_usage_level/upgrade`) listed in the changelog dated 2026-06-11
  (announced/imminent at research time).
- `GET /trade-api/v2/account/limits` returns per-bucket `refill_rate` and
  `bucket_capacity` (since 2026-04-30).
- Legacy (non-V2) endpoints now cost 5×–10× the V2 equivalents (changelog 2026-06-01
  and 2026-06-04) — use V2.
- Perps/margin API has its own separate read/write buckets.

---

## Effective dates & upcoming changes

- Current official fee schedule: **"Last updated and effective: Feb 5, 2026"** (PDF
  footer, S1). Verified still the live document on 2026-06-09 via in-browser SHA-256
  match.
- **"No upcoming fee changes scheduled."** — live fee-schedule page, 2026-06-09 (S2);
  corroborated by `GET /series/fee_changes` returning an empty array (S7).
- Recent per-series changes (from `show_historical=true`, S7): all KX*PERP crypto
  perp series → multiplier 0 (zero fees) effective 2026-06-03 to 2026-06-08; KXWCGAME →
  `quadratic_with_maker_fees` effective 2026-06-05; several political one-offs →
  multiplier 0 effective 2026-03-03.
- Announced/imminent API changes (changelog entries dated 2026-06-11, two days after
  research date): margin `fee_tiers` to return both maker and taker rates; self-serve
  rate-limit tier endpoints.

---

## Uncertainties

Everything in this section is flagged because it is **not fully nailed down by an
official source retrieved on 2026-06-09**:

1. **`flat` fee_type semantics.** The API enum defines `flat` and the Get Series doc
   says it is "described by the Specific Trading Fees Table" — yet the PDF's Specific
   (S&P/Nasdaq) table is a 0.035 **quadratic**, and the live S&P/Nasdaq series are
   represented as `quadratic` × 0.5. **Zero of 10,801 live series use `flat`.** Treat
   `flat` as a reserved/possible future fee shape with officially ambiguous semantics;
   handle it explicitly (e.g., refuse to trade a series with unknown fee math) rather
   than guessing.
2. **Maker scaling under fee_multiplier** (maker = m × 0.0175) is **inferred** from the
   doc wording ("multiplier applied to the fee calculations") plus exact agreement of
   the live page's KXINXY/KXNASDAQ100Y maker range ($0.01–$0.22 per 100 contracts =
   ceil(0.5 × 0.0175 × 100 × P(1−P)) at the extremes). Strong evidence, but no official
   sentence states "the multiplier also applies to maker fees".
3. **Per-order vs per-fill rounding interplay.** The PDF states next-cent round-up per
   trade; the API docs describe per-fill centicent rounding with a per-order accumulator
   and whole-cent rebates. I could not retrieve an official worked example reconciling
   the two for a multi-fill order for a non-direct member. The conservative bound
   (ceil-to-cent per fill) is safe; actual charges may be up to a cent less per order.
4. **Sub-cent scalar settlement fees.** S11 says settlement fees "may apply for
   sub-cent scalar settlement" with no published rate. Binary yes/no settlement is
   fee-free; if FORTUNA ever touches sub-cent scalar markets, this needs fresh research.
5. **Website non-standard table vs API enumeration.** The live fee-schedule page table
   paginated to ~94 distinct rows; the full API series enumeration yields 150
   non-standard series (the site appears to show only series with currently-listed
   markets). The API list is taken as authoritative for the current state.
6. **`linear_cent`** as the standard `price_level_structure` value is observed on live
   market objects; its exact tick semantics (1¢ uniformly) are inferred from the name,
   observed 1–99¢ pricing, and the documented contrast with `deci_cent`/
   `tapered_deci_cent`.
7. **help.kalshi.com depth.** The help center fees page was reachable and consistent,
   but it defers all formulas to the PDF; no separate official help article restating
   the 0.0175 maker coefficient was retrieved (secondary sources S14 agree with the PDF).
8. **Changelog entries dated 2026-06-11** (margin taker rates; self-serve tier
   endpoints) are dated after the research date — treat as announced, not yet verified
   live.
9. **Old INX/NASDAQ100 (non-KX) rulebook tickers** named in the PDF (INXD, INXW, ...)
   mostly do not resolve on the v2 API (the live equivalents are the KX-prefixed
   series; a legacy `INX` series still appears in category listings with multiplier 1
   but appears inactive). For fee purposes follow the per-series API fields, not ticker
   prefixes.
10. **Rate-limit token costs of specific cheaper endpoints** ("order cancellations,
    single-order reads, quote create/cancel...") — exact reduced token numbers were not
    captured; check each endpoint's API-reference page when implementing.
